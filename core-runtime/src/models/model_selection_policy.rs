//! Quality-floor-first model-level routing plan (#50 / #150).
//!
//! This is an evidence-bound *decision*, never an execution or certification.
//! The caller supplies trusted, locally verified offers and host constraints.
//! No network fallback, model mutation, inference or automatic cold swap occurs.
//! Real lifecycle admission, handle validation, draining and readiness remain
//! independently mandatory at execution time (see #137 and #153).
//!
//! Evidence cannot be minted by the caller of an authenticated IPC request:
//! a future controller must source it from its own reviewed, exact-artifact
//! model/benchmark records. These public structs are NOT a trust boundary.

use std::collections::HashSet;

use super::backend_capabilities::{
    compiled_backend_capabilities, BackendKind, ExecutionTarget, InferenceTask,
    RuntimeBackendCapabilities, ServingPath, CAPABILITY_SCHEMA_VERSION,
};

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
    pub max_p95_total_ms: u64,
    pub current_model_id: Option<&'a str>,
    pub allow_resident_reroute: bool,
    pub allow_cold_switch: bool,
    /// Must be obtained from trusted monotonic switch policy state.
    pub cooldown_active: bool,
    /// Minimum relative p95 gain to leave a suitable incumbent, 0..=10000.
    pub min_improvement_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    NotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanRefusal {
    InvalidPolicy,
    DuplicateModelId,
    NoEligibleModel,
    AmbiguousBestCandidate,
    CooldownWithoutEligibleIncumbent,
    ResidentRerouteNotAuthorized,
    ColdSwitchNotAuthorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelAction<'a> {
    /// Safe to propose using existing route; MUST revalidate handle at use.
    UseResident {
        model_id: &'a str,
        handle_id: u64,
        expected_p95_ms: u64,
    },
    /// No mutation: only an explicit authenticated host decision may reroute.
    ProposeResidentReroute {
        model_id: &'a str,
        handle_id: u64,
        expected_p95_ms: u64,
    },
    /// No mutation: only the canonical transition owner can unload/load.
    ProposeColdSwitch {
        model_id: &'a str,
        expected_p95_ms: u64,
    },
    Refuse(PlanRefusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateAssessment<'a> {
    pub model_id: &'a str,
    pub refusal: Option<CandidateRefusal>,
    pub expected_p95_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelectionPlan<'a> {
    pub action: ModelAction<'a>,
    pub assessments: Vec<CandidateAssessment<'a>>,
}

/// Plan a single request-boundary choice, using measured total cost of a useful
/// answer AFTER enforcing qualification, quality, custody, task and budgets.
/// This function has no state, IO or side effects. A real controller must
/// independently revalidate selected identity, policy and resources before IO.
pub fn plan_model_selection<'a>(
    offers: &'a [ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
) -> ModelSelectionPlan<'a> {
    let compiled = compiled_backend_capabilities();
    plan_with_compiled_catalog(offers, policy, &compiled)
}

