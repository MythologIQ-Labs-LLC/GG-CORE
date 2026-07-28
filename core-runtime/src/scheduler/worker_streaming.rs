//! Streaming execution helpers for the worker loop.

use super::streaming_queue::StreamingQueuedRequest;
use crate::engine::InferenceEngine;
use crate::memory::ResourceLimits;
use crate::security::SecurityPipeline;
use crate::telemetry;

/// Execute a streaming inference request with resource control.
pub(crate) async fn execute(
    engine: &InferenceEngine,
    resource_limits: Option<&ResourceLimits>,
    security: Option<&SecurityPipeline>,
    request: StreamingQueuedRequest,
) {
    let model_id = request.model_id.clone();

    if !scan_ingress(security, &request).await {
        return;
    }

    let _guard = match super::worker::acquire_guard(engine, resource_limits, &model_id).await {
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

    let start = std::time::Instant::now();
    let result = run_stream(
        engine,
        &model_id,
        request.prompt,
        request.config,
        request.token_sender,
        security,
    )
    .await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(())) => telemetry::record_request_success(&model_id, latency_ms, 0),
        Ok(Err(e)) => telemetry::record_request_failure(&model_id, &e.to_string()),
        Err(e) => telemetry::record_request_failure(&model_id, &e.to_string()),
    }
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
async fn run_stream(
    engine: &InferenceEngine,
    model_id: &str,
    prompt: String,
    config: crate::engine::InferenceConfig,
    sender: crate::engine::TokenStreamSender,
    security: Option<&SecurityPipeline>,
) -> Result<Result<(), crate::engine::inference::InferenceError>, tokio::task::JoinError> {
    #[cfg(feature = "gguf")]
    {
        // Cast to usize to avoid capturing raw pointers (which are !Send).
        // SAFETY: the caller awaits the JoinHandle, so both the engine and the
        // security pipeline references outlive the spawned task.
        let engine_addr = engine as *const InferenceEngine as usize;
        let sec_addr = security.map(|s| s as *const SecurityPipeline as usize);
        let mid = model_id.to_string();
        tokio::task::spawn_blocking(move || {
            let engine = unsafe { &*(engine_addr as *const InferenceEngine) };
            let security = sec_addr.map(|a| unsafe { &*(a as *const SecurityPipeline) });
            engine.run_stream_sync(&mid, &prompt, &config, sender, security)
        })
        .await
    }
    #[cfg(not(feature = "gguf"))]
    {
        let _ = (engine, model_id, prompt, config, sender, security);
        Ok(Err(
            crate::engine::inference::InferenceError::ExecutionFailed(
                "streaming requires gguf feature".into(),
            ),
        ))
    }
}
