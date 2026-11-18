//! Adversarial model selection cases. Every "qualified" offer below is
//! synthetic policy input, NOT a real-world model or platform qualification.

use super::*;

const ALLOWED: &[&str] = &["light", "quality", "onnx"];

fn policy() -> SelectionPolicy<'static> {
    SelectionPolicy {
        allowed_model_ids: ALLOWED,
        pinned_model_id: None,
        task: InferenceTask::TextGeneration,
        target: ExecutionTarget::Cpu,
        require_streaming: true,
        quality_floor_bps: 7_000,
        max_model_memory_bytes: 3_000,
        max_p95_total_ms: 1_000,
        current_model_id: Some("light"),
        allow_resident_reroute: true,
        allow_cold_switch: true,
        cooldown_active: false,
        min_improvement_bps: 1_000,
    }
}

fn light() -> ModelOffer<'static> {
    ModelOffer {
        model_id: "light",
        backend: BackendKind::Gguf,
        task: InferenceTask::TextGeneration,
        target: ExecutionTarget::Cpu,
        evidence_qualified: true,
        custody_verified: true,
        streaming_supported: true,
        quality_bps: Some(7_500),
        p95_inference_ms: Some(400),
        p95_cold_load_ms: Some(200),
        resident_memory_bytes: Some(500),
        working_memory_bytes: Some(200),
        resident_handle: Some(17),
        serving_ready: true,
    }
}

fn quality() -> ModelOffer<'static> {
    ModelOffer {
        model_id: "quality",
        backend: BackendKind::Gguf,
        task: InferenceTask::TextGeneration,
        target: ExecutionTarget::Cpu,
        evidence_qualified: true,
        custody_verified: true,
        streaming_supported: true,
        quality_bps: Some(9_100),
        p95_inference_ms: Some(250),
        p95_cold_load_ms: Some(120),
        resident_memory_bytes: Some(1_500),
        working_memory_bytes: Some(300),
        resident_handle: None,
        serving_ready: false,
    }
}

#[test]
fn quality_floor_first_then_lowest_total_cost_proposes_cold_switch() {
    let options = [light(), quality()];
    let result = plan_model_selection(&options, &policy());
    assert_eq!(
        result.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            expected_p95_ms: 370
        }
    );
    assert_eq!(result.assessments.len(), 2);
    assert!(result.assessments.iter().all(|a| a.refusal.is_none()));
}

#[test]
fn a_loaded_light_model_is_preferred_when_switch_cost_outweighs_gain() {
    let mut high = quality();
    high.p95_cold_load_ms = Some(210);
    let result = plan_model_selection(&[light(), high], &policy());
    assert_eq!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            handle_id: 17,
            expected_p95_ms: 400
        }
    );
}

#[test]
fn cooldown_stays_with_eligible_incumbent_even_if_other_model_is_faster() {
    let mut p = policy();
    p.cooldown_active = true;
    let result = plan_model_selection(&[light(), quality()], &p);
    assert!(matches!(
        result.action,
        ModelAction::UseResident { model_id: "light", .. }
    ));
}

#[test]
fn cooldown_never_serves_ineligible_incumbent_below_quality_floor() {
    let mut p = policy();
    p.cooldown_active = true;
    p.quality_floor_bps = 8_000;
    let result = plan_model_selection(&[light(), quality()], &p);
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::CooldownWithoutEligibleIncumbent)
    );
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::BelowQualityFloor)
    );
}

#[test]
fn no_switch_permission_does_not_authorize_a_cold_load() {
    let mut p = policy();
    p.allow_cold_switch = false;
    p.current_model_id = None;
    let result = plan_model_selection(&[quality()], &p);
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
    let result = plan_model_selection(&[light()], &p);
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::ResidentRerouteNotAuthorized)
    );
}

#[test]
fn authorized_resident_reroute_is_a_proposal_not_execution() {
    let mut p = policy();
    p.current_model_id = None;
    let result = plan_model_selection(&[light()], &p);
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
    let result = plan_model_selection(&[light(), quality()], &p);
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
    let result = plan_model_selection(&[light()], &p);
    assert_eq!(result.action, ModelAction::Refuse(PlanRefusal::InvalidPolicy));
}

