//! Speculative decoding for accelerated text generation.
//!
//! Implements draft-verify loop where a smaller draft model generates candidate
//! tokens that are verified by the target model in parallel.

use crate::engine::InferenceError;

/// Configuration for speculative decoding.
#[derive(Debug, Clone)]
pub struct SpeculativeConfig {
    /// Number of draft tokens to generate before verification.
    pub draft_tokens: usize,
    /// Acceptance threshold (0.0 to 1.0).
    pub acceptance_threshold: f32,
    /// Enable speculative decoding.
    pub enabled: bool,
}

impl Default for SpeculativeConfig {
    fn default() -> Self {
        Self {
            draft_tokens: 4,
            acceptance_threshold: 0.9,
            enabled: true,
        }
    }
}

/// Result from the verification phase.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// Number of draft tokens accepted.
    pub accepted_count: usize,
    /// Correction token if verification diverged.
    pub correction_token: Option<u32>,
}

impl VerifyResult {
    /// Create a result where all tokens are accepted.
    pub fn accept_all(count: usize) -> Self {
        Self {
            accepted_count: count,
            correction_token: None,
        }
    }

    /// Create a result where verification diverged.
    pub fn diverge_at(accepted: usize, correction: u32) -> Self {
        Self {
            accepted_count: accepted,
            correction_token: Some(correction),
        }
    }
}

/// Draft model trait for generating candidate tokens.
#[async_trait::async_trait]
pub trait DraftModel: Send + Sync {
    /// Generate draft tokens from context.
    async fn generate_draft(
        &self,
        context: &[u32],
        count: usize,
    ) -> Result<Vec<u32>, InferenceError>;
}

/// Target model trait for verification.
#[async_trait::async_trait]
pub trait TargetModel: Send + Sync {
    /// Verify draft tokens against target model.
    async fn verify_tokens(
        &self,
        context: &[u32],
        draft: &[u32],
    ) -> Result<VerifyResult, InferenceError>;

    /// Generate a single token (fallback when draft rejected).
    async fn generate_one(&self, context: &[u32]) -> Result<u32, InferenceError>;

    /// Get end-of-sequence token.
    fn eos_token(&self) -> Option<u32>;
}

/// Speculative decoding executor.
pub struct SpeculativeDecoder<D, T> {
    draft_model: D,
    target_model: T,
    config: SpeculativeConfig,
}

impl<D, T> SpeculativeDecoder<D, T>
where
    D: DraftModel,
    T: TargetModel,
{
    /// Create a new speculative decoder.
    pub fn new(draft_model: D, target_model: T, config: SpeculativeConfig) -> Self {
        Self {
            draft_model,
            target_model,
            config,
        }
    }

    /// Generate tokens using speculative decoding.
    pub async fn generate(
        &self,
        prompt_tokens: &[u32],
        max_tokens: u32,
    ) -> Result<Vec<u32>, InferenceError> {
        if !self.config.enabled {
            return self.generate_standard(prompt_tokens, max_tokens).await;
        }

        let mut output = Vec::with_capacity(max_tokens as usize);
        let mut context = prompt_tokens.to_vec();

        while output.len() < max_tokens as usize {
            let accepted = self.speculative_step(&mut context).await?;
            output.extend_from_slice(&accepted);

            if self.is_eos(output.last()) {
                break;
            }
        }

        Ok(output)
    }

    /// Single speculative decoding step.
    async fn speculative_step(&self, context: &mut Vec<u32>) -> Result<Vec<u32>, InferenceError> {
        let draft = self
            .draft_model
            .generate_draft(context, self.config.draft_tokens)
            .await?;

        let verified = self.target_model.verify_tokens(context, &draft).await?;
        let accepted = Self::accept_tokens(&draft, &verified);

        if accepted.is_empty() {
            let token = self.target_model.generate_one(context).await?;
            context.push(token);
            return Ok(vec![token]);
        }

        context.extend_from_slice(&accepted);
        Ok(accepted)
    }

    /// Standard generation without speculation (fallback).
    async fn generate_standard(
        &self,
        prompt_tokens: &[u32],
        max_tokens: u32,
    ) -> Result<Vec<u32>, InferenceError> {
        let mut output = Vec::with_capacity(max_tokens as usize);
        let mut context = prompt_tokens.to_vec();

        while output.len() < max_tokens as usize {
            let token = self.target_model.generate_one(&context).await?;
            output.push(token);
            context.push(token);

            if self.is_eos(Some(&token)) {
                break;
            }
        }

        Ok(output)
    }

    /// Check if token is end-of-sequence.
    fn is_eos(&self, token: Option<&u32>) -> bool {
        match (token, self.target_model.eos_token()) {
            (Some(&t), Some(eos)) => t == eos,
            _ => false,
        }
    }

    /// Accept tokens based on verification result.
    fn accept_tokens(draft: &[u32], verified: &VerifyResult) -> Vec<u32> {
        let mut accepted: Vec<u32> = draft
            .iter()
            .take(verified.accepted_count)
            .copied()
            .collect();

        if let Some(correction) = verified.correction_token {
            accepted.push(correction);
        }

        accepted
    }

    /// Get current configuration.
    pub fn config(&self) -> &SpeculativeConfig {
        &self.config
    }
}
