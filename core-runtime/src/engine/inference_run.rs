//! Controlled model inference and context guards, split from inference.rs.
//! Cancellation wins even if a backend completed partial output.

use std::sync::Arc;

use super::{InferenceEngine, InferenceError, InferenceParams, InferenceResult};
use crate::engine::{FinishReason, InferenceInput, InferenceOutput, Model, StopControl};

impl InferenceEngine {
    /// Conservative bytes-per-token estimate for context check.
    const BYTES_PER_TOKEN: usize = 4;

    pub(super) fn check_context(&self, prompt: &str) -> Result<(), InferenceError> {
        let estimated_tokens = prompt.len() / Self::BYTES_PER_TOKEN;
        if estimated_tokens > self.max_context_length {
            return Err(InferenceError::ContextExceeded {
                max: self.max_context_length,
                got: estimated_tokens,
            });
        }
        Ok(())
    }

    pub(super) async fn infer_controlled(
        model: &Arc<dyn Model>,
        prompt: &str,
        params: &InferenceParams,
        max_memory_bytes: Option<usize>,
        control: &StopControl,
    ) -> Result<InferenceResult, InferenceError> {
        // `max_memory_bytes` is the per-request working budget the worker
        // already reserved; resident weights are accounted at load. It is
        // passed to the backend, never compared against model weights.
        let mut config = params.to_config();
        config.max_memory_bytes = max_memory_bytes;

        let input = InferenceInput::Text(prompt.to_string());
        let check = || control.should_stop();
        let output = model.infer_cancellable(&input, &config, Some(&check)).await;
        // A stop observed at or after a checkpoint wins over whatever the
        // backend returned: partial output must never look like success.
        if let Some(stopped) = control.stop_error() {
            return Err(stopped);
        }
        let output = output.map_err(|e| InferenceError::ExecutionFailed(e.to_string()))?;

        match output {
            InferenceOutput::Generation(gen) if gen.finish_reason == FinishReason::Cancelled => {
                Err(InferenceError::Cancelled)
            }
            InferenceOutput::Generation(gen) if gen.finish_reason == FinishReason::Timeout => {
                Err(InferenceError::DeadlineExceeded {
                    timeout_ms: config.timeout_ms,
                })
            }
            InferenceOutput::Generation(gen) => Ok(InferenceResult {
                output: gen.text,
                tokens_generated: gen.tokens_generated as usize,
                finished: true,
            }),
            _ => Err(InferenceError::ExecutionFailed(
                "Model returned non-generation output".into(),
            )),
        }
    }}
