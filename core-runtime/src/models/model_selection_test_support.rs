//! Adversarial model selection cases. Every "qualified" offer below is
//! synthetic policy input, NOT a real-world model or platform qualification.

use super::super::backend_capabilities::{
    BackendKind, ExecutionTarget, InferenceTask, RuntimeBackendCapabilities, ServingPath,
    CAPABILITY_SCHEMA_VERSION,
};
use super::*;

const ALLOWED: &[&str] = &["light", "quality", "onnx"];

pub(super) fn choose<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
) -> ModelSelectionPlan<'a> {
    let compiled = [
        RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Gguf,
            model_formats: vec!["gguf".into()],
            compiled_paths: vec![ServingPath::new(
                InferenceTask::TextGeneration,
                ExecutionTarget::Cpu,
            )],
            supports_streaming: true,
            supports_kv_cache: true,
            qualified_paths: Vec::new(),
        },
        RuntimeBackendCapabilities {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            backend: BackendKind::Onnx,
            model_formats: vec!["onnx".into()],
            compiled_paths: vec![ServingPath::new(
                InferenceTask::Embedding,
                ExecutionTarget::Cpu,
            )],
            supports_streaming: false,
            supports_kv_cache: false,
            qualified_paths: Vec::new(),
        },
    ];
    plan_with_compiled_catalog(offers, policy, &compiled)
}

pub(super) fn policy() -> SelectionPolicy<'static> {
    SelectionPolicy {
        allowed_model_ids: ALLOWED,
        pinned_model_id: None,
        task: InferenceTask::TextGeneration,
        target: ExecutionTarget::Cpu,
        require_streaming: true,
        quality_floor_bps: 7_000,
        max_model_memory_bytes: 3_000,
        max_estimated_total_ms: 1_000,
        current_model_id: Some("light"),
        allow_resident_reroute: true,
        allow_cold_switch: true,
        allow_quality_escalation: true,
        cooldown_active: false,
        min_improvement_bps: 1_000,
    }
}

pub(super) fn light() -> ModelOffer<'static> {
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

pub(super) fn quality() -> ModelOffer<'static> {
    ModelOffer {
        model_id: "quality",
        backend: BackendKind::Gguf,
        task: InferenceTask::TextGeneration,
        target: ExecutionTarget::Cpu,
        evidence_qualified: true,
        custody_verified: true,
        streaming_supported: true,
        quality_bps: Some(9_100),
        p95_inference_ms: Some(220),
        p95_cold_load_ms: Some(120),
        resident_memory_bytes: Some(1_500),
        working_memory_bytes: Some(300),
        resident_handle: None,
        serving_ready: false,
    }
}
