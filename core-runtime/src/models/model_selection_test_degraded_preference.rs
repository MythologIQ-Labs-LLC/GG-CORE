//! Host-governed degraded model preference: all offer data is synthetic.

use super::*;
use crate::models::backend_capabilities::{
    BackendKind, ExecutionTarget, InferenceTask, RuntimeBackendCapabilities, ServingPath,
    CAPABILITY_SCHEMA_VERSION,
};

fn catalog() -> Vec<RuntimeBackendCapabilities> {
    vec![RuntimeBackendCapabilities {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        backend: BackendKind::Gguf,
        model_formats: vec!["gguf".into()],
        compiled_paths: vec![ServingPath::new(
            InferenceTask::TextGeneration,
            ExecutionTarget::Cpu,
        )],
        supports_streaming: true,
        supports_kv_cache: true,
        qualified_paths: vec![],
    }]
}

fn select<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    pref: DegradedModelPreference,
) -> ModelSelectionPlan<'a> {
    plan_with_catalog_and_degraded_preference(offers, policy, &catalog(), pref)
}

#[test]
fn existing_ranking_is_unchanged_by_default() {
    let offers = [test_support::light(), test_support::quality()];
    let baseline = test_support::choose(&offers, &test_support::policy());
    let opted = select(
        &offers,
        &test_support::policy(),
        DegradedModelPreference::FastestEligible,
    );
    assert_eq!(baseline, opted);
    assert!(matches!(
        opted.action,
        ModelAction::ProposeColdSwitch { .. }
    ));
}

#[test]
fn prefer_ready_resident_over_a_cheaper_cold_offer() {
    let offers = [test_support::light(), test_support::quality()];
    let plan = select(
        &offers,
        &test_support::policy(),
        DegradedModelPreference::PreferReadyResident,
    );
    assert!(matches!(
        plan.action,
        ModelAction::UseResident {
            model_id: "light",
            handle_id: 17,
            ..
        }
    ));
    assert!(plan.assessments.iter().all(|item| item.refusal.is_none()));
}

#[test]
fn prefer_resident_falls_back_to_cold_only_if_no_ready_resident() {
    let mut resident = test_support::light();
    resident.serving_ready = false;
    let plan = select(
        &[resident, test_support::quality()],
        &test_support::policy(),
        DegradedModelPreference::PreferReadyResident,
    );
    assert_eq!(
        plan.assessments[0].refusal,
        Some(CandidateRefusal::NotReady)
    );
    assert!(matches!(
        plan.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            ..
        }
    ));
}

#[test]
fn require_resident_refuses_an_otherwise_eligible_cold_candidate() {
    let mut host = test_support::policy();
    host.current_model_id = None;
    let plan = select(
        &[test_support::quality()],
        &host,
        DegradedModelPreference::RequireReadyResident,
    );
    assert_eq!(
        plan.action,
        ModelAction::Refuse(PlanRefusal::ResidentRequiredByHost)
    );
    assert_eq!(plan.assessments[0].refusal, None);
}

#[test]
fn residency_preference_cannot_override_quality_floor_or_pin() {
    let offers = [test_support::light(), test_support::quality()];
    let mut host = test_support::policy();
    host.quality_floor_bps = 8_000;
    let no_quality = select(
        &offers,
        &host,
        DegradedModelPreference::RequireReadyResident,
    );
    assert_eq!(
        no_quality.assessments[0].refusal,
        Some(CandidateRefusal::BelowQualityFloor)
    );
    assert_eq!(
        no_quality.action,
        ModelAction::Refuse(PlanRefusal::ResidentRequiredByHost)
    );
    host.quality_floor_bps = 7_000;
    host.pinned_model_id = Some("quality");
    let pinned = select(&offers, &host, DegradedModelPreference::PreferReadyResident);
    assert_eq!(
        pinned.assessments[0].refusal,
        Some(CandidateRefusal::NotPinned)
    );
    assert!(matches!(
        pinned.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            ..
        }
    ));
}

#[test]
fn no_reroute_consent_cannot_be_minted_by_preference() {
    let mut host = test_support::policy();
    host.current_model_id = None;
    host.allow_resident_reroute = false;
    let offers = [test_support::light(), test_support::quality()];
    let required = select(
        &offers,
        &host,
        DegradedModelPreference::RequireReadyResident,
    );
    assert_eq!(
        required.action,
        ModelAction::Refuse(PlanRefusal::ResidentRequiredByHost)
    );
    let preferred = select(&offers, &host, DegradedModelPreference::PreferReadyResident);
    assert!(matches!(
        preferred.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            ..
        }
    ));
}

#[test]
fn no_escalation_consent_cannot_be_minted_by_ready_residency() {
    let mut host = test_support::policy();
    host.allow_quality_escalation = false;
    host.quality_floor_bps = 8_000;
    let mut quality = test_support::quality();
    quality.resident_handle = Some(29);
    quality.serving_ready = true;
    let plan = select(
        &[test_support::light(), quality],
        &host,
        DegradedModelPreference::RequireReadyResident,
    );
    assert_eq!(
        plan.action,
        ModelAction::Refuse(PlanRefusal::ResidentRequiredByHost)
    );
}
