//! Test-only model for the release-wait tests: holds its request until the
//! request is stopped, through the cancellable path the engine really uses.

use std::time::Duration;

use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig, InferenceError,
    InferenceInput, InferenceOutput, Model,
};

/// Holds itself until its request is stopped (overrides the cancellable path).
pub(super) struct Blocking;

#[async_trait::async_trait]
impl Model for Blocking {
    fn model_id(&self) -> &str {
        "blocking"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        4096
    }
    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "ok".into(),
            tokens_generated: 1,
            finish_reason: FinishReason::Stop,
        }))
    }
    async fn infer_cancellable(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
        is_stopped: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, InferenceError> {
        while !is_stopped.is_some_and(|stopped| stopped()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(InferenceError::ModelError("stopped".into()))
    }
    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
