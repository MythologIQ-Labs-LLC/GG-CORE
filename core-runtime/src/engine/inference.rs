//! Core inference execution with real model delegation.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::engine::DegradedModePolicy;
#[cfg(feature = "gguf")]
use crate::engine::InferenceConfig;
use crate::engine::Model;
use crate::engine::{InferenceInput, InferenceOutput};
use crate::models::ModelHandle;

pub use super::inference_types::{InferenceError, InferenceParams, InferenceResult};

/// Executes model inference by delegating to registered models.
pub struct InferenceEngine {
    max_context_length: usize,
    /// Models indexed by model_id for lookup.
    models: Arc<RwLock<HashMap<String, Arc<dyn Model>>>>,
    /// Degraded-mode policy applied under resource pressure (B-07).
    degraded: DegradedModePolicy,
}

impl InferenceEngine {
    pub fn new(max_context_length: usize) -> Self {
        Self {
            max_context_length,
            models: Arc::new(RwLock::new(HashMap::new())),
            degraded: DegradedModePolicy::default(),
        }
    }

    /// Construct with a custom degraded-mode policy (overrides the default).
    pub fn with_degraded_policy(max_context_length: usize, degraded: DegradedModePolicy) -> Self {
        Self {
            max_context_length,
            models: Arc::new(RwLock::new(HashMap::new())),
            degraded,
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
    pub async fn run(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        params.validate()?;
        let model = self.get_model(model_id).await?;
        let prompt = self.apply_degraded_context(prompt)?;
        Self::infer_with_model(&model, &prompt, params).await
    }

    /// Run inference with cooperative per-token cancellation.
    ///
    /// The cancellation flag is checked before inference and also
    /// threaded through to the GGUF backend for per-token checks.
    pub async fn run_cancellable(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
        is_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<InferenceResult, InferenceError> {
        use std::sync::atomic::Ordering;

        params.validate()?;

        if is_cancelled.load(Ordering::Acquire) {
            return Err(InferenceError::ExecutionFailed("cancelled".into()));
        }

        let model = self.get_model(model_id).await?;
        self.check_context(prompt)?;

        let cancel = Arc::clone(&is_cancelled);
        let check = move || cancel.load(Ordering::Acquire);
        let result = Self::infer_cancellable(&model, prompt, params, None, Some(&check)).await?;

        Ok(result)
    }

    /// Run inference with per-token cancellation and a per-call memory budget.
    ///
    /// The `max_memory_bytes` is enforced before calling into the model.
    pub async fn run_cancellable_with_memory_limit(
        &self,
        model_id: &str,
        prompt: &str,
        params: &InferenceParams,
        is_cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        max_memory_bytes: usize,
    ) -> Result<InferenceResult, InferenceError> {
        use std::sync::atomic::Ordering;

        params.validate()?;

        if is_cancelled.load(Ordering::Acquire) {
            return Err(InferenceError::ExecutionFailed("cancelled".into()));
        }

        let model = self.get_model(model_id).await?;
        self.check_context(prompt)?;

        let cancel = Arc::clone(&is_cancelled);
        let check = move || cancel.load(Ordering::Acquire);
        let result =
            Self::infer_cancellable(&model, prompt, params, Some(max_memory_bytes), Some(&check))
                .await?;

        Ok(result)
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

    async fn infer_with_model(
        model: &Arc<dyn Model>,
        prompt: &str,
        params: &InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        Self::infer_cancellable(model, prompt, params, None, None).await
    }

    async fn infer_cancellable(
        model: &Arc<dyn Model>,
        prompt: &str,
        params: &InferenceParams,
        max_memory_bytes: Option<usize>,
        is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
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
        let output = model
            .infer_cancellable(&input, &config, is_cancelled)
            .await
            .map_err(|e| InferenceError::ExecutionFailed(e.to_string()))?;

        match output {
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