/// Private seam for deterministic source tests. The runtime API always uses
/// its own compiled capabilities; the host/client cannot forge this catalog.
fn plan_with_compiled_catalog<'a>(
    offers: &'a [ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> ModelSelectionPlan<'a> {
    let refused = |reason| ModelSelectionPlan {
        action: ModelAction::Refuse(reason),
        assessments: Vec::new(),
    };
    if policy.allowed_model_ids.is_empty()
        || policy.allowed_model_ids.iter().any(|id| id.is_empty())
        || policy.quality_floor_bps > 10_000
        || policy.min_improvement_bps > 10_000
        || policy.max_model_memory_bytes == 0
        || policy.max_p95_total_ms == 0
        || policy
            .pinned_model_id
            .is_some_and(|id| !policy.allowed_model_ids.contains(&id))
    {
        return refused(PlanRefusal::InvalidPolicy);
    }
    let mut seen = HashSet::new();
    if offers
        .iter()
        .any(|offer| offer.model_id.is_empty() || !seen.insert(offer.model_id))
    {
        return refused(PlanRefusal::DuplicateModelId);
    }

    let mut assessments = Vec::with_capacity(offers.len());
    let mut viable = Vec::new();
    for (index, offer) in offers.iter().enumerate() {
        match assess(offer, policy, compiled) {
            Ok(p95) => {
                viable.push((index, p95));
                assessments.push(CandidateAssessment {
                    model_id: offer.model_id,
                    refusal: None,
                    expected_p95_ms: Some(p95),
                });
            }
            Err(reason) => assessments.push(CandidateAssessment {
                model_id: offer.model_id,
                refusal: Some(reason),
                expected_p95_ms: None,
            }),
        }
    }
    if viable.is_empty() {
        return ModelSelectionPlan {
            action: ModelAction::Refuse(PlanRefusal::NoEligibleModel),
            assessments,
        };
    }
    let incumbent = viable.iter().copied().find(|(i, _)| {
        policy.current_model_id == Some(offers[*i].model_id)
            && offers[*i].resident_handle.is_some()
    });
    let best_ms = viable.iter().map(|(_, p95)| *p95).min().unwrap_or(u64::MAX);
    let tied: Vec<_> = viable
        .iter()
        .copied()
        .filter(|(_, p95)| *p95 == best_ms)
        .collect();
    let best = if tied.len() == 1 {
        tied[0]
    } else if let Some(current) = incumbent.filter(|(_, cost)| *cost == best_ms) {
        current
    } else {
        return ModelSelectionPlan {
            action: ModelAction::Refuse(PlanRefusal::AmbiguousBestCandidate),
            assessments,
        };
    };
    // Never jump models solely for marginal modeled improvements.
    let chosen = match incumbent {
        Some(current) if current.0 != best.0 => {
            let savings = current.1.saturating_sub(best.1) as u128 * 10_000;
            let threshold = current.1 as u128 * u128::from(policy.min_improvement_bps);
            if policy.cooldown_active || savings < threshold {
                current
            } else {
                best
            }
        }
        _ => best,
    };
    // A switch/reroute disallowed by policy cannot displace a qualified,
    // ready incumbent. Its performance score is irrelevant without authority.
    let chosen = if let Some(current) = incumbent {
        let offer = &offers[chosen.0];
        if (offer.resident_handle.is_none() && !policy.allow_cold_switch)
            || (offer.model_id != offers[current.0].model_id
                && offer.resident_handle.is_some()
                && !policy.allow_resident_reroute)
        {
            current
        } else {
            chosen
        }
    } else {
        chosen
    };
    let (selected_index, expected_p95_ms) = chosen;
    let selected = &offers[selected_index];
    let action = if let Some(handle_id) = selected.resident_handle {
        if policy.current_model_id == Some(selected.model_id) {
            ModelAction::UseResident {
                model_id: selected.model_id,
                handle_id,
                expected_p95_ms,
            }
        } else if policy.cooldown_active && incumbent.is_none() {
            ModelAction::Refuse(PlanRefusal::CooldownWithoutEligibleIncumbent)
        } else if policy.allow_resident_reroute {
            ModelAction::ProposeResidentReroute {
                model_id: selected.model_id,
                handle_id,
                expected_p95_ms,
            }
        } else {
            ModelAction::Refuse(PlanRefusal::ResidentRerouteNotAuthorized)
        }
    } else if policy.cooldown_active && incumbent.is_none() {
        ModelAction::Refuse(PlanRefusal::CooldownWithoutEligibleIncumbent)
    } else if policy.allow_cold_switch {
        ModelAction::ProposeColdSwitch {
            model_id: selected.model_id,
            expected_p95_ms,
        }
    } else {
        ModelAction::Refuse(PlanRefusal::ColdSwitchNotAuthorized)
    };
    ModelSelectionPlan { action, assessments }
}

fn assess(
    model: &ModelOffer<'_>,
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> Result<u64, CandidateRefusal> {
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
    let path = ServingPath::new(model.task, model.target);
    if !compiled.iter().any(|backend| {
        backend.schema_version == CAPABILITY_SCHEMA_VERSION
            && backend.backend == model.backend
            && backend.compiled_paths.contains(&path)
    }) {
        return Err(PathNotCompiled);
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
    if policy.require_streaming && !model.streaming_supported {
        return Err(StreamingUnavailable);
    }
    let quality = model.quality_bps.filter(|v| *v <= 10_000).ok_or(MissingOrInvalidEvidence)?;
    if quality < policy.quality_floor_bps {
        return Err(BelowQualityFloor);
    }
    let bytes = model
        .resident_memory_bytes
        .zip(model.working_memory_bytes)
        .and_then(|(a, b)| a.checked_add(b))
        .ok_or(MissingOrInvalidEvidence)?;
    if bytes > policy.max_model_memory_bytes {
        return Err(OverMemoryBudget);
    }
    let load_ms = match model.resident_handle {
        Some(0) => return Err(MissingOrInvalidEvidence),
        Some(_) if model.serving_ready => 0,
        Some(_) => return Err(NotReady),
        None if model.serving_ready => return Err(MissingOrInvalidEvidence),
        None => model.p95_cold_load_ms.ok_or(MissingOrInvalidEvidence)?,
    };
    let cost = model
        .p95_inference_ms
        .and_then(|v| v.checked_add(load_ms))
        .ok_or(MissingOrInvalidEvidence)?;
    if cost > policy.max_p95_total_ms {
        return Err(OverLatencyBudget);
    }
    Ok(cost)
}

#[cfg(test)]
#[path = "model_selection_policy_tests.rs"]
mod tests;
