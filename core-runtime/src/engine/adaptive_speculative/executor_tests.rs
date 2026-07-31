//! Tests for the adaptive speculative executor (B-21c).
//!
//! The load-bearing test is `commits_correction_on_reject`: the rejected draft
//! suffix must never be committed. Mocks supply finite draft log-probs so the
//! heuristic scheduler plans a real (non-fallback) verification window.

use async_trait::async_trait;

use super::AdaptiveSpeculativeExecutor;
use crate::engine::adaptive_speculative::heuristic::{
    AdaptiveVerificationScheduler, HeuristicConfidenceEstimator,
};
use crate::engine::adaptive_speculative::telemetry::SpeculativeTelemetry;
use crate::engine::adaptive_speculative::{
    BlockDraftModel, DraftBlock, TargetVerifier, VerificationPlan, VerificationResult,
};
use crate::engine::InferenceError;
use crate::models::speculative_config::{AdaptiveMode, AdaptiveSpeculativeConfig};

struct MockDraft {
    tokens: Vec<u32>,
    log_probs: Vec<f32>,
}

#[async_trait]
impl BlockDraftModel for MockDraft {
    async fn draft(&self, _context: &[u32], _max: usize) -> Result<DraftBlock, InferenceError> {
        Ok(DraftBlock {
            tokens: self.tokens.clone(),
            log_probs: self.log_probs.clone(),
        })
    }
}

struct MockVerifier {
    accepted: usize,
    correction: Option<u32>,
    one: u32,
    eos: Option<u32>,
}

#[async_trait]
impl TargetVerifier for MockVerifier {
    async fn verify(
        &self,
        _context: &[u32],
        _draft: &DraftBlock,
        _plan: &VerificationPlan,
    ) -> Result<VerificationResult, InferenceError> {
        Ok(VerificationResult {
            accepted_count: self.accepted,
            correction_token: self.correction,
            target_log_probs: vec![],
        })
    }

    async fn generate_one(&self, _context: &[u32]) -> Result<u32, InferenceError> {
        Ok(self.one)
    }

    fn eos_token(&self) -> Option<u32> {
        self.eos
    }
}

fn active_config(mode: AdaptiveMode) -> AdaptiveSpeculativeConfig {
    AdaptiveSpeculativeConfig {
        enabled: true,
        mode,
        ..Default::default()
    }
}

async fn run_with(
    draft: MockDraft,
    verifier: MockVerifier,
    config: AdaptiveSpeculativeConfig,
    max_tokens: usize,
) -> Vec<u32> {
    let estimator = HeuristicConfidenceEstimator::neutral();
    let scheduler = AdaptiveVerificationScheduler::new(config);
    let telemetry = SpeculativeTelemetry::new();
    let exec =
        AdaptiveSpeculativeExecutor::new(&draft, &verifier, &estimator, &scheduler, &telemetry, 4);
    exec.run(&[100, 101], max_tokens).await.expect("run")
}

// High-confidence log-probs (>= -0.1 saturates the base score) so the scheduler
// plans a non-fallback window.
fn hi(n: usize) -> Vec<f32> {
    vec![0.0; n]
}

#[tokio::test]
async fn accepts_full_block() {
    let draft = MockDraft {
        tokens: vec![1, 2, 3, 4],
        log_probs: hi(4),
    };
    let verifier = MockVerifier {
        accepted: 4,
        correction: None,
        one: 7,
        eos: None,
    };
    let out = run_with(draft, verifier, active_config(AdaptiveMode::Balanced), 4).await;
    assert_eq!(out, vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn commits_correction_on_reject() {
    // Draft [1,2,3,4]; target accepts only [1] and corrects to 99. The rejected
    // suffix [2,3,4] must NEVER appear in the committed output.
    let draft = MockDraft {
        tokens: vec![1, 2, 3, 4],
        log_probs: hi(4),
    };
    let verifier = MockVerifier {
        accepted: 1,
        correction: Some(99),
        one: 7,
        eos: None,
    };
    let out = run_with(draft, verifier, active_config(AdaptiveMode::Balanced), 8).await;
    assert!(!out.is_empty());
    assert!(
        !out.contains(&2),
        "rejected suffix token 2 committed: {out:?}"
    );
    assert!(
        !out.contains(&3),
        "rejected suffix token 3 committed: {out:?}"
    );
    assert!(
        !out.contains(&4),
        "rejected suffix token 4 committed: {out:?}"
    );
    // Only accepted (1) + correction (99) tokens are emitted.
    assert!(
        out.iter().all(|&t| t == 1 || t == 99),
        "unexpected token: {out:?}"
    );
}

#[tokio::test]
async fn empty_draft_falls_back_to_one() {
    let draft = MockDraft {
        tokens: vec![],
        log_probs: vec![],
    };
    let verifier = MockVerifier {
        accepted: 0,
        correction: None,
        one: 7,
        eos: None,
    };
    let out = run_with(draft, verifier, active_config(AdaptiveMode::Balanced), 3).await;
    assert_eq!(out, vec![7, 7, 7]);
}

#[tokio::test]
async fn stops_at_eos() {
    // Empty draft -> generate_one returns 5, which is EOS -> stop after one token.
    let draft = MockDraft {
        tokens: vec![],
        log_probs: vec![],
    };
    let verifier = MockVerifier {
        accepted: 0,
        correction: None,
        one: 5,
        eos: Some(5),
    };
    let out = run_with(draft, verifier, active_config(AdaptiveMode::Balanced), 10).await;
    assert_eq!(out, vec![5]);
}

#[tokio::test]
async fn disabled_config_falls_back_to_single_tokens() {
    // Disabled config -> scheduler plans fallback -> executor uses generate_one,
    // never the draft block.
    let draft = MockDraft {
        tokens: vec![1, 2, 3, 4],
        log_probs: hi(4),
    };
    let verifier = MockVerifier {
        accepted: 4,
        correction: None,
        one: 8,
        eos: None,
    };
    let out = run_with(draft, verifier, AdaptiveSpeculativeConfig::default(), 2).await;
    assert_eq!(out, vec![8, 8]);
}
