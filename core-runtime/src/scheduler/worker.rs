//! Single worker loop: dequeue requests and execute inference.
//!
//! All inference (regular and streaming) goes through this worker.
//! The IPC handler enqueues requests and awaits responses.

use std::sync::Arc;
use tokio::task::JoinHandle;

use super::queue::{QueuedRequest, RequestQueue};
use super::worker_streaming;
use crate::engine::inference::{InferenceError, InferenceResult};
use crate::engine::InferenceEngine;
use crate::memory::ResourceLimits;
use crate::models::lifecycle::ModelLifecycle;
use crate::models::registry::ModelRegistry;
use crate::security::SecurityPipeline;
use crate::telemetry;

/// Spawn the worker loop. Returns a handle for shutdown.
///
/// No security pipeline is attached; production callers must use
/// `spawn_worker_with_registry` with `Some(pipeline)`.
pub fn spawn_worker(
    queue: Arc<RequestQueue>,
    engine: Arc<InferenceEngine>,
    shutdown: tokio_util::sync::CancellationToken,
) -> JoinHandle<()> {
    spawn_worker_with_registry(queue, engine, None, None, None, None, shutdown)
}

/// Spawn with optional registry, resource limits, and security pipeline.
pub fn spawn_worker_with_registry(
    queue: Arc<RequestQueue>,
    engine: Arc<InferenceEngine>,
    lifecycle: Option<Arc<ModelLifecycle>>,
    registry: Option<Arc<ModelRegistry>>,
    resource_limits: Option<ResourceLimits>,
    security: Option<Arc<SecurityPipeline>>,
    shutdown: tokio_util::sync::CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        worker_loop(
            &queue,
            &engine,
            lifecycle.as_deref(),
            registry.as_deref(),
            resource_limits.as_ref(),
            security.as_deref(),
            shutdown,
        )
        .await;
    })
}

async fn worker_loop(
    queue: &RequestQueue,
    engine: &InferenceEngine,
    lifecycle: Option<&ModelLifecycle>,
    registry: Option<&ModelRegistry>,
    resource_limits: Option<&ResourceLimits>,
    security: Option<&SecurityPipeline>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    loop {
        // Check streaming queue first (non-blocking), then wait on main.
        if let Some(sreq) = queue.dequeue_streaming().await {
            worker_streaming::execute(engine, resource_limits, security, sreq).await;
            continue;
        }
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                tracing::info!("worker: shutdown signal received");
                break;
            }
            req_opt = queue.wait_and_dequeue() => {
                if let Some(request) = req_opt {
                    execute_request(
                        engine, lifecycle, registry,
                        resource_limits, security, request,
                    ).await;
                }
            }
        }
    }
}

async fn execute_request(
    engine: &InferenceEngine,
    lifecycle: Option<&ModelLifecycle>,
    registry: Option<&ModelRegistry>,
    resource_limits: Option<&ResourceLimits>,
    security: Option<&SecurityPipeline>,
    request: QueuedRequest,
) {
    let model_id = request.model_id.clone();
    let cancelled = request.cancel_check();

    let Some(request) = scan_ingress(security, request) else {
        return;
    };

    let _guard = match acquire_guard(engine, resource_limits, &model_id).await {
        Ok(g) => g,
        Err(msg) => {
            telemetry::record_admission_rejection(&model_id, &msg);
            send_response(request, Err(msg));
            return;
        }
    };
    let start = std::time::Instant::now();
    let result = run_inference(
        engine,
        resource_limits,
        &model_id,
        &request.prompt,
        &request.params,
        cancelled,
    )
    .await;
    let latency_ms = start.elapsed().as_millis() as u64;

    let result = apply_egress(security, &model_id, result);
    record_result(&result, &model_id, latency_ms, lifecycle, registry).await;
    send_response(request, result.map_err(|e| e.to_string()));
}

/// Ingress scan. Returns the request when admitted; on rejection sends the
/// policy error to the caller and returns `None`.
fn scan_ingress(
    security: Option<&SecurityPipeline>,
    request: QueuedRequest,
) -> Option<QueuedRequest> {
    let Some(sec) = security else {
        return Some(request);
    };
    let verdict = sec.scan_prompt(&request.prompt);
    telemetry::record_security_scan(
        &request.model_id,
        verdict.latency_us,
        verdict.risk_score,
        !verdict.allowed,
    );
    if verdict.allowed {
        Some(request)
    } else {
        send_response(request, Err("request rejected by security policy".into()));
        None
    }
}

/// Egress sanitization on successful output; errors pass through untouched.
fn apply_egress(
    security: Option<&SecurityPipeline>,
    model_id: &str,
    result: InferResult,
) -> InferResult {
    let Some(sec) = security else {
        return result;
    };
    result.map(|mut r| {
        let s = sec.sanitize_output(&r.output);
        telemetry::record_output_sanitize(model_id, s.latency_us, s.redactions as u64);
        r.output = s.output;
        r
    })
}

type GuardResult = Result<Option<crate::memory::ResourceGuard>, String>;

type InferResult = Result<InferenceResult, InferenceError>;

pub(super) async fn acquire_guard(
    engine: &InferenceEngine,
    limits: Option<&ResourceLimits>,
    model_id: &str,
) -> GuardResult {
    let Some(limits) = limits else {
        return Ok(None);
    };
    let mem = estimate_memory(engine, model_id).await;
    limits.try_acquire(mem).map(Some).map_err(|e| e.to_string())
}

async fn run_inference(
    engine: &InferenceEngine,
    resource_limits: Option<&ResourceLimits>,
    model_id: &str,
    prompt: &str,
    params: &crate::engine::InferenceParams,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) -> InferResult {
    if let Some(limits) = resource_limits {
        engine
            .run_cancellable_with_memory_limit(
                model_id,
                prompt,
                params,
                cancelled,
                limits.max_memory_per_call(),
            )
            .await
    } else {
        engine
            .run_cancellable(model_id, prompt, params, cancelled)
            .await
    }
}

async fn record_result(
    result: &InferResult,
    model_id: &str,
    latency_ms: u64,
    lifecycle: Option<&ModelLifecycle>,
    registry: Option<&ModelRegistry>,
) {
    match result {
        Ok(r) => {
            telemetry::record_request_success(model_id, latency_ms, r.tokens_generated as u64);
            if let (Some(lc), Some(reg)) = (lifecycle, registry) {
                if let Some(handle) = lc.get_handle(model_id).await {
                    reg.record_request(handle, latency_ms as f64).await;
                }
            }
        }
        Err(e) => telemetry::record_request_failure(model_id, &e.to_string()),
    }
}

async fn estimate_memory(engine: &InferenceEngine, model_id: &str) -> usize {
    const FALLBACK_BYTES: usize = 256 * 1024 * 1024;
    engine
        .model_memory_usage(model_id)
        .await
        .unwrap_or(FALLBACK_BYTES)
}

fn send_response(request: QueuedRequest, result: Result<InferenceResult, String>) {
    if let Some(tx) = request.response_tx {
        let _ = tx.send(result);
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "worker_security_tests.rs"]
mod security_tests;
