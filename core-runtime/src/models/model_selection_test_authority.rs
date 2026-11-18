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
            expected_p95_ms: 400
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
        ModelAction::UseResident { model_id: "light", .. }
    ));
    assert_eq!(result.assessments[1].refusal, Some(CandidateRefusal::NotPinned));
}

#[test]
fn pin_to_unapproved_model_fails_policy_validation() {
    let mut p = policy();
    p.pinned_model_id = Some("outsider");
    let result = choose(&[light()], &p);
    assert_eq!(result.action, ModelAction::Refuse(PlanRefusal::InvalidPolicy));
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

