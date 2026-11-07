//! Core inference execution with real model delegation.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::engine::DegradedModePolicy;
#[cfg(feature = "gguf")]
use crate::engine::InferenceConfig;
use crate::engine::Model;
use crate::engine::{FinishReason, InferenceInput, InferenceOutput, StopControl};
use crate::models::ModelHandle;

pub use super::inference_types::{InferenceError, InferenceParams, InferenceResult};
// Adaptive speculative decode wiring (B-21c); child module for private-field access.
#[cfg(feature = "advanced")]
#[path = "inference_speculative.rs"]
mod speculative_wire;

/// Executes model inference by delegating to registered models.
pub struct InferenceEngine {
    max_context_length: usize,
    /// Models indexed by model_id for lookup.
    models: Arc<RwLock<HashMap<String, Arc<dyn Model>>>>,
    /// Degraded-mode policy applied under resource pressure (B-07).
    degraded: DegradedModePolicy,
    // Adaptive speculative decoding (B-21c); off by default, target->draft pairs.
    #[cfg(feature = "advanced")]
    spec_config: crate::models::speculative_config::AdaptiveSpeculativeConfig,
    #[cfg(feature = "advanced")]
    spec_telemetry: Arc<crate::engine::adaptive_speculative::telemetry::SpeculativeTelemetry>,
    #[cfg(feature = "advanced")]
    draft_pairs: Arc<RwLock<HashMap<String, String>>>,
}

impl InferenceEngine {
    pub fn new(max_context_length: usize) -> Self {
        Self::with_degraded_policy(max_context_length, DegradedModePolicy::default())
    }

    /// Construct with a custom degraded-mode policy (overrides the default).
    pub fn with_degraded_policy(max_context_length: usize, degraded: DegradedModePolicy) -> Self {
        Self {
            max_context_length,
            models: Arc::new(RwLock::new(HashMap::new())),
            degraded,
            #[cfg(feature = "advanced")]
            spec_config: crate::models::speculative_config::AdaptiveSpeculativeConfig::default(),
            #[cfg(feature = "advanced")]
            spec_telemetry: Arc::default(),
            #[cfg(feature = "advanced")]
            draft_pairs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a model for inference.
    pub async fn register_model(
        &self,
        model_id: String,
        _handle: ModelHandle,
        model: Arc<dyn Model>,
    ) {
        self.models.write().await.insert(model_id, model);
    }

    /// Unregister a model.
    pub async fn unregister_model(&self, model_id: &str) {
        self.models.write().await.remove(model_id);
    }

    /// Run inference on text prompt using the specified model.
    ///
    /// `params.timeout_ms` is a wall-clock deadline measured from this call;
    /// prompt prefill and generation stop at their next checkpoint once it
    /// passes and the call fails with `DeadlineExceeded`.
    pub(crate) async fn run(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        params.validate()?;
        let control = StopControl::from_timeout(None, params.timeout_ms);
        let model = self.get_model(model_id).await?;
        let prompt = self.apply_degraded_context(prompt)?;
        // B-21c: config-gated speculative decode (off by default); miss -> single-model.
        #[cfg(all(feature = "gguf", feature = "advanced"))]
        if let Some(r) = self
            .try_speculative(model_id, &model, &prompt, params)
            .await
        {
            return match control.stop_error() {
                Some(stopped) => Err(stopped),
                None => r,
            };
        }
        Self::infer_controlled(&model, &prompt, params, None, &control).await
    }

    /// Test convenience over `run_controlled`: a caller-owned cancel flag and
    /// a deadline from `params.timeout_ms`, measured from this call.
    #[cfg(test)]
    pub(crate) async fn run_cancellable(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
        is_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<InferenceResult, InferenceError> {
        let control = StopControl::from_timeout(Some(is_cancelled), params.timeout_ms);
        self.run_controlled(model_id, prompt, params, &control, None)
            .await
    }

    /// Test convenience: `run_cancellable` plus a per-call memory budget
    /// (`max_memory_bytes` enforced before calling into the model).
    #[cfg(test)]
    pub(crate) async fn run_cancellable_with_memory_limit(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
        is_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        max_memory_bytes: usize,
    ) -> Result<InferenceResult, InferenceError> {
        let control = StopControl::from_timeout(Some(is_cancelled), params.timeout_ms);
        self.run_controlled(model_id, prompt, params, &control, Some(max_memory_bytes))
            .await
    }

    /// Run inference under an explicit stop control (queue-owned cancel flag
    /// and deadline). A stopped request always returns `Cancelled` or
    /// `DeadlineExceeded`, never a partial success.
    pub(crate) async fn run_controlled(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
        control: &StopControl,
        max_memory_bytes: Option<usize>,
    ) -> Result<InferenceResult, InferenceError> {
        params.validate()?;
        if let Some(stopped) = control.stop_error() {
            return Err(stopped);
        }
        let model = self.get_model(model_id).await?;
        self.check_context(prompt)?;
        Self::infer_controlled(&model, prompt, params, max_memory_bytes, control).await
    }

    /// Look up a model by ID, cloning the Arc (drops the read lock).
    async fn get_model(&self, model_id: &str) -> Result<Arc<dyn Model>, InferenceError> {
        let models = self.models.read().await;
        models
            .get(model_id)
            .cloned()
            .ok_or_else(|| InferenceError::ModelNotLoaded(model_id.to_string()))
    }

    /// Conservative bytes-per-token estimate for context check.
    const BYTES_PER_TOKEN: usize = 4;

    fn check_context(&self, prompt: &str) -> Result<(), InferenceError> {
        let estimated_tokens = prompt.len() / Self::BYTES_PER_TOKEN;
        if estimated_tokens > self.max_context_length {
            return Err(InferenceError::ContextExceeded {
                max: self.max_context_length,
                got: estimated_tokens,
            });
        }
        Ok(())
    }

    async fn infer_controlled(
        model: &Arc<dyn Model>,
        prompt: &str,
        params: &InferenceParams,
        max_memory_bytes: Option<usize>,
        control: &StopControl,
    ) -> Result<InferenceResult, InferenceError> {
        if let Some(budget) = max_memory_bytes {
            let model_mem = model.memory_usage();
            if model_mem > budget {
                return Err(InferenceError::MemoryExceeded {
                    used: model_mem,
                    limit: budget,
                });
            }
        }

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
    }

    pub fn max_context_length(&self) -> usize {
        self.max_context_length
    }

    /// Check if a model is registered.
    pub async fn has_model(&self, model_id: &str) -> bool {
        self.models.read().await.contains_key(model_id)
    }

    /// Return the memory usage reported by a registered model, or None if not found.
    pub async fn model_memory_usage(&self, model_id: &str) -> Option<usize> {
        self.models
            .read()
            .await
            .get(model_id)
            .map(|m| m.memory_usage())
    }
}

#[cfg(feature = "gguf")]
#[path = "inference_streaming.rs"]
mod streaming;

#[path = "inference_degraded.rs"]
mod degraded_impl;

#[cfg(test)]
#[path = "inference_tests.rs"]
mod tests;

// Typed non-generative inference is crate-private until a capability-aware
// authenticated IPC queue and uniform security pipeline are qualified.
// No production caller until the typed IPC path (#171) is qualified; the
// allow is scoped to non-test builds and must be removed by that change.
#[cfg_attr(not(test), allow(dead_code))]
#[path = "inference_typed.rs"]
mod typed;

#[cfg(test)]
#[path = "inference_chaos_tests.rs"]
mod chaos_tests;
