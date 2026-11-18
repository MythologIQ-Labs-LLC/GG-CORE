//! Streaming execution helpers for the worker loop.
#![forbid(unsafe_code)]

use std::sync::Arc;

use super::streaming_queue::StreamingQueuedRequest;
use crate::engine::InferenceEngine;
use crate::memory::{ResourceGuard, ResourceLimits};
use crate::security::SecurityPipeline;
use crate::telemetry;

/// Native execution owners travel together; neither security nor admission
/// may be borrowed from an async frame that can be dropped during inference.
struct StreamOwners {
    security: Option<Arc<SecurityPipeline>>,
    guard: Option<ResourceGuard>,
}

/// One control bundle keeps cancellation and deadline tied to the same request
/// without growing the worker's already-wide execution signature.
struct StreamControl {
    cancel: tokio_util::sync::CancellationToken,
    deadline: Option<std::time::Instant>,
    /// Streaming queue id, for the #184 stop-trace span (gguf only).
    #[cfg_attr(not(feature = "gguf"), allow(dead_code))]
    queue_id: u64,
}

impl StreamControl {
    fn of(request: &StreamingQueuedRequest) -> Self {
        Self {
            cancel: request.cancelled.clone(),
            deadline: request.deadline,
            queue_id: request.id,
        }
    }

    fn should_stop(&self) -> bool {
        self.cancel.is_cancelled()
            || self
                .deadline
                .is_some_and(|d| std::time::Instant::now() >= d)
    }
}

/// Execute a streaming inference request with resource control.
pub(crate) async fn execute(
    engine: &Arc<InferenceEngine>,
    resource_limits: Option<&ResourceLimits>,
    security: Option<&Arc<SecurityPipeline>>,
    request: StreamingQueuedRequest,
) {
    let model_id = request.model_id.clone();

    if !scan_ingress(security.map(|s| &**s), &request).await {
        return;
    }
    if request.is_cancelled() || request.is_expired() {
        let reason = if request.is_cancelled() {
            "stream cancelled before execution"
        } else {
            "stream deadline exceeded before execution"
        };
        let _ = request
            .token_sender
            .end(crate::engine::StreamTerminal::Error(reason.into()))
            .await;
        return;
    }

    let guard = match super::worker::acquire_guard(engine, resource_limits, &model_id).await {
        Ok(g) => g,
        Err(msg) => {
            telemetry::record_admission_rejection(&model_id, &msg);
            let _ = request
                .token_sender
                .end(crate::engine::StreamTerminal::Error(msg))
                .await;
            return;
        }
    };

    let control = StreamControl::of(&request);
    let start = std::time::Instant::now();
    let result = run_stream(
        engine,
        &model_id,
        request.prompt,
        request.config,
        request.token_sender,
        StreamOwners {
            security: security.cloned(),
            guard,
        },
        control,
    )
    .await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(())) => telemetry::record_request_success(&model_id, latency_ms, 0),
        Ok(Err(e)) => telemetry::record_request_failure(&model_id, &e.to_string()),
        Err(e) => telemetry::record_request_failure(&model_id, &e.to_string()),
    }
}

/// Run the backend inside the request's #184 stop-trace span and report S4
/// the moment it returns, so a deadline passing later never marks a stream
/// that had already finished.
#[cfg(feature = "gguf")]
fn traced<T>(
    control: &StreamControl,
    run: impl FnOnce(&(dyn Fn() -> bool + Send + Sync)) -> T,
) -> T {
    let span = super::worker::stop_scope(control.queue_id, true);
    let _entered = span.enter();
    let start = std::time::Instant::now();
    let should_stop = || control.should_stop();
    let out = run(&should_stop);
    trace_stream_release(control, start.elapsed().as_millis() as u64);
    out
}

/// #184 stop trace S4 for a stream. A stopped stream can return `Ok(())`,
/// so the stop is decided by the control, not by the result.
#[cfg_attr(not(feature = "gguf"), allow(dead_code))]
fn trace_stream_release(control: &StreamControl, run_ms: u64) {
    if !control.should_stop() {
        return;
    }
    tracing::info!(
        target: "gg_core::stop",
        queue_id = control.queue_id,
        cancel_flag = control.cancel.is_cancelled(),
        run_ms,
        stream = true,
        dropped = crate::telemetry::stop_trace::dropped(),
        "worker released after stop"
    );
}

/// Ingress scan for the streaming path. Returns `true` when admitted;
/// on rejection sends the final error frame and returns `false`.
async fn scan_ingress(
    security: Option<&SecurityPipeline>,
    request: &StreamingQueuedRequest,
) -> bool {
    let Some(sec) = security else {
        return true;
    };
    let verdict = sec.scan_prompt(&request.prompt);
    telemetry::record_security_scan(
        &request.model_id,
        verdict.latency_us,
        verdict.risk_score,
        !verdict.allowed,
    );
    if verdict.allowed {
        return true;
    }
    let _ = request
        .token_sender
        .end(crate::engine::StreamTerminal::Rejected(
            "prompt rejected by ingress security scan".into(),
        ))
        .await;
    false
}

/// Run streaming inference on a blocking thread.
///
/// The blocking task owns `Arc` clones of the engine and security pipeline. A
/// `spawn_blocking` task is not cancelled when this future is dropped (worker
/// shutdown, an aborted caller, or a runtime torn down mid-stream), so it must
/// never borrow from the caller: a borrowed pipeline was freed under a running
/// stream and faulted in the terminal sanitizer flush (#137 dump, 0xc0000005).
async fn run_stream(
    engine: &Arc<InferenceEngine>,
    model_id: &str,
    prompt: String,
    config: crate::engine::InferenceConfig,
    sender: crate::engine::TokenStreamSender,
    owners: StreamOwners,
    control: StreamControl,
) -> Result<Result<(), crate::engine::inference::InferenceError>, tokio::task::JoinError> {
    #[cfg(feature = "gguf")]
    {
        let engine = Arc::clone(engine);
        let StreamOwners { security, guard } = owners;
        let mid = model_id.to_string();
        super::worker::run_blocking_admitted(guard, move || {
            traced(&control, |stop| {
                let sec = security.as_deref();
                engine.run_stream_sync(&mid, &prompt, &config, sender, sec, Some(stop))
            })
        })
        .await
    }
    #[cfg(not(feature = "gguf"))]
    {
        // No native work is launched without GGUF, so release admitted bytes
        // here; reading the bundle also keeps both feature sets lint-clean.
        let StreamOwners { security, guard } = owners;
        let _ = (
            engine,
            model_id,
            prompt,
            config,
            security,
            guard,
            control.should_stop(),
        );
        let err = crate::engine::inference::InferenceError::ExecutionFailed(
            "streaming requires gguf feature".into(),
        );
        // Always terminate the stream: a dropped sender leaves the IPC
        // relay (and the client) without a final chunk.
        let _ = sender
            .end(crate::engine::StreamTerminal::Error(err.to_string()))
            .await;
        Ok(Err(err))
    }
}

#[cfg(test)]
#[path = "worker_streaming_trace_tests.rs"]
mod trace_tests;

#[cfg(all(test, feature = "gguf"))]
#[path = "worker_streaming_lifetime_tests.rs"]
mod lifetime_tests;
