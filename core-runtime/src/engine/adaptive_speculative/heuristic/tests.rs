//! Unit tests for heuristic confidence estimator and adaptive scheduler.
#![cfg(feature = "advanced")]

use super::{
    AcceptanceHistory, AdaptiveVerificationScheduler, HeuristicConfidenceEstimator, HISTORY_LEN,
};
use crate::engine::adaptive_speculative::{DraftBlock, SurvivalProfile};
use crate::models::speculative_config::{AdaptiveMode, AdaptiveSpeculativeConfig};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_draft(log_probs: Vec<f32>) -> DraftBlock {
    let n = log_probs.len();
    DraftBlock {
        tokens: vec![1u32; n],
        log_probs,
    }
}

fn balanced_config() -> AdaptiveSpeculativeConfig {
    AdaptiveSpeculativeConfig {
        enabled: true,
        mode: AdaptiveMode::Balanced,
        max_draft_tokens: 8,
        min_verification_tokens: 1,
        max_verification_tokens: 8,
        confidence_floor: 0.70,
        acceptance_floor: 0.60,
        auto_disable: true,
        auto_disable_threshold: 1.05,
        ..Default::default()
    }
}

fn warm_scheduler(sched: &AdaptiveVerificationScheduler) {
    for _ in 0..HISTORY_LEN {
        sched.record_result(4, 4);
    }
}

// ── AcceptanceHistory ─────────────────────────────────────────────────────────

#[test]
fn history_neutral_prior_when_empty() {
    let h = AcceptanceHistory::new();
    assert!((h.mean() - 0.5).abs() < f32::EPSILON);
}

#[test]
fn history_mean_reflects_pushed_values() {
    let h = AcceptanceHistory::new();
    h.push(1.0);
    h.push(0.0);
    assert!((h.mean() - 0.5).abs() < 0.01);
}

// ── ConfidenceEstimator ───────────────────────────────────────────────────────

#[test]
fn high_confidence_draft_scores_near_one() {
    use crate::engine::adaptive_speculative::ConfidenceEstimator;
    let est = HeuristicConfidenceEstimator::neutral();
    let draft = make_draft(vec![-0.05, -0.02, -0.08]);
    let profile = est.estimate(&[], &draft);
    assert_eq!(profile.scores.len(), 3);
    assert!(
        profile.scores.iter().all(|&s| s > 0.85),
        "scores: {:?}",
        profile.scores
    );
}

#[test]
fn low_confidence_draft_scores_near_zero() {
    use crate::engine::adaptive_speculative::ConfidenceEstimator;
    let est = HeuristicConfidenceEstimator::neutral();
    let draft = make_draft(vec![-12.0, -11.5, -10.5]);
    let profile = est.estimate(&[], &draft);
    assert!(
        profile.scores.iter().all(|&s| s < 0.2),
        "scores: {:?}",
        profile.scores
    );
}

#[test]
fn empty_draft_returns_empty_profile() {
    use crate::engine::adaptive_speculative::ConfidenceEstimator;
    let est = HeuristicConfidenceEstimator::neutral();
    let draft = DraftBlock::from_tokens(vec![]);
    let profile = est.estimate(&[], &draft);
    assert!(profile.scores.is_empty());
}

#[test]
fn historical_acceptance_shifts_scores_upward() {
    use crate::engine::adaptive_speculative::ConfidenceEstimator;
    let est = HeuristicConfidenceEstimator::neutral();
    for _ in 0..HISTORY_LEN {
        est.record_acceptance(1.0);
    }
    let draft = make_draft(vec![-1.0, -1.0]);
    let high = est.estimate(&[], &draft);

    let est2 = HeuristicConfidenceEstimator::neutral();
    for _ in 0..HISTORY_LEN {
        est2.record_acceptance(0.0);
    }
    let low = est2.estimate(&[], &draft);

    assert!(
        high.scores[0] > low.scores[0],
        "high={:?} low={:?}",
        high.scores,
        low.scores
    );
}

// ── VerificationScheduler ─────────────────────────────────────────────────────

#[test]
fn high_confidence_profile_selects_wide_window() {
    use crate::engine::adaptive_speculative::VerificationScheduler;
    let sched = AdaptiveVerificationScheduler::new(balanced_config());
    warm_scheduler(&sched);
    let draft = make_draft(vec![-0.05; 6]);
    let profile = SurvivalProfile {
        scores: vec![0.95; 6],
    };
    let plan = sched.plan(&draft, &profile);
    assert!(
        !plan.is_fallback(),
        "expected non-fallback plan, got {:?}",
        plan
    );
    assert!(
        plan.window >= 4,
        "expected wide window, got {}",
        plan.window
    );
}

#[test]
fn low_confidence_profile_selects_narrow_window() {
    use crate::engine::adaptive_speculative::VerificationScheduler;
    let sched = AdaptiveVerificationScheduler::new(balanced_config());
    warm_scheduler(&sched);
    let draft = make_draft(vec![-0.05; 6]);
    let profile = SurvivalProfile {
        scores: vec![0.1; 6],
    };
    let plan = sched.plan(&draft, &profile);
    assert!(
        plan.window <= 2,
        "expected narrow window, got {}",
        plan.window
    );
}

#[test]
fn auto_disable_fires_below_speedup_threshold() {
    use crate::engine::adaptive_speculative::VerificationScheduler;
    let cfg = AdaptiveSpeculativeConfig {
        auto_disable: true,
        auto_disable_threshold: 1.90,
        ..balanced_config()
    };
    let sched = AdaptiveVerificationScheduler::new(cfg);
    for _ in 0..HISTORY_LEN {
        sched.record_result(0, 4);
    }
    let draft = make_draft(vec![-0.05; 4]);
    let profile = SurvivalProfile::uniform(4);
    let plan = sched.plan(&draft, &profile);
    assert!(plan.is_fallback(), "auto-disable should return fallback");
    assert!(sched.auto_disable_fired());
}

#[test]
fn underperforming_path_with_auto_disable_off() {
    use crate::engine::adaptive_speculative::VerificationScheduler;
    let cfg = AdaptiveSpeculativeConfig {
        auto_disable: false,
        auto_disable_threshold: 1.90,
        ..balanced_config()
    };
    let sched = AdaptiveVerificationScheduler::new(cfg);
    for _ in 0..HISTORY_LEN {
        sched.record_result(0, 4);
    }
    let draft = make_draft(vec![-0.05; 4]);
    let profile = SurvivalProfile::uniform(4);
    let plan = sched.plan(&draft, &profile);
    assert!(!sched.auto_disable_fired());
    assert!(!plan.is_fallback(), "auto_disable=false must not fall back");
}

#[test]
fn disabled_mode_always_returns_fallback() {
    use crate::engine::adaptive_speculative::VerificationScheduler;
    let cfg = AdaptiveSpeculativeConfig {
        enabled: true,
        mode: AdaptiveMode::Disabled,
        ..Default::default()
    };
    let sched = AdaptiveVerificationScheduler::new(cfg);
    let draft = make_draft(vec![-0.05; 4]);
    let profile = SurvivalProfile::uniform(4);
    let plan = sched.plan(&draft, &profile);
    assert!(plan.is_fallback());
}