#[test]
fn synthetic_compiled_or_untrusted_model_is_not_qualified() {
    let mut compiled = quality();
    compiled.evidence_qualified = false;
    let result = plan_model_selection(&[compiled], &policy());
    assert_eq!(result.action, ModelAction::Refuse(PlanRefusal::NoEligibleModel));
    assert_eq!(result.assessments[0].refusal, Some(CandidateRefusal::Unqualified));

    let mut untrusted = quality();
    untrusted.custody_verified = false;
    let result = plan_model_selection(&[untrusted], &policy());
    assert_eq!(result.assessments[0].refusal, Some(CandidateRefusal::UnverifiedCustody));
}

#[test]
fn no_implicit_task_backend_or_accelerator_substitution() {
    let mut onnx = quality();
    onnx.model_id = "onnx";
    onnx.backend = BackendKind::Onnx;
    onnx.task = InferenceTask::Embedding;
    onnx.streaming_supported = false;
    let result = plan_model_selection(&[onnx.clone()], &policy());
    assert_eq!(result.assessments[0].refusal, Some(CandidateRefusal::TaskMismatch));

    onnx.task = InferenceTask::TextGeneration;
    onnx.target = ExecutionTarget::Cuda;
    let result = plan_model_selection(&[onnx.clone()], &policy());
    assert_eq!(result.assessments[0].refusal, Some(CandidateRefusal::TargetMismatch));

    onnx.target = ExecutionTarget::Cpu;
    let result = plan_model_selection(&[onnx], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::StreamingUnavailable)
    );
}

#[test]
fn no_unknown_quality_or_memory_or_latency_is_fabricated() {
    let mut m = quality();
    m.quality_bps = None;
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.quality_bps = Some(10_001);
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.quality_bps = Some(9_100);
    m.resident_memory_bytes = None;
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.resident_memory_bytes = Some(u64::MAX);
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.resident_memory_bytes = Some(1_500);
    m.p95_cold_load_ms = None;
    assert_eq!(
        plan_model_selection(&[m], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn quality_memory_latency_and_overflow_are_hard_limits() {
    let mut m = quality();
    m.quality_bps = Some(6_999);
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::BelowQualityFloor)
    );
    m.quality_bps = Some(9_100);
    m.resident_memory_bytes = Some(2_900);
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::OverMemoryBudget)
    );
    m.resident_memory_bytes = Some(1_500);
    m.p95_inference_ms = Some(950);
    assert_eq!(
        plan_model_selection(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::OverLatencyBudget)
    );
    m.p95_inference_ms = Some(u64::MAX);
    assert_eq!(
        plan_model_selection(&[m], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn loaded_registration_without_readiness_is_not_eligible() {
    let mut m = light();
    m.serving_ready = false;
    let result = plan_model_selection(&[m], &policy());
    assert_eq!(result.assessments[0].refusal, Some(CandidateRefusal::NotReady));
    let mut m = quality();
    m.serving_ready = true;
    let result = plan_model_selection(&[m], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn invalid_or_stale_handle_is_not_usable() {
    let mut m = light();
    m.resident_handle = Some(0);
    let result = plan_model_selection(&[m], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn equal_cost_unknown_preference_refuses_ambiguity() {
    let mut a = light();
    a.resident_handle = None;
    a.serving_ready = false;
    a.p95_cold_load_ms = Some(0);
    a.p95_inference_ms = Some(370);
    let result = plan_model_selection(&[a, quality()], &policy());
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::AmbiguousBestCandidate)
    );
}

#[test]
fn equal_cost_prefers_existing_resident_to_avoid_thrashing() {
    let mut high = quality();
    high.p95_inference_ms = Some(280);
    let result = plan_model_selection(&[high, light()], &policy());
    assert!(matches!(
        result.action,
        ModelAction::UseResident { model_id: "light", .. }
    ));
}

#[test]
fn duplicate_ids_and_empty_allowlist_fail_closed() {
    let duplicate = plan_model_selection(&[quality(), quality()], &policy());
    assert_eq!(
        duplicate.action,
        ModelAction::Refuse(PlanRefusal::DuplicateModelId)
    );
    let mut p = policy();
    p.allowed_model_ids = &[];
    assert_eq!(
        plan_model_selection(&[light()], &p).action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
}

#[test]
fn explicit_host_limits_cannot_be_invalid_or_silently_clamped() {
    let mut p = policy();
    p.min_improvement_bps = 10_001;
    assert_eq!(
        plan_model_selection(&[light()], &p).action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
    p.min_improvement_bps = 1_000;
    p.max_model_memory_bytes = 0;
    assert_eq!(
        plan_model_selection(&[light()], &p).action,
        ModelAction::Refuse(PlanRefusal::InvalidPolicy)
    );
}
