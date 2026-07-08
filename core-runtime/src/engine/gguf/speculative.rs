//! Speculative decoding integration for GGUF models.
//!
//! Implements DraftModel and TargetModel traits for GgufGenerator,
//! enabling 2-3x speedup on CPU by predicting multiple tokens at once.

use std::sync::Arc;

use super::GgufGenerator;
use crate::engine::speculative::{DraftModel, TargetModel, VerifyResult};
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
}
