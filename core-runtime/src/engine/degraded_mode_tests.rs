//! Tests for the degraded-mode decision + the truncation helper.

use super::{
    truncate_on_char_boundary, DegradedDecision, DegradedModeConfig, DegradedModePolicy,
    ResourcePressure,
};

#[test]
fn context_over_budget_reduces_when_allowed() {
    let policy = DegradedModePolicy::default();
    match policy.evaluate(ResourcePressure::Context { max: 100, got: 150 }) {
        DegradedDecision::ReduceContextTo { tokens, reason } => {
            assert_eq!(tokens, 100);
            assert!(!reason.is_empty(), "decision must carry an explanation");
        }
        other => panic!("expected ReduceContextTo, got {other:?}"),
    }
}

#[test]
fn context_reduction_disabled_rejects() {
    let policy = DegradedModePolicy::new(DegradedModeConfig {
        allow_context_reduction: false,
        min_context_tokens: 16,
    });
    match policy.evaluate(ResourcePressure::Context { max: 100, got: 150 }) {
        DegradedDecision::Reject { reason } => assert!(reason.contains("reduction disabled")),
        other => panic!("expected Reject, got {other:?}"),
    }
}

#[test]
fn context_below_min_tokens_rejects() {
    let policy = DegradedModePolicy::new(DegradedModeConfig {
        allow_context_reduction: true,
        min_context_tokens: 200,
    });
    assert!(matches!(
        policy.evaluate(ResourcePressure::Context { max: 100, got: 150 }),
        DegradedDecision::Reject { .. }
    ));
}

#[test]
fn memory_pressure_rejects_with_reason() {
    let policy = DegradedModePolicy::default();
    match policy.evaluate(ResourcePressure::Memory {
        used: 2048,
        limit: 1024,
    }) {
        DegradedDecision::Reject { reason } => assert!(reason.contains("no smaller model")),
        other => panic!("expected Reject, got {other:?}"),
    }
}

#[test]
fn capability_pressure_rejects_with_reason() {
    let policy = DegradedModePolicy::default();
    match policy.evaluate(ResourcePressure::Capability {
        name: "chat".into(),
    }) {
        DegradedDecision::Reject { reason } => assert!(reason.contains("chat")),
        other => panic!("expected Reject, got {other:?}"),
    }
}

#[test]
fn truncate_on_char_boundary_never_splits_utf8() {
    // "héllo" — the 'é' is 2 bytes (indices 1..3). A byte budget of 2 lands
    // mid-codepoint; truncation must back off to the boundary at byte 1.
    let s = "héllo";
    let out = truncate_on_char_boundary(s, 2);
    assert!(out.len() <= 2);
    assert!(s.starts_with(&out), "must be a prefix of the original");
    assert_eq!(out, "h"); // backed off from the 'é' boundary
                          // Whole string when within budget.
    assert_eq!(truncate_on_char_boundary(s, 100), s);
}
