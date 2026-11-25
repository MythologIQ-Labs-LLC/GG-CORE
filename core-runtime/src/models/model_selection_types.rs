//! Types for host-governed, evidence-bound model selection (#50).

use super::super::backend_capabilities::{BackendKind, ExecutionTarget, InferenceTask};

/// One observed model, measured on this task/hardware and verified locally.
/// Missing measurements exclude the model; nothing is guessed or inherited
/// from another model, machine, backend or accelerator.
#[derive(Debug, Clone)]
pub struct ModelOffer<'a> {
    pub model_id: &'a str,
    pub backend: BackendKind,
    pub task: InferenceTask,
    pub target: ExecutionTarget,
    /// Exact-artifact and installed serving-path qualification, not Cargo.
    pub evidence_qualified: bool,
    /// Model artifact/manifest custody was independently verified.
    pub custody_verified: bool,
    pub streaming_supported: bool,
    /// Higher is better, 0..=10000, measured for this task class.
    pub quality_bps: Option<u16>,
    /// Measured end-to-end p95 for the model when resident.
    pub p95_inference_ms: Option<u64>,
    /// Required to plan a nonresident cold transition.
    pub p95_cold_load_ms: Option<u64>,
    pub resident_memory_bytes: Option<u64>,
    pub working_memory_bytes: Option<u64>,
    /// Canonical lifecycle handle, not a borrowed route or cache entry.
    pub resident_handle: Option<u64>,
    /// Real inference/readiness probe. Registration alone is NOT readiness.
    pub serving_ready: bool,
}

/// All limits/preferences are host/operator decisions. No global quality
/// thresholds or unapproved quality/latency trade-offs are hardcoded.
#[derive(Debug, Clone)]
pub struct SelectionPolicy<'a> {
    pub allowed_model_ids: &'a [&'a str],
    pub pinned_model_id: Option<&'a str>,
    pub task: InferenceTask,
    pub target: ExecutionTarget,
    pub require_streaming: bool,
    pub quality_floor_bps: u16,
    /// Upper bound for one model's measured resident + working bytes.
    /// Does NOT replace global memory admission or transient peak checks.
    pub max_model_memory_bytes: u64,
    /// Upper bound on the sum of separately measured p95 components; this is
    /// NOT a measured percentile of end-to-end latency.
    pub max_estimated_total_ms: u64,
    pub current_model_id: Option<&'a str>,
    pub allow_resident_reroute: bool,
    pub allow_cold_switch: bool,
    /// Explicit consent for choosing a more capable model than the incumbent.
    pub allow_quality_escalation: bool,
    /// Must be obtained from trusted monotonic switch policy state.
    pub cooldown_active: bool,
    /// Minimum relative p95 gain to leave a suitable incumbent, 0..=10000.
    pub min_improvement_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateRefusal {
    NotAllowed,
    NotPinned,
    Unqualified,
    PathNotCompiled,
    UnverifiedCustody,
    TaskMismatch,
    TargetMismatch,
    StreamingUnavailable,
    MissingOrInvalidEvidence,
    BelowQualityFloor,
    OverMemoryBudget,
    OverLatencyBudget,
    HardwareTargetUnverified,
    HardwareHeadroomUnknown,
    OverHardwareHeadroom,
    NotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlanRefusal {
    InvalidPolicy,
    InvalidHardwareProfile,
    DuplicateModelId,
    NoEligibleModel,
    AmbiguousBestCandidate,
    CooldownWithoutEligibleIncumbent,
    ResidentRerouteNotAuthorized,
    ColdSwitchNotAuthorized,
    QualityEscalationNotAuthorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelAction<'a> {
    /// Safe to propose using existing route; MUST revalidate handle at use.
    UseResident {
        model_id: &'a str,
        handle_id: u64,
        estimated_total_ms: u64,
    },
    /// No mutation: only an explicit authenticated host decision may reroute.
    ProposeResidentReroute {
        model_id: &'a str,
        handle_id: u64,
        estimated_total_ms: u64,
    },
    /// No mutation: only the canonical transition owner can unload/load.
    ProposeColdSwitch {
        model_id: &'a str,
        estimated_total_ms: u64,
    },
    Refuse(PlanRefusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct CandidateAssessment<'a> {
    pub model_id: &'a str,
    pub refusal: Option<CandidateRefusal>,
    pub estimated_total_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ModelSelectionPlan<'a> {
    pub action: ModelAction<'a>,
    pub assessments: Vec<CandidateAssessment<'a>>,
}
