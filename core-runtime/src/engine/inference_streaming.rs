//! GGUF streaming inference for `InferenceEngine`.
//!
//! Extracted from `inference.rs` (Section 4 Razor) as a child module so it
//! retains access to the engine's private registry. Behaviorally identical to
//! the pre-extraction methods; only the streaming path (which downcasts to the
//! concrete `GgufGenerator`) lives here.

use super::{InferenceConfig, InferenceEngine, InferenceError};

impl InferenceEngine {
    /// Run streaming inference, sending tokens to the provided sender.
    ///
    /// Looks up the model, downcasts to `GgufGenerator`, and calls
    /// `generate_stream()`. Designed for use with `spawn_blocking`.
    pub(crate) fn run_stream_sync(
        &self,
        model_id: &str,
        prompt: &str,
        config: &InferenceConfig,
        sender: crate::engine::TokenStreamSender,
        security: Option<&crate::security::SecurityPipeline>,
        should_stop: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<(), InferenceError> {
        // Emit exactly one terminal frame for every outcome — including the
        // lookup/downcast failures below — so a client can always distinguish a
        // completed stream from an errored one (B-24a).
        let result = self.stream_tokens(model_id, prompt, config, &sender, security, should_stop);
        // A cooperative cancellation/timeout must never be reported as a
        // successful complete generation, even if the backend returned Ok.
        let result = result.and_then(|()| stop_or_send_failure(&sender, should_stop));
        let terminal = match &result {
            Ok(()) => crate::engine::StreamTerminal::Complete,
            Err(e) => crate::engine::StreamTerminal::Error(e.to_string()),
        };
        let _ = tokio::runtime::Handle::current().block_on(sender.end(terminal));
        result
    }

    /// Stream frames for a prompt (no terminal — the caller owns that). When
    /// `security` is present the output is detokenized + egress-sanitized
    /// in-runtime and emitted as sanitized text frames (B-24b); otherwise raw
    /// token frames.
    fn stream_tokens(
        &self,
        model_id: &str,
        prompt: &str,
        config: &InferenceConfig,
        sender: &crate::engine::TokenStreamSender,
        security: Option<&crate::security::SecurityPipeline>,
        should_stop: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<(), InferenceError> {
        use crate::engine::gguf::GgufGenerator;
        use crate::security::stream_sanitizer::StreamSanitizer;

        // Clone Arc and drop read lock before calling into model.
        let rt = tokio::runtime::Handle::current();
        let model = {
            let models = rt.block_on(self.models.read());
            models
                .get(model_id)
                .cloned()
                .ok_or_else(|| InferenceError::ModelNotLoaded(model_id.to_string()))?
        };

        let generator = model
            .as_any()
            .downcast_ref::<GgufGenerator>()
            .ok_or_else(|| {
                InferenceError::ExecutionFailed("model does not support streaming".into())
            })?;

        let mut sanitizer = security.map(StreamSanitizer::new);
        generator
            .generate_stream(prompt, config, sender, should_stop, sanitizer.as_mut())
            .map_err(|e| InferenceError::ExecutionFailed(e.to_string()))
    }
}

/// A cooperative cancellation/timeout, or a frame the consumer never
/// received (stalled reader, deadline, gone), must never be reported as a
/// successful complete generation, even if the backend returned `Ok`.
fn stop_or_send_failure(
    sender: &crate::engine::TokenStreamSender,
    should_stop: Option<&(dyn Fn() -> bool + Send + Sync)>,
) -> Result<(), InferenceError> {
    if should_stop.is_some_and(|check| check()) {
        return Err(InferenceError::ExecutionFailed(
            "stream cancelled or deadline exceeded".into(),
        ));
    }
    match sender.failure() {
        None => Ok(()),
        Some(failure) => Err(InferenceError::ExecutionFailed(failure.to_string())),
    }
}
