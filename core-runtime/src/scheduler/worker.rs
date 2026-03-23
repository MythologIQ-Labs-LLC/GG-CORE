//! Single worker loop: dequeue requests and execute inference.
//!
//! All inference (regular and streaming) goes through this worker.
//! The IPC handler enqueues requests and awaits responses.

use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::engine::InferenceEngine;
use crate::memory::ResourceLimits;
use crate::models::lifecycle::ModelLifecycle;
use crate::models::registry::ModelRegistry;
use crate::telemetry;
use super::queue::{QueuedRequest, RequestQueue};
use super::worker_streaming;

/// Spawn the worker loop. Returns a handle for shutdown.
pub fn spawn_worker(
    queue: Arc<RequestQueue>,
    engine: Arc<InferenceEngine>,
    shutdown: tokio_util::sync::CancellationToken,
) -> JoinHandle<()> {
    spawn_worker_with_registry(queue, engine, None, None, None, shutdown)
}

/// Spawn with optional registry and resource limits.
pub fn spawn_worker_with_registry(
    queue: Arc<RequestQueue>,
    engine: Arc<InferenceEngine>,
    lifecycle: Option<Arc<ModelLifecycle>>,
    registry: Option<Arc<ModelRegistry>>,
    resource_limits: Option<ResourceLimits>,
    shutdown: tokio_util::sync::CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        worker_loop(
            &queue, &engine,
            lifecycle.as_deref(), registry.as_deref(),
            resource_limits.as_ref(), shutdown,
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
    shutdown: tokio_util::sync::CancellationToken,
) {
    loop {
        // Check streaming queue first (non-blocking), then wait on main.
        if let Some(sreq) = queue.dequeue_streaming().await {
            worker_streaming::execute(engine, resource_limits, sreq).await;
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
                        resource_limits, request,
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
    request: QueuedRequest,
) {
    let model_id = request.model_id.clone();
    let cancelled = request.cancel_check();

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
        engine, resource_limits, &model_id,
        &request.prompt, &request.params, cancelled,
    ).await;
    let latency_ms = start.elapsed().as_millis() as u64;

    record_result(&result, &model_id, latency_ms, lifecycle, registry).await;
    send_response(request, result.map_err(|e| e.to_string()));
}

type GuardResult = Result<Option<crate::memory::ResourceGuard>, String>;

pub(super) async fn acquire_guard(
    engine: &InferenceEngine,
    limits: Option<&ResourceLimits>,
    model_id: &str,
) -> GuardResult {
    let Some(limits) = limits else { return Ok(None) };
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
) -> Result<crate::engine::inference::InferenceResult, crate::engine::inference::InferenceError> {
    if let Some(limits) = resource_limits {
        engine
            .run_cancellable_with_memory_limit(
                model_id, prompt, params, cancelled,
                limits.max_memory_per_call(),
            )
            .await
    } else {
        engine.run_cancellable(model_id, prompt, params, cancelled).await
    }
}

async fn record_result(
    result: &Result<crate::engine::inference::InferenceResult, crate::engine::inference::InferenceError>,
    model_id: &str,
    latency_ms: u64,
    lifecycle: Option<&ModelLifecycle>,
    registry: Option<&ModelRegistry>,
) {
    match result {
        Ok(r) => {
            telemetry::record_request_success(
                model_id, latency_ms, r.tokens_generated as u64,
            );
            if let (Some(lc), Some(reg)) = (lifecycle, registry) {
                if let Some(handle) = lc.get_handle(model_id).await {
                    reg.record_request(handle, latency_ms as f64).await;
                }
            }
        }
        Err(e) => {
            telemetry::record_request_failure(model_id, &e.to_string());
        }
    }
}

async fn estimate_memory(engine: &InferenceEngine, model_id: &str) -> usize {
    const FALLBACK_BYTES: usize = 256 * 1024 * 1024;
    engine.model_memory_usage(model_id).await.unwrap_or(FALLBACK_BYTES)
}

fn send_response(
    request: QueuedRequest,
    result: Result<crate::engine::inference::InferenceResult, String>,
) {
    if let Some(tx) = request.response_tx {
        let _ = tx.send(result);
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
