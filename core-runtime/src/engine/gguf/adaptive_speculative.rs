//! Adaptive-speculative GGUF adapter (B-21c).
//!
//! Implements the adaptive-speculative model-side traits over a `GgufGenerator`,
//! so the [`AdaptiveSpeculativeExecutor`] can drive a GGUF draft/target pair via
//! the block-level `BlockDraftModel`/`TargetVerifier`.
//!
//! Degradation: the GGUF backend surfaces no per-token draft log-probs, so
//! [`DraftBlock::from_tokens`] fills them with `-inf` and the heuristic estimator
//! leans on temperature/repetition/history signal (see ADR-007 §Implementation
//! Status). A real speedup additionally needs KV-cache reuse across steps (B-21f).
//!
//! [`AdaptiveSpeculativeExecutor`]: crate::engine::adaptive_speculative::executor::AdaptiveSpeculativeExecutor

use async_trait::async_trait;

use super::GgufGenerator;
use crate::engine::adaptive_speculative::{
    BlockDraftModel, DraftBlock, TargetVerifier, VerificationPlan, VerificationResult,
};
use crate::engine::InferenceError;

/// Draft-model adapter: generates a block of candidate tokens from a GGUF model.
/// Borrows the generator (obtained via `Model::as_any` downcast at the call site).
pub struct GgufBlockDraftModel<'a> {
    generator: &'a GgufGenerator,
}

impl<'a> GgufBlockDraftModel<'a> {
    pub fn new(generator: &'a GgufGenerator) -> Self {
        Self { generator }
    }
}

#[async_trait]
impl BlockDraftModel for GgufBlockDraftModel<'_> {
    async fn draft(&self, context: &[u32], max: usize) -> Result<DraftBlock, InferenceError> {
        let tokens = self.generator.generate_tokens(context, max).await?;
        // No per-token log-probs from the backend -> confidence runs degraded (B-21f).
        Ok(DraftBlock::from_tokens(tokens))
    }
}

/// Target-verifier adapter: greedily verifies draft tokens against a GGUF model.
pub struct GgufTargetVerifier<'a> {
    generator: &'a GgufGenerator,
}

impl<'a> GgufTargetVerifier<'a> {
    pub fn new(generator: &'a GgufGenerator) -> Self {
        Self { generator }
    }
}

#[async_trait]
impl TargetVerifier for GgufTargetVerifier<'_> {
    async fn verify(
        &self,
        context: &[u32],
        draft: &DraftBlock,
        _plan: &VerificationPlan,
    ) -> Result<VerificationResult, InferenceError> {
        // The GGUF backend verifies the whole draft greedily (first-divergence);
        // the plan window is advisory and not enforced here.
        let vr = self
            .generator
            .verify_draft_tokens(context, &draft.tokens)
            .await?;
        Ok(match vr.correction_token {
            Some(correction) => VerificationResult::reject_at(vr.accepted_count, correction),
            None => VerificationResult::accept_all(vr.accepted_count),
        })
    }

    async fn generate_one(&self, context: &[u32]) -> Result<u32, InferenceError> {
        let tokens = self.generator.generate_tokens(context, 1).await?;
        tokens
            .into_iter()
            .next()
            .ok_or_else(|| InferenceError::ModelError("target generated no token".into()))
    }

    fn eos_token(&self) -> Option<u32> {
        self.generator.eos_token_id()
    }
}
