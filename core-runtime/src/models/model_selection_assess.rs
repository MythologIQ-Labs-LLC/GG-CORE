//! Fail-closed per-model evidence and resource checks.

use super::{CandidateRefusal, ModelOffer, SelectionPolicy};
use crate::models::backend_capabilities::{
    RuntimeBackendCapabilities, ServingPath, CAPABILITY_SCHEMA_VERSION,
};

pub(super) fn assess(
    model: &ModelOffer<'_>,
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> Result<u64, CandidateRefusal> {
    check_authority_and_path(model, policy, compiled)?;
    check_resource_and_latency(model, policy)
}

fn check_authority_and_path(
    model: &ModelOffer<'_>,
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> Result<(), CandidateRefusal> {
    use CandidateRefusal::*;
    if !policy.allowed_model_ids.contains(&model.model_id) {
        return Err(NotAllowed);
    }
    if policy.pinned_model_id.is_some_and(|p| p != model.model_id) {
        return Err(NotPinned);
    }
    if !model.evidence_qualified {
        return Err(Unqualified);
    }
    if !model.custody_verified {
        return Err(UnverifiedCustody);
    }
    if model.task != policy.task {
        return Err(TaskMismatch);
    }
    if model.target != policy.target {
        return Err(TargetMismatch);
    }
    let path = ServingPath::new(model.task, model.target);
    let compiled_path = compiled.iter().find(|backend| {
        backend.schema_version == CAPABILITY_SCHEMA_VERSION
            && backend.backend == model.backend
            && backend.compiled_paths.contains(&path)
    });
    let Some(compiled_path) = compiled_path else {
        return Err(PathNotCompiled);
    };
    if policy.require_streaming
        && (!model.streaming_supported || !compiled_path.supports_streaming)
    {
        return Err(StreamingUnavailable);
    }
    Ok(())
}

fn check_resource_and_latency(
    model: &ModelOffer<'_>,
    policy: &SelectionPolicy<'_>,
) -> Result<u64, CandidateRefusal> {
    use CandidateRefusal::*;
    let quality = model
        .quality_bps
        .filter(|value| *value <= 10_000)
        .ok_or(MissingOrInvalidEvidence)?;
    if quality < policy.quality_floor_bps {
        return Err(BelowQualityFloor);
    }
    let memory_bytes = model
        .resident_memory_bytes
        .zip(model.working_memory_bytes)
        .and_then(|(resident, working)| resident.checked_add(working))
        .ok_or(MissingOrInvalidEvidence)?;
    if memory_bytes > policy.max_model_memory_bytes {
        return Err(OverMemoryBudget);
    }
    let cold_ms = cold_start_time(model)?;
    let p95_total = model
        .p95_inference_ms
        .and_then(|inference| inference.checked_add(cold_ms))
        .ok_or(MissingOrInvalidEvidence)?;
    if p95_total > policy.max_p95_total_ms {
        return Err(OverLatencyBudget);
    }
    Ok(p95_total)
}

fn cold_start_time(model: &ModelOffer<'_>) -> Result<u64, CandidateRefusal> {
    use CandidateRefusal::*;
    match model.resident_handle {
        Some(0) => Err(MissingOrInvalidEvidence),
        Some(_) if model.serving_ready => Ok(0),
        Some(_) => Err(NotReady),
        None if model.serving_ready => Err(MissingOrInvalidEvidence),
        None => model.p95_cold_load_ms.ok_or(MissingOrInvalidEvidence),
    }
}
