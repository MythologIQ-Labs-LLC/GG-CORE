//! Adaptive speculative decoding traits and types.
//!
//! Provides backend-agnostic abstractions for block-level draft-verify
//! speculation. Callers compose [`BlockDraftModel`], [`ConfidenceEstimator`],
//! [`VerificationScheduler`], and [`TargetVerifier`] to construct a
//! fully adaptive speculative decode loop.
//!
//! Single-model fallback: when no draft is available the scheduler returns
//! a zero-size [`VerificationPlan`] and the executor calls
//! [`TargetVerifier::generate_one`] directly.
//!
//! # Example
//!
//! ```rust,ignore
//! # #[cfg(feature = "advanced")]
//! # {
//! use gg_core::engine::adaptive_speculative::{
//!     DraftBlock, SurvivalProfile, VerificationPlan, VerificationResult,
//! };
//! # }
//! ```

#![cfg(feature = "advanced")]

use crate::engine::InferenceError;

// ── Types ────────────────────────────────────────────────────────────────────

/// A block of draft tokens proposed by a draft model.
///
/// Carries the raw token ids together with per-token log-probabilities
/// assigned by the drafting model. Log-probs are `f32::NEG_INFINITY`
/// when the model does not supply them (e.g. greedy-only backends).
#[derive(Debug, Clone)]
pub struct DraftBlock {
    /// Proposed token ids, in generation order.
    pub tokens: Vec<u32>,
    /// Log-probability for each token under the draft model distribution.
    pub log_probs: Vec<f32>,
}

impl DraftBlock {
    /// Construct a block from tokens without probability information.
    pub fn from_tokens(tokens: Vec<u32>) -> Self {
        let n = tokens.len();
        Self {
            tokens,
            log_probs: vec![f32::NEG_INFINITY; n],
        }
    }

    /// Returns `true` when this block carries no draft tokens.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// Survival profile: per-token confidence scores used by the scheduler.
///
/// Values are in `[0.0, 1.0]`. A value of `1.0` indicates the estimator
/// is certain the target model will accept that draft token; `0.0`
/// indicates certainty of rejection.
#[derive(Debug, Clone)]
pub struct SurvivalProfile {
    /// Confidence score for each draft token at the matching index.
    pub scores: Vec<f32>,
}

impl SurvivalProfile {
    /// All-ones profile — use when confidence estimation is unavailable.
    pub fn uniform(len: usize) -> Self {
        Self {
            scores: vec![1.0; len],
        }
    }
}

/// A verification plan produced by the scheduler.
///
/// A zero-`window` plan signals that speculation is skipped this step
/// and the target model should generate exactly one token.
#[derive(Debug, Clone)]
pub struct VerificationPlan {
    /// Number of draft tokens the target verifier should inspect.
    pub window: usize,
    /// Whether the target model should emit a correction token on
    /// first rejection.
    pub emit_correction: bool,
}

impl VerificationPlan {
    /// Plan requesting fallback to single-token generation.
    pub fn fallback() -> Self {
        Self {
            window: 0,
            emit_correction: false,
        }
    }

    /// Returns `true` when the plan requests standard (non-speculative) decoding.
    pub fn is_fallback(&self) -> bool {
        self.window == 0
    }
}

/// Result returned from the target verifier for a single speculation step.
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Number of draft tokens accepted by the target model.
    pub accepted_count: usize,
    /// Correction token emitted at the first rejection position, if any.
    pub correction_token: Option<u32>,
    /// Log-probabilities assigned by the target model to each draft token.
    pub target_log_probs: Vec<f32>,
}

impl VerificationResult {
    /// All draft tokens accepted; no correction needed.
    pub fn accept_all(count: usize) -> Self {
        Self {
            accepted_count: count,
            correction_token: None,
            target_log_probs: vec![0.0; count],
        }
    }

    /// Diverged at `accepted` tokens; `correction` is the target-sampled token.
    pub fn reject_at(accepted: usize, correction: u32) -> Self {
        Self {
            accepted_count: accepted,
            correction_token: Some(correction),
            target_log_probs: vec![0.0; accepted],
        }
    }

    /// Collect all output tokens (accepted draft tokens + optional correction).
    pub fn into_tokens(self, draft: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = draft.iter().take(self.accepted_count).copied().collect();
        if let Some(t) = self.correction_token {
            out.push(t);
        }
        out
    }
}

// ── Traits ───────────────────────────────────────────────────────────────────

/// Generates a block of draft tokens from a context prefix.
///
/// Implementations may be backed by a small GGUF model, a quantized
/// distilled model, or any future backend. The interface is intentionally
/// backend-agnostic.
///
/// Returning an empty [`DraftBlock`] signals that the draft model cannot
/// provide candidates for the current context; the executor will fall
/// back to standard single-token decoding.
#[async_trait::async_trait]
pub trait BlockDraftModel: Send + Sync {
    /// Generate up to `max_tokens` draft tokens from `context`.
    async fn draft(&self, context: &[u32], max_tokens: usize)
        -> Result<DraftBlock, InferenceError>;
}

/// Estimates per-token confidence for a proposed [`DraftBlock`].
///
/// Implementations range from trivial (constant 1.0) to learned confidence
/// heads (v2+). The `v1` contract: every implementation must be callable
/// with a blank context slice and must never panic.
///
/// Returning a [`SurvivalProfile`] with a length that differs from
/// `draft.tokens.len()` is a caller contract violation and may cause
/// the scheduler to clamp or pad.
pub trait ConfidenceEstimator: Send + Sync {
    /// Estimate per-token confidence for the given draft.
    fn estimate(&self, context: &[u32], draft: &DraftBlock) -> SurvivalProfile;
}

/// Decides how many draft tokens to submit for verification.
///
/// Given the draft block and its survival profile, the scheduler returns
/// a [`VerificationPlan`]. A zero-window plan triggers fallback.
///
/// Implementations may use acceptance-rate history, mode (Conservative /
/// Balanced / Aggressive from [`AdaptiveMode`]), or cost models.
///
/// [`AdaptiveMode`]: crate::models::speculative_config::AdaptiveMode
pub trait VerificationScheduler: Send + Sync {
    /// Produce a verification plan for the current step.
    fn plan(&self, draft: &DraftBlock, profile: &SurvivalProfile) -> VerificationPlan;
}

/// Verifies a draft block against the target model distribution.
///
/// Implementations wrap the target (large) model. The trait is kept
/// symmetric with [`crate::engine::speculative::TargetModel`] so that
/// existing GGUF wrappers can implement both without duplication.
#[async_trait::async_trait]
pub trait TargetVerifier: Send + Sync {
    /// Verify `plan.window` tokens from `draft` given `context`.
    ///
    /// Must return a [`VerificationResult`] even when `plan.window == 0`;
    /// in that case callers use [`TargetVerifier::generate_one`] instead.
    async fn verify(
        &self,
        context: &[u32],
        draft: &DraftBlock,
        plan: &VerificationPlan,
    ) -> Result<VerificationResult, InferenceError>;

    /// Generate exactly one token (fallback / standard decoding path).
    async fn generate_one(&self, context: &[u32]) -> Result<u32, InferenceError>;

    /// End-of-sequence token id, if the backend exposes it.
    fn eos_token(&self) -> Option<u32>;
}

pub mod heuristic;
pub mod telemetry;

#[cfg(test)]
mod tests;
