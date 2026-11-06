//! Worker execution for typed (embedding/classification) requests.
//!
//! Same order and controls as generation: ingress scan of every input text,
//! cancel/deadline check, resource admission, panic-safe blocking execution
//! under the request's own stop control, egress filtering of label text,
//! and content-free telemetry. There is no ONNX- or task-specific bypass.

use std::sync::Arc;

use super::queue::TypedQueuedRequest;
use crate::engine::inference::InferenceError;
use crate::engine::{InferenceEngine, InferenceInput, InferenceOutput};
use crate::memory::ResourceLimits;
use crate::security::SecurityPipeline;
use crate::telemetry;

type TypedResult = Result<InferenceOutput, InferenceError>;

pub(crate) async fn execute(
    engine: &Arc<InferenceEngine>,
    resource_limits: Option<&ResourceLimits>,
    security: Option<&SecurityPipeline>,
    request: TypedQueuedRequest,
) {
    let model_id = request.model_id.clone();
    if let Err(message) =
        super::worker_ingress::scan_texts(security, &model_id, &texts(&request.input))
    {
        request.respond(Err(message));
        return;
    }
    if request.stop_control().should_stop() {
        request.reject_unexecuted();
        return;
    }
    let _guard = match super::worker::acquire_guard(engine, resource_limits, &model_id).await {
        Ok(guard) => guard,
        Err(message) => {
            telemetry::record_admission_rejection(&model_id, &message);
            request.respond(Err(message));
            return;
        }
    };
    let start = std::time::Instant::now();
    let max_memory = resource_limits.map(ResourceLimits::max_memory_per_call);
    let result = run_blocking(engine, &request, max_memory).await;
    let result = apply_egress(security, &model_id, result);
    match &result {
        Ok(_) => {
            telemetry::record_request_success(&model_id, start.elapsed().as_millis() as u64, 0)
        }
        Err(e) => telemetry::record_request_failure(&model_id, &e.to_string()),
    }
    request.respond(result.map_err(|e| e.to_string()));
}

fn texts(input: &InferenceInput) -> Vec<&str> {
    match input {
        InferenceInput::Text(text) => vec![text.as_str()],
        InferenceInput::TextBatch(batch) => batch.iter().map(String::as_str).collect(),
        InferenceInput::ChatMessages(messages) => {
            messages.iter().map(|m| m.content.as_str()).collect()
        }
    }
}

/// Execute on the blocking pool so CPU-bound backends never stall the async
/// workers; a backend panic becomes an explicit error, not a dead worker.
async fn run_blocking(
    engine: &Arc<InferenceEngine>,
    request: &TypedQueuedRequest,
    max_memory: Option<usize>,
) -> TypedResult {
    let engine = Arc::clone(engine);
    let model_id = request.model_id.clone();
    let input = request.input.clone();
    let capability = request.capability;
    let params = request.params.clone();
    let control = request.stop_control();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(
            engine
                .run_typed_controlled(&model_id, &input, capability, &params, &control, max_memory),
        )
    })
    .await
    .unwrap_or_else(|e| {
        Err(InferenceError::ExecutionFailed(format!(
            "inference task failed: {e}"
        )))
    })
}

/// Classification labels are model-produced text: pass them through the
/// same egress sanitizer as generated text. Vectors are numeric and were
/// already checked for finiteness and dimensions by the engine.
fn apply_egress(
    security: Option<&SecurityPipeline>,
    model_id: &str,
    result: TypedResult,
) -> TypedResult {
    let Some(sec) = security else {
        return result;
    };
    result.map(|output| match output {
        InferenceOutput::Classification(mut c) => {
            let mut redactions = 0u64;
            let mut clean = |label: &mut String| {
                let s = sec.sanitize_output(label);
                redactions += s.redactions as u64;
                *label = s.output;
            };
            clean(&mut c.label);
            for (label, _) in &mut c.all_labels {
                clean(label);
            }
            telemetry::record_output_sanitize(model_id, 0, redactions);
            InferenceOutput::Classification(c)
        }
        other => other,
    })
}
