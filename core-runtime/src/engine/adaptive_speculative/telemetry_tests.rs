//! Unit tests for [`super::SpeculativeTelemetry`].
#![cfg(feature = "advanced")]

use super::{AutoDisableReason, SpeculativeTelemetry};

#[test]
fn initial_snapshot_is_zeroed() {
    let t = SpeculativeTelemetry::new();
    let s = t.snapshot();
    assert_eq!(s.draft_tokens_generated, 0);
    assert_eq!(s.verification_steps, 0);
    assert_eq!(s.accepted_tokens, 0);
    assert_eq!(s.rejected_tokens, 0);
    assert!((s.acceptance_rate).abs() < f32::EPSILON);
    assert!((s.mean_accepted_length).abs() < f32::EPSILON);
    assert_eq!(s.auto_disable_count, 0);
    assert!(s.auto_disable_reason.is_none());
}

#[test]
fn stats_accumulate_across_multiple_steps() {
    let t = SpeculativeTelemetry::new();
    // step 1: 4 drafted, 3 accepted
    t.record_step(4, 3, 100, 80);
    // step 2: 4 drafted, 4 accepted
    t.record_step(4, 4, 120, 90);
    // step 3: 4 drafted, 2 accepted
    t.record_step(4, 2, 110, 85);

    let s = t.snapshot();
    assert_eq!(s.draft_tokens_generated, 12);
    assert_eq!(s.verification_steps, 3);
    assert_eq!(s.accepted_tokens, 9);
    assert_eq!(s.rejected_tokens, 3);
}

#[test]
fn acceptance_rate_computes_correctly() {
    let t = SpeculativeTelemetry::new();
    // 50% then 100% acceptance
    t.record_step(4, 2, 100, 80);
    t.record_step(4, 4, 100, 80);

    let s = t.snapshot();
    // 6 accepted / 8 drafted = 0.75
    assert!((s.acceptance_rate - 0.75).abs() < 1e-5);
}

#[test]
fn acceptance_rate_zero_when_no_steps() {
    let t = SpeculativeTelemetry::new();
    let s = t.snapshot();
    assert!((s.acceptance_rate).abs() < f32::EPSILON);
}

#[test]
fn auto_disable_reason_recorded() {
    let t = SpeculativeTelemetry::new();
    t.record_auto_disable(&AutoDisableReason::AcceptanceRateLow);
    let s = t.snapshot();
    assert_eq!(s.auto_disable_count, 1);
    assert_eq!(s.auto_disable_reason.as_deref(), Some("ACCEPTANCE_RATE_LOW"));
}

#[test]
fn auto_disable_reason_overwritten_by_latest() {
    let t = SpeculativeTelemetry::new();
    t.record_auto_disable(&AutoDisableReason::AcceptanceRateLow);
    t.record_auto_disable(&AutoDisableReason::SpeedupBelowThreshold);
    let s = t.snapshot();
    assert_eq!(s.auto_disable_count, 2);
    assert_eq!(s.auto_disable_reason.as_deref(), Some("SPEEDUP_BELOW_THRESHOLD"));
}

#[test]
fn snapshot_is_immutable_view_does_not_advance_state() {
    let t = SpeculativeTelemetry::new();
    t.record_step(4, 4, 100, 80);
    let s1 = t.snapshot();
    let s2 = t.snapshot();
    assert_eq!(s1.verification_steps, s2.verification_steps);
    assert_eq!(s1.accepted_tokens, s2.accepted_tokens);
}

#[test]
fn reset_clears_all_accumulators() {
    let t = SpeculativeTelemetry::new();
    t.record_step(4, 3, 100, 80);
    t.record_auto_disable(&AutoDisableReason::ExplicitDisable);
    t.reset();
    let s = t.snapshot();
    assert_eq!(s.draft_tokens_generated, 0);
    assert_eq!(s.verification_steps, 0);
    assert_eq!(s.auto_disable_count, 0);
    assert!(s.auto_disable_reason.is_none());
}

#[test]
fn net_speedup_at_least_one() {
    let t = SpeculativeTelemetry::new();
    t.record_step(4, 0, 100, 80); // zero accepted
    let s = t.snapshot();
    assert!(s.net_speedup >= 1.0);
}

#[test]
fn mean_latencies_compute_correctly() {
    let t = SpeculativeTelemetry::new();
    t.record_step(4, 4, 100, 80);
    t.record_step(4, 4, 200, 120);
    let s = t.snapshot();
    assert!((s.mean_draft_latency_us - 150.0).abs() < 1e-3);
    assert!((s.mean_verify_latency_us - 100.0).abs() < 1e-3);
}

#[test]
fn all_auto_disable_reason_codes_are_stable() {
    assert_eq!(AutoDisableReason::AcceptanceRateLow.as_code(), "ACCEPTANCE_RATE_LOW");
    assert_eq!(AutoDisableReason::SpeedupBelowThreshold.as_code(), "SPEEDUP_BELOW_THRESHOLD");
    assert_eq!(AutoDisableReason::PairingIncompatible.as_code(), "PAIRING_INCOMPATIBLE");
    assert_eq!(AutoDisableReason::ExplicitDisable.as_code(), "EXPLICIT_DISABLE");
}
