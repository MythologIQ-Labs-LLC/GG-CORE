//! Adversarial model selection source tests. Synthetic offers only.

use super::test_support::{choose, light, policy, quality};
use super::*;

#[test]
fn no_switch_permission_does_not_authorize_a_cold_load() {
    let mut p = policy();
    p.allow_cold_switch = false;
    p.current_model_id = None;
    let result = choose(&[quality()], &p);
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::ColdSwitchNotAuthorized)
    );
}

#[test]
fn no_reroute_permission_does_not_authorize_new_handle() {
    let mut p = policy();
    p.current_model_id = None;
    p.allow_resident_reroute = false;
    let result = choose(&[light()], &p);
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::ResidentRerouteNotAuthorized)
    );
}

#[test]
fn authorized_resident_reroute_is_a_proposal_not_execution() {
    let mut p = policy();
    p.current_model_id = None;
    let result = choose(&[light()], &p);
    assert_eq!(
        result.action,
        ModelAction::ProposeResidentReroute {
            model_id: "light",
            handle_id: 17,
            estimated_total_ms: 400
        }
    );
}

#[test]
fn pinned_model_never_falls_back_to_faster_option() {
    let mut p = policy();
    p.pinned_model_id = Some("light");
    let result = choose(&[light(), quality()], &p);
    assert!(matches!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
    assert_eq!(
        result.assessments[1].refusal,
        Some(CandidateRefusal::NotPinned)
    );
}

#[test]
fn pin_to_unapproved_model_fails_policy_validation() {
    let mut p = policy();
    p.pinned_model_id = Some("outsider");
    let result = choose(&[light()], &p);
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
}

#[test]
fn duplicate_ids_and_empty_allowlist_fail_closed() {
    let duplicate = choose(&[quality(), quality()], &policy());
    assert_eq!(
        duplicate.action,
        ModelAction::Refuse(PlanRefusal::DuplicateModelId)
    );
    let mut p = policy();
    p.allowed_model_ids = &[];
    assert_eq!(
        choose(&[light()], &p).action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
}

#[test]
fn explicit_host_limits_cannot_be_invalid_or_silently_clamped() {
    let mut p = policy();
    p.min_improvement_bps = 10_001;
    assert_eq!(
        choose(&[light()], &p).action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
    p.min_improvement_bps = 1_000;
    p.max_model_memory_bytes = 0;
    assert_eq!(
        choose(&[light()], &p).action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
}

#[test]
fn selection_trace_serializes_without_becoming_an_input_authority() {
    let plan = choose(&[light(), quality()], &policy());
    let json = serde_json::to_value(&plan).expect("output-only selection trace");
    assert_eq!(json["action"]["propose_cold_switch"]["model_id"], "quality");
    assert_eq!(json["assessments"][0]["model_id"], "light");
    assert_eq!(json["assessments"][1]["estimated_total_ms"], 340);
}

#[test]
fn quality_escalation_requires_host_consent_even_if_switch_is_allowed() {
    let mut guard = policy();
    guard.allow_quality_escalation = false;
    let current = choose(&[light(), quality()], &guard);
    assert!(matches!(
        current.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
    guard.quality_floor_bps = 8_000;
    let denied = choose(&[light(), quality()], &guard);
    assert_eq!(
        denied.action,
        ModelAction::Refuse(PlanRefusal::QualityEscalationNotAuthorized)
    );
    // An explicit model pin is itself positive model-choice authorization.
    guard.pinned_model_id = Some("quality");
    let pinned = choose(&[light(), quality()], &guard);
    assert!(matches!(
        pinned.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            ..
        }
    ));
}

#[test]
fn unknown_incumbent_quality_cannot_be_treated_as_escalation_consent() {
    let mut guard = policy();
    guard.allow_quality_escalation = false;
    guard.quality_floor_bps = 8_000;
    let mut old = light();
    old.quality_bps = None;
    let plan = choose(&[old, quality()], &guard);
    assert_eq!(
        plan.action,
        ModelAction::Refuse(PlanRefusal::QualityEscalationNotAuthorized)
    );
}

#[test]
fn unauthorized_faster_cold_load_cannot_hide_a_permitted_resident() {
    let mut host = policy();
    host.current_model_id = None;
    host.allow_cold_switch = false;
    let result = choose(&[light(), quality()], &host);
    assert!(matches!(
        result.action,
        ModelAction::ProposeResidentReroute {
            model_id: "light",
            ..
        }
    ));
}

#[test]
fn unauthorized_cold_tie_must_not_create_a_false_ambiguity() {
    let mut host = policy();
    host.current_model_id = None;
    host.allow_cold_switch = false;
    let mut cold = quality();
    cold.p95_inference_ms = Some(280);
    let result = choose(&[light(), cold], &host);
    assert!(matches!(
        result.action,
        ModelAction::ProposeResidentReroute {
            model_id: "light",
            ..
        }
    ));
}

#[test]
fn unconsented_quality_escalation_cannot_hide_safe_alternative() {
    let mut host = policy();
    host.allow_quality_escalation = false;
    let mut previous = light();
    previous.serving_ready = false;
    let mut alternative = light();
    alternative.model_id = "onnx";
    alternative.resident_handle = Some(18);
    alternative.p95_inference_ms = Some(420);
    let result = choose(&[previous, quality(), alternative], &host);
    assert!(matches!(
        result.action,
        ModelAction::ProposeResidentReroute {
            model_id: "onnx",
            ..
        }
    ));
}
