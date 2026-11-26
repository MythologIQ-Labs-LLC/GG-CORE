//! Local reservation-aware model selection: model-free source acceptance.

use super::*;

#[test]
fn local_budget_planner_tracks_reservations_without_minting_serving_paths() {
    use crate::memory::{ResourceLimits, ResourceLimitsConfig};

    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 2_048,
        max_total_memory: 2_048,
        max_concurrent: 2,
    });
    let offer = test_support::quality();
    let p = test_support::policy();
    let open =
        plan_model_selection_with_local_limits(std::slice::from_ref(&offer), &p, Some(&limits));

    let guard = limits.reserve_resident(512).expect("reserve other model");
    let occupied =
        plan_model_selection_with_local_limits(std::slice::from_ref(&offer), &p, Some(&limits));
    if cfg!(feature = "gguf") {
        assert!(matches!(open.action, ModelAction::ProposeColdSwitch { .. }));
        assert_eq!(
            occupied.assessments[0].refusal,
            Some(CandidateRefusal::OverHardwareHeadroom)
        );
    } else {
        assert_eq!(
            open.assessments[0].refusal,
            Some(CandidateRefusal::PathNotCompiled)
        );
        assert_eq!(
            occupied.assessments[0].refusal,
            Some(CandidateRefusal::PathNotCompiled)
        );
    }
    drop(guard);
    let restored = plan_model_selection_with_local_limits(&[offer], &p, Some(&limits));
    assert_eq!(restored.action, open.action);
}

#[test]
fn local_budget_planner_without_limits_never_invents_cold_fit() {
    let offer = test_support::quality();
    let result = plan_model_selection_with_local_limits(&[offer], &test_support::policy(), None);
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::NoEligibleModel)
    );
    let refusal = if cfg!(feature = "gguf") {
        CandidateRefusal::HardwareHeadroomUnknown
    } else {
        CandidateRefusal::PathNotCompiled
    };
    assert_eq!(result.assessments[0].refusal, Some(refusal));
}

#[test]
fn public_opt_in_resident_preference_uses_real_compiled_paths_and_local_budget() {
    use crate::memory::{ResourceLimits, ResourceLimitsConfig};

    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 4_096,
        max_total_memory: 4_096,
        max_concurrent: 2,
    });
    let offers = [test_support::light(), test_support::quality()];
    let plan = plan_model_selection_with_local_limits_and_preference(
        &offers,
        &test_support::policy(),
        Some(&limits),
        DegradedModelPreference::PreferReadyResident,
    );
    if cfg!(feature = "gguf") {
        assert!(matches!(
            plan.action,
            ModelAction::UseResident {
                model_id: "light",
                handle_id: 17,
                ..
            }
        ));
        assert!(plan.assessments.iter().all(|item| item.refusal.is_none()));
    } else {
        assert_eq!(plan.action, ModelAction::Refuse(PlanRefusal::NoEligibleModel));
        assert!(plan
            .assessments
            .iter()
            .all(|item| item.refusal == Some(CandidateRefusal::PathNotCompiled)));
    }
}
