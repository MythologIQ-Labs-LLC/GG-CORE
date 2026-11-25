//! Model-free hardware admission negative controls for advisory policy #50.
//! All model costs and non-CPU capability paths below are synthetic fixtures.

use super::*;
use crate::models::backend_capabilities::{
    BackendKind, ExecutionTarget, InferenceTask, RuntimeBackendCapabilities, ServingPath,
    CAPABILITY_SCHEMA_VERSION,
};
use crate::models::hardware_profile::{
    HardwareProfile, TargetAvailability, HARDWARE_PROFILE_SCHEMA_VERSION,
};

fn cpu_catalog() -> Vec<RuntimeBackendCapabilities> {
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

fn choose<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    hardware: &HardwareProfile,
) -> ModelSelectionPlan<'a> {
    plan_with_compiled_catalog_and_hardware(offers, policy, &cpu_catalog(), hardware)
}

#[test]
fn detection_never_fabricates_gpu_or_free_memory() {
    let detected = HardwareProfile::detect_local();
    assert!(detected.is_valid());
    assert_eq!(detected.schema_version, HARDWARE_PROFILE_SCHEMA_VERSION);
    assert_eq!(detected.cpu, TargetAvailability::VerifiedAvailable);
    assert_eq!(detected.cuda, TargetAvailability::Unknown);
    assert_eq!(detected.metal, TargetAvailability::Unknown);
    assert_eq!(detected.unreserved_ram_bytes, None);
    assert_eq!(detected.unreserved_vram_bytes, None);
    assert!(detected.available_cpu_threads.is_none_or(|n| n > 0));
    let json = serde_json::to_string(&detected).expect("serialize profile");
    assert!(json.contains(r#""cuda":"unknown""#));
    assert!(json.contains(r#""unreserved_vram_bytes":null"#));
}

#[test]
fn resident_cpu_model_can_be_proposed_without_inventing_memory_headroom() {
    let result = choose(
        &[test_support::light()],
        &test_support::policy(),
        &HardwareProfile::detect_local(),
    );
    assert!(matches!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
    assert_eq!(result.assessments[0].refusal, None);
}

#[test]
fn unmeasured_cold_headroom_is_not_permission_to_load() {
    let result = choose(
        &[test_support::quality()],
        &test_support::policy(),
        &HardwareProfile::detect_local(),
    );
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::NoEligibleModel)
    );
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::HardwareHeadroomUnknown)
    );
}

#[test]
fn measured_headroom_refuses_unfit_cold_model_and_admits_fit_candidate() {
    let mut hardware = HardwareProfile::detect_local();
    hardware.unreserved_ram_bytes = Some(1_700);
    let offer = test_support::quality(); // 1500 resident + 300 working
    let result = choose(&[offer.clone()], &test_support::policy(), &hardware);
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::OverHardwareHeadroom)
    );
    hardware.unreserved_ram_bytes = Some(1_800);
    let result = choose(&[offer], &test_support::policy(), &hardware);
    assert!(matches!(
        result.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            ..
        }
    ));
}

#[test]
fn zero_new_headroom_does_not_double_charge_a_ready_resident() {
    let mut hardware = HardwareProfile::detect_local();
    hardware.unreserved_ram_bytes = Some(0);
    let result = choose(&[test_support::light()], &test_support::policy(), &hardware);
    assert!(matches!(result.action, ModelAction::UseResident { .. }));
}

#[test]
fn unverifiable_gpu_is_refused_even_when_synthetic_backend_supports_it() {
    let mut model = test_support::light();
    model.target = ExecutionTarget::Cuda;
    let mut policy = test_support::policy();
    policy.target = ExecutionTarget::Cuda;
    let catalog = [RuntimeBackendCapabilities {
        schema_version: CAPABILITY_SCHEMA_VERSION,
        backend: BackendKind::Gguf,
        model_formats: vec!["gguf".into()],
        compiled_paths: vec![ServingPath::new(
            InferenceTask::TextGeneration,
            ExecutionTarget::Cuda,
        )],
        supports_streaming: true,
        supports_kv_cache: true,
        qualified_paths: vec![],
    }];
    let mut hardware = HardwareProfile::detect_local();
    let result =
        plan_with_compiled_catalog_and_hardware(&[model.clone()], &policy, &catalog, &hardware);
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::HardwareTargetUnverified)
    );
    hardware.cuda = TargetAvailability::VerifiedUnavailable;
    let result =
        plan_with_compiled_catalog_and_hardware(&[model.clone()], &policy, &catalog, &hardware);
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::HardwareTargetUnverified)
    );
    // Only synthetic evidence for a tested path; not a production GPU claim.
    hardware.cuda = TargetAvailability::VerifiedAvailable;
    let result = plan_with_compiled_catalog_and_hardware(&[model], &policy, &catalog, &hardware);
    assert!(matches!(result.action, ModelAction::UseResident { .. }));
}

#[test]
fn invalid_profile_schema_or_thread_count_refuses_every_action() {
    for modify in [0u8, 1u8] {
        let mut hardware = HardwareProfile::detect_local();
        if modify == 0 {
            hardware.schema_version += 1;
        } else {
            hardware.available_cpu_threads = Some(0);
        }
        let result = choose(&[test_support::light()], &test_support::policy(), &hardware);
        assert_eq!(
            result.action,
            ModelAction::Refuse(PlanRefusal::InvalidHardwareProfile)
        );
        assert!(result.assessments.is_empty());
    }
}

#[test]
fn hardware_rejection_cannot_hide_another_permitted_resident() {
    let mut hardware = HardwareProfile::detect_local();
    hardware.unreserved_ram_bytes = Some(0);
    let offers = [test_support::quality(), test_support::light()];
    let result = choose(&offers, &test_support::policy(), &hardware);
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::OverHardwareHeadroom)
    );
    assert!(matches!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
}
