//! Speculative decoding integration for GGUF models.
//!
//! Implements DraftModel and TargetModel traits for GgufGenerator,
//! enabling 2-3x speedup on CPU by predicting multiple tokens at once.

use std::sync::Arc;

use super::GgufGenerator;
use crate::engine::speculative_v2::{DraftModel, TargetModel, VerifyResult};
use crate::engine::InferenceError;

/// Wrapper for using GgufGenerator as a draft model.
pub struct GgufDraftModel {
    generator: Arc<GgufGenerator>,
}

impl GgufDraftModel {
    pub fn new(generator: Arc<GgufGenerator>) -> Self {
        Self { generator }
    }
}

#[async_trait::async_trait]
impl DraftModel for GgufDraftModel {
    async fn generate_draft(
        &self,
        context: &[u32],
        count: usize,
    ) -> Result<Vec<u32>, InferenceError> {
        self.generator.generate_tokens(context, count).await
    }

    /// Uniform placeholder: the GGUF generator exposes no per-token draft
    /// probabilities, so verification relies on `verify_tokens` (target-side
    /// comparison), not draft-probability weighting.
    fn get_probabilities(&self, _context: &[u32], tokens: &[u32]) -> Vec<f32> {
        vec![1.0; tokens.len()]
    }
}

/// Wrapper for using GgufGenerator as a target model.
pub struct GgufTargetModel {
    generator: Arc<GgufGenerator>,
}

impl GgufTargetModel {
    pub fn new(generator: Arc<GgufGenerator>) -> Self {
        Self { generator }
    }
}

#[async_trait::async_trait]
impl TargetModel for GgufTargetModel {
    async fn verify_tokens(
        &self,
        context: &[u32],
        draft: &[u32],
    ) -> Result<VerifyResult, InferenceError> {
        self.generator.verify_draft_tokens(context, draft).await
    }

    async fn generate_one(&self, context: &[u32]) -> Result<u32, InferenceError> {
        let tokens = self.generator.generate_tokens(context, 1).await?;
        tokens
            .into_iter()
            .next()
            .ok_or_else(|| InferenceError::ModelError("failed to generate token".into()))
    }

    fn eos_token(&self) -> Option<u32> {
        self.generator.eos_token_id()
    }

    /// Uniform placeholder: per-token target probabilities are not surfaced by the
    /// GGUF generator; `verify_tokens` performs the greedy target comparison.
    fn get_probabilities(&self, _context: &[u32], tokens: &[u32]) -> Vec<f32> {
        vec![1.0; tokens.len()]
    }
}
