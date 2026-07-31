//! Adaptive speculative decode wiring for `InferenceEngine` (B-21c).
//!
//! Child module of `engine::inference` so it can reach `InferenceEngine`'s private
//! speculative fields; relocated here to keep `inference.rs` under the Section-4
//! Razor file limit. The speculative path is entered only from `run` (which
//! `Runtime::infer` wraps with prompt-injection scan + PII sanitize), is off by
//! default, and falls through to single-model on any miss. Rejected draft suffixes
//! are never committed (enforced by the executor).

#[cfg(all(feature = "gguf", feature = "advanced"))]
use std::sync::Arc;

use super::{InferenceError, InferenceParams, InferenceResult};
use crate::engine::InferenceEngine;
#[cfg(all(feature = "gguf", feature = "advanced"))]
use crate::engine::Model;

impl InferenceEngine {
    /// Enable/configure adaptive speculative decoding (B-21c). Off by default; the
    /// speculative path runs only when the config is active AND a draft pair is
    /// registered — otherwise inference falls through to single-model.
    pub fn set_speculative_config(
        &mut self,
        config: crate::models::speculative_config::AdaptiveSpeculativeConfig,
    ) {
        self.spec_config = config;
    }

    /// Register a target -> draft model pair for speculative decoding (B-21c).
    /// Both ids must be registered models; the draft is the smaller model.
    pub async fn register_draft_pair(&self, target_id: String, draft_id: String) {
        self.draft_pairs.write().await.insert(target_id, draft_id);
    }

    /// Snapshot of live speculative telemetry (for `status`).
    pub fn speculative_snapshot(
        &self,
    ) -> crate::engine::adaptive_speculative::telemetry::SpeculativeSessionStats {
        self.spec_telemetry.snapshot()
    }

    /// Attempt an adaptive speculative decode. Returns `None` (fall through to
    /// single-model) unless speculation is active, a draft pair is registered, and
    /// both models downcast to GGUF generators.
    #[cfg(all(feature = "gguf", feature = "advanced"))]
    pub(super) async fn try_speculative(
        &self,
        target_id: &str,
        target_model: &Arc<dyn Model>,
        prompt: &str,
        params: &InferenceParams,
    ) -> Option<Result<InferenceResult, InferenceError>> {
        use crate::engine::gguf::GgufGenerator;

        if !self.spec_config.is_active() {
            return None;
        }
        let draft_id = self.draft_pairs.read().await.get(target_id).cloned()?;
        let draft_model = self.get_model(&draft_id).await.ok()?;
        let target_gen = target_model.as_any().downcast_ref::<GgufGenerator>()?;
        let draft_gen = draft_model.as_any().downcast_ref::<GgufGenerator>()?;
        Some(
            self.run_speculative(target_gen, draft_gen, prompt, params)
                .await,
        )
    }

    /// The speculative decode itself, once the GGUF pair is resolved (B-21c).
    #[cfg(all(feature = "gguf", feature = "advanced"))]
    async fn run_speculative(
        &self,
        target_gen: &crate::engine::gguf::GgufGenerator,
        draft_gen: &crate::engine::gguf::GgufGenerator,
        prompt: &str,
        params: &InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        use crate::engine::adaptive_speculative::executor::AdaptiveSpeculativeExecutor;
        use crate::engine::adaptive_speculative::heuristic::{
            AdaptiveVerificationScheduler, HeuristicConfidenceEstimator,
        };
        use crate::engine::gguf::{GgufBlockDraftModel, GgufTargetVerifier};

        // The executor + GGUF adapters use `engine::error::InferenceError`; map it
        // to the `inference_types::InferenceError` this façade returns.
        let cfg = params.to_config();
        let prompt_tokens = target_gen
            .tokenize(prompt)
            .map_err(|e| InferenceError::ExecutionFailed(e.to_string()))?;
        let drafter = GgufBlockDraftModel::new(draft_gen);
        let verifier = GgufTargetVerifier::new(target_gen);
        let estimator = HeuristicConfidenceEstimator::new(cfg.temperature, cfg.repetition_penalty);
        let scheduler = AdaptiveVerificationScheduler::new(self.spec_config.clone());
        let executor = AdaptiveSpeculativeExecutor::new(
            &drafter,
            &verifier,
            &estimator,
            &scheduler,
            self.spec_telemetry.as_ref(),
            self.spec_config.max_draft_tokens,
        );
        let out_tokens = executor
            .run(&prompt_tokens, params.max_tokens)
            .await
            .map_err(|e| InferenceError::ExecutionFailed(e.to_string()))?;
        let tokens_generated = out_tokens.len();
        let text = target_gen
            .detokenize(&out_tokens)
            .map_err(|e| InferenceError::ExecutionFailed(e.to_string()))?;
        Ok(InferenceResult {
            output: text,
            tokens_generated,
            finished: true,
        })
    }
}
