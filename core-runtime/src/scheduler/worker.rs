//! Worker execution: dequeue requests and execute inference.
//!
//! All inference (regular, streaming and typed) goes through the worker
//! loops in `worker_pool`. The IPC handler enqueues requests and awaits
//! responses.

use std::sync::Arc;
use tokio::task::JoinHandle;

use super::queue::{QueuedRequest, RequestQueue};
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
/// Runs `max_concurrent` worker loops (1 without resource limits); see
/// `worker_pool`.
pub fn spawn_worker_with_registry(
    queue: Arc<RequestQueue>,
    engine: Arc<InferenceEngine>,
    lifecycle: Option<Arc<ModelLifecycle>>,
    registry: Option<Arc<ModelRegistry>>,
    resource_limits: Option<ResourceLimits>,
    security: Option<Arc<SecurityPipeline>>,
    shutdown: tokio_util::sync::CancellationToken,
) -> JoinHandle<()> {
    let ctx = super::worker_pool::WorkerCtx {
        engine,
        lifecycle,
        registry,
        limits: resource_limits,
        security,
    };
    super::worker_pool::spawn(queue, ctx, shutdown)
}

pub(super) async fn execute_request(
    engine: &Arc<InferenceEngine>,
    lifecycle: Option<&ModelLifecycle>,
    registry: Option<&ModelRegistry>,
    resource_limits: Option<&ResourceLimits>,
    security: Option<&SecurityPipeline>,
    request: QueuedRequest,
) {
    let model_id = request.model_id.clone();
    let control = request.stop_control();

    let Some(request) = scan_ingress(security, request) else {
        return;
    };
    // Cancelled or expired while waiting for the worker: explicit failure, no work.
    if control.should_stop() {
        request.reject_unexecuted();
        return;
    }

    let _guard = match acquire_guard(engine, resource_limits, &model_id).await {
        Ok(g) => g,
        Err(msg) => {
            telemetry::record_admission_rejection(&model_id, &msg);
            send_response(request, Err(msg));
            return;
        }
    };
    let start = std::time::Instant::now();
    let max_memory = resource_limits.map(ResourceLimits::max_memory_per_call);
    let result = run_blocking(engine, &request, control, max_memory).await;
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
    match super::worker_ingress::scan_texts(security, &request.model_id, &[&request.prompt]) {
        Ok(()) => Some(request),
        Err(message) => {
            send_response(request, Err(message));
            None
        }
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

type InferResult = Result<InferenceResult, InferenceError>;

/// Run native inference on the blocking pool. Prefill and generation block
/// for seconds; on an async worker thread they could starve the tasks that
/// deliver `cancel_request`, disconnects and deadlines for this very request.
/// A panic in the backend becomes an explicit error instead of killing the loop.
async fn run_blocking(
    engine: &Arc<InferenceEngine>,
    request: &QueuedRequest,
    control: crate::engine::StopControl,
    max_memory: Option<usize>,
) -> InferResult {
    let engine = Arc::clone(engine);
    let model_id = request.model_id.clone();
    let prompt = request.prompt.clone();
    let params = request.params.clone();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(engine.run_controlled(&model_id, &prompt, &params, &control, max_memory))
    })
    .await
    .unwrap_or_else(|e| {
        Err(InferenceError::ExecutionFailed(format!(
            "inference task failed: {e}"
        )))
    })
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

fn send_response(request: QueuedRequest, result: Result<InferenceResult, String>) {
    if let Some(tx) = request.response_tx {
        let _ = tx.send(result);
    }
}

#[path = "worker_admission.rs"]
mod admission;
pub(super) use admission::acquire_guard;

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "worker_security_tests.rs"]
mod security_tests;

#[cfg(test)]
#[path = "worker_streaming_tests.rs"]
mod streaming_tests;

#[cfg(test)]
#[path = "worker_stop_tests.rs"]
mod stop_tests;

#[cfg(test)]
#[path = "worker_hol_tests.rs"]
mod hol_tests;
