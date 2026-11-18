//! Adversarial model selection source tests. Synthetic offers only.

use super::test_support::{choose, light, policy, quality};
use super::*;
use crate::models::backend_capabilities::{BackendKind, ExecutionTarget, InferenceTask};

#[test]
fn synthetic_compiled_or_untrusted_model_is_not_qualified() {
    let mut compiled = quality();
    compiled.evidence_qualified = false;
    let result = choose(&[compiled], &policy());
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::NoEligibleModel)
    );
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::Unqualified)
    );

    let mut untrusted = quality();
    untrusted.custody_verified = false;
    let result = choose(&[untrusted], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::UnverifiedCustody)
    );
}

#[test]
fn no_implicit_task_backend_or_accelerator_substitution() {
    let mut onnx = quality();
    onnx.model_id = "onnx";
    onnx.backend = BackendKind::Onnx;
    onnx.task = InferenceTask::Embedding;
    onnx.streaming_supported = false;
    let result = choose(&[onnx.clone()], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::TaskMismatch)
    );

    onnx.task = InferenceTask::TextGeneration;
    onnx.target = ExecutionTarget::Cuda;
    let result = choose(&[onnx.clone()], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::TargetMismatch)
    );

    onnx.target = ExecutionTarget::Cpu;
    let result = choose(&[onnx], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::PathNotCompiled)
    );
    let mut gguf = quality();
    gguf.streaming_supported = false;
    let result = choose(&[gguf], &policy());
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
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.quality_bps = Some(10_001);
    assert_eq!(
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.quality_bps = Some(9_100);
    m.resident_memory_bytes = None;
    assert_eq!(
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.resident_memory_bytes = Some(u64::MAX);
    assert_eq!(
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
    m.resident_memory_bytes = Some(1_500);
    m.p95_cold_load_ms = None;
    assert_eq!(
        choose(&[m], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn quality_memory_latency_and_overflow_are_hard_limits() {
    let mut m = quality();
    m.quality_bps = Some(6_999);
    assert_eq!(
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::BelowQualityFloor)
    );
    m.quality_bps = Some(9_100);
    m.resident_memory_bytes = Some(2_900);
    assert_eq!(
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::OverMemoryBudget)
    );
    m.resident_memory_bytes = Some(1_500);
    m.p95_inference_ms = Some(950);
    assert_eq!(
        choose(&[m.clone()], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::OverLatencyBudget)
    );
    m.p95_inference_ms = Some(u64::MAX);
    assert_eq!(
        choose(&[m], &policy()).assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn loaded_registration_without_readiness_is_not_eligible() {
    let mut m = light();
    m.serving_ready = false;
    let result = choose(&[m], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::NotReady)
    );
    let mut m = quality();
    m.serving_ready = true;
    let result = choose(&[m], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn invalid_or_stale_handle_is_not_usable() {
    let mut m = light();
    m.resident_handle = Some(0);
    let result = choose(&[m], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::MissingOrInvalidEvidence)
    );
}

#[test]
fn model_backend_must_have_an_actual_compiled_serving_path() {
    let mut m = quality();
    m.model_id = "onnx";
    m.backend = BackendKind::Onnx;
    let result = choose(&[m], &policy());
    assert_eq!(
        result.assessments[0].refusal,
        Some(CandidateRefusal::PathNotCompiled)
    );
}
