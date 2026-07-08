//! Unit tests for the adaptive speculative decoding module.
#![cfg(feature = "advanced")]

use super::{
    BlockDraftModel, ConfidenceEstimator, DraftBlock, SurvivalProfile, TargetVerifier,
    VerificationPlan, VerificationResult, VerificationScheduler,
};
use crate::engine::InferenceError;

// ── Mock implementations ──────────────────────────────────────────────────────

struct MockDraft {
    tokens: Vec<u32>,
}

#[async_trait::async_trait]
impl BlockDraftModel for MockDraft {
    async fn draft(&self, _ctx: &[u32], max: usize) -> Result<DraftBlock, InferenceError> {
        let tokens = self.tokens.iter().take(max).copied().collect::<Vec<_>>();
        Ok(DraftBlock::from_tokens(tokens))
    }
}

struct MockTarget {
    accept_count: usize,
    correction: Option<u32>,
    fallback_token: u32,
}

#[async_trait::async_trait]
impl TargetVerifier for MockTarget {
    async fn verify(
        &self,
        _ctx: &[u32],
        draft: &DraftBlock,
        plan: &VerificationPlan,
    ) -> Result<VerificationResult, InferenceError> {
        let window = plan.window.min(draft.tokens.len());
        let accepted = self.accept_count.min(window);
        Ok(if accepted == window {
            VerificationResult::accept_all(accepted)
        } else {
            VerificationResult::reject_at(accepted, self.correction.unwrap_or(self.fallback_token))
        })
    }

    async fn generate_one(&self, _ctx: &[u32]) -> Result<u32, InferenceError> {
        Ok(self.fallback_token)
    }

    fn eos_token(&self) -> Option<u32> {
        Some(2)
    }
}

/// Simple constant-window scheduler.
struct FixedScheduler(usize);

impl VerificationScheduler for FixedScheduler {
    fn plan(&self, _draft: &DraftBlock, _profile: &SurvivalProfile) -> VerificationPlan {
        VerificationPlan {
            window: self.0,
            emit_correction: self.0 > 0,
        }
    }
}

/// Trivial uniform confidence estimator.
struct UnitEstimator;

impl ConfidenceEstimator for UnitEstimator {
    fn estimate(&self, _ctx: &[u32], draft: &DraftBlock) -> SurvivalProfile {
        SurvivalProfile::uniform(draft.tokens.len())
    }
}

// ── Shared step helper ────────────────────────────────────────────────────────

async fn run_step(
    drafter: &impl BlockDraftModel,
    target: &impl TargetVerifier,
    scheduler: &impl VerificationScheduler,
    estimator: &impl ConfidenceEstimator,
    context: &[u32],
    max_draft: usize,
) -> Result<Vec<u32>, InferenceError> {
    let block = drafter.draft(context, max_draft).await?;
    if block.is_empty() {
        let t = target.generate_one(context).await?;
        return Ok(vec![t]);
    }
    let profile = estimator.estimate(context, &block);
    let plan = scheduler.plan(&block, &profile);
    if plan.is_fallback() {
        let t = target.generate_one(context).await?;
        return Ok(vec![t]);
    }
    let result = target.verify(context, &block, &plan).await?;
    Ok(result.into_tokens(&block.tokens))
}

// ── Async path tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn success_path_all_tokens_accepted() {
    let drafter = MockDraft {
        tokens: vec![10, 20, 30, 40],
    };
    let target = MockTarget {
        accept_count: 4,
        correction: None,
        fallback_token: 99,
    };
    let out = run_step(
        &drafter,
        &target,
        &FixedScheduler(4),
        &UnitEstimator,
        &[1, 2],
        4,
    )
    .await
    .unwrap();
    assert_eq!(out, vec![10, 20, 30, 40]);
}

#[tokio::test]
async fn rejection_path_emits_correction() {
    let drafter = MockDraft {
        tokens: vec![10, 20, 30, 40],
    };
    let target = MockTarget {
        accept_count: 2,
        correction: Some(77),
        fallback_token: 99,
    };
    let out = run_step(
        &drafter,
        &target,
        &FixedScheduler(4),
        &UnitEstimator,
        &[1, 2],
        4,
    )
    .await
    .unwrap();
    // draft[0..2] + correction
    assert_eq!(out, vec![10, 20, 77]);
}

#[tokio::test]
async fn fallback_path_when_draft_empty() {
    let drafter = MockDraft { tokens: vec![] };
    let target = MockTarget {
        accept_count: 0,
        correction: None,
        fallback_token: 55,
    };
    let out = run_step(
        &drafter,
        &target,
        &FixedScheduler(4),
        &UnitEstimator,
        &[1],
        4,
    )
    .await
    .unwrap();
    assert_eq!(out, vec![55]);
}

#[tokio::test]
async fn fallback_path_when_scheduler_returns_zero_window() {
    let drafter = MockDraft {
        tokens: vec![10, 20, 30],
    };
    let target = MockTarget {
        accept_count: 3,
        correction: None,
        fallback_token: 55,
    };
    // Zero-window scheduler forces fallback even when draft has tokens.
    let out = run_step(
        &drafter,
        &target,
        &FixedScheduler(0),
        &UnitEstimator,
        &[1],
        4,
    )
    .await
    .unwrap();
    assert_eq!(out, vec![55]);
}

// ── Type / constructor unit tests ─────────────────────────────────────────────

#[test]
fn draft_block_from_tokens_fills_log_probs() {
    let b = DraftBlock::from_tokens(vec![1, 2, 3]);
    assert_eq!(b.tokens.len(), b.log_probs.len());
    assert!(b
        .log_probs
        .iter()
        .all(|p| p.is_infinite() && p.is_sign_negative()));
}

#[test]
fn verification_result_into_tokens_all_accepted() {
    let result = VerificationResult::accept_all(3);
    let draft = vec![10u32, 20, 30];
    assert_eq!(result.into_tokens(&draft), vec![10, 20, 30]);
}

#[test]
fn verification_result_into_tokens_with_correction() {
    let result = VerificationResult::reject_at(1, 99);
    let draft = vec![10u32, 20, 30];
    assert_eq!(result.into_tokens(&draft), vec![10, 99]);
}

#[test]
fn survival_profile_uniform_length() {
    let sp = SurvivalProfile::uniform(5);
    assert_eq!(sp.scores.len(), 5);
    assert!(sp.scores.iter().all(|&s| (s - 1.0).abs() < f32::EPSILON));
}

#[test]
fn verification_plan_fallback_is_zero_window() {
    let plan = VerificationPlan::fallback();
    assert!(plan.is_fallback());
    assert_eq!(plan.window, 0);
}
