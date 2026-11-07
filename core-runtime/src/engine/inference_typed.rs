//! Backend-neutral typed inference core for a future capability-aware IPC path.
//!
//! This is intentionally crate-private. Public callers MUST go through the
//! authenticated IPC queue, ingress/egress policy, resource admission, and
//! request lifecycle. No new public transport is authorized by this module.

use super::{InferenceEngine, InferenceError, InferenceParams};
use crate::engine::{InferenceCapability, InferenceInput, InferenceOutput, StopControl};

const MAX_EMBEDDING_DIM: usize = 16_384;
const MAX_CLASS_LABELS: usize = 2_048;
const MAX_CLASS_LABEL_BYTES: usize = 256;

impl InferenceEngine {
    /// Execute a typed model task after the external worker has granted
    /// admission and applied ingress policy. Not a public IPC/API entrypoint.
    ///
    /// Unlike the legacy text-generation path, successful embedding and
    /// classification outputs remain typed, not encoded as generated text.
    pub(crate) async fn run_typed_controlled(
        &self,
        model_id: &str,
        input: &InferenceInput,
        capability: InferenceCapability,
        params: &InferenceParams,
        control: &StopControl,
        max_memory_bytes: Option<usize>,
    ) -> Result<InferenceOutput, InferenceError> {
        params.validate()?;
        input
            .validate()
            .map_err(|e| InferenceError::InvalidParams(e.to_string()))?;
        validate_task_input(capability, input)?;
        if let Some(stopped) = control.stop_error() {
            return Err(stopped);
        }

        let model = self.get_model(model_id).await?;
        if !model.capabilities().contains(&capability) {
            return Err(InferenceError::InvalidParams(
                "requested task is not supported by selected model".into(),
            ));
        }
        // `max_memory_bytes` is the per-request working budget reserved by
        // the worker; resident weights are accounted once at model load and
        // are never compared against it (that refused every model larger
        // than the budget). It is passed to the backend below.
        // Context validation is per input item. Never silently truncate
        // typed embedding or classification input based on degraded policy.
        match input {
            InferenceInput::Text(text) => self.check_context(text)?,
            InferenceInput::TextBatch(batch) => {
                for text in batch {
                    self.check_context(text)?;
                }
            }
            // Already refused by validate_task_input; fail closed, never panic.
            InferenceInput::ChatMessages(_) => {
                return Err(InferenceError::InvalidParams(
                    "typed chat input is not supported".into(),
                ))
            }
        }

        let mut config = params.to_config();
        config.max_memory_bytes = max_memory_bytes;
        let check = || control.should_stop();
        let result = model
            .infer_cancellable(input, &config, Some(&check))
            .await
            .map_err(|e| InferenceError::ExecutionFailed(e.to_string()));

        // Cancellation/deadline wins even when the backend returns partial
        // results. It cannot produce a success-shaped late response.
        if let Some(stopped) = control.stop_error() {
            return Err(stopped);
        }

        let output = result?;
        validate_typed_output(capability, input, &output)?;
        Ok(output)
    }
}

fn validate_task_input(
    capability: InferenceCapability,
    input: &InferenceInput,
) -> Result<(), InferenceError> {
    match (capability, input) {
        (InferenceCapability::Embedding, InferenceInput::Text(_))
        | (InferenceCapability::Embedding, InferenceInput::TextBatch(_))
        | (InferenceCapability::TextClassification, InferenceInput::Text(_)) => Ok(()),
        // The current classifier returns one classification for TextBatch,
        // rather than a result per input. Refuse ambiguous batch semantics.
        (InferenceCapability::TextClassification, InferenceInput::TextBatch(_)) => {
            Err(InferenceError::InvalidParams(
                "classification batch results are not yet supported".into(),
            ))
        }
        _ => Err(InferenceError::InvalidParams(
            "task/input shape is not implemented for typed inference".into(),
        )),
    }
}

fn validate_embedding(vector: &[f32], dimensions: usize) -> Result<(), InferenceError> {
    if dimensions == 0
        || dimensions > MAX_EMBEDDING_DIM
        || dimensions != vector.len()
        || !vector.iter().all(|value| value.is_finite())
    {
        return Err(InferenceError::ExecutionFailed(
            "embedding result has invalid dimensions or non-finite values".into(),
        ));
    }
    Ok(())
}

fn validate_typed_output(
    capability: InferenceCapability,
    input: &InferenceInput,
    result: &InferenceOutput,
) -> Result<(), InferenceError> {
    match (capability, input, result) {
        (
            InferenceCapability::Embedding,
            InferenceInput::Text(_),
            InferenceOutput::Embedding(embedding),
        ) => validate_embedding(&embedding.vector, embedding.dimensions),
        (
            InferenceCapability::Embedding,
            InferenceInput::TextBatch(inputs),
            InferenceOutput::EmbeddingBatch(batch),
        ) => {
            if inputs.len() != batch.len() || batch.is_empty() {
                return Err(InferenceError::ExecutionFailed(
                    "embedding batch result count does not match request".into(),
                ));
            }
            for item in batch {
                validate_embedding(&item.vector, item.dimensions)?;
            }
            Ok(())
        }
        (
            InferenceCapability::TextClassification,
            InferenceInput::Text(_),
            InferenceOutput::Classification(classification),
        ) => {
            if classification.label.is_empty()
                || classification.label.len() > MAX_CLASS_LABEL_BYTES
                || !classification.confidence.is_finite()
                || !(0.0..=1.0).contains(&classification.confidence)
                || classification.all_labels.is_empty()
                || classification.all_labels.len() > MAX_CLASS_LABELS
                || classification.all_labels.iter().any(|(label, confidence)| {
                    label.is_empty()
                        || label.len() > MAX_CLASS_LABEL_BYTES
                        || !confidence.is_finite()
                        || !(0.0..=1.0).contains(confidence)
                })
            {
                return Err(InferenceError::ExecutionFailed(
                    "classification result contains invalid labels or confidence".into(),
                ));
            }
            Ok(())
        }
        _ => Err(InferenceError::ExecutionFailed(
            "model output type does not match requested capability".into(),
        )),
    }
}

#[cfg(test)]
#[path = "inference_typed_tests.rs"]
mod tests;
