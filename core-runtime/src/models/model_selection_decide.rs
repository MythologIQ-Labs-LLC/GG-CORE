//! Deterministic quality-first model choice; no runtime mutation.

use std::collections::HashSet;

use super::{
    CandidateAssessment, ModelAction, ModelOffer, ModelSelectionPlan, PlanRefusal, SelectionPolicy,
};
use crate::models::backend_capabilities::RuntimeBackendCapabilities;

#[derive(Clone, Copy)]
struct Eligible {
    index: usize,
    cost_ms: u64,
}

pub(super) fn plan<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> ModelSelectionPlan<'a> {
    if let Some(reason) = invalid_policy_or_catalog(offers, policy) {
        return denied(reason);
    }
    let (assessments, viable) = evaluate(offers, policy, compiled);
    if viable.is_empty() {
        return ModelSelectionPlan {
            action: ModelAction::Refuse(PlanRefusal::NoEligibleModel),
            assessments,
        };
    }
    let incumbent = viable.iter().copied().find(|choice| {
        policy.current_model_id == Some(offers[choice.index].model_id)
            && offers[choice.index].resident_handle.is_some()
    });
    let winner = match best(&viable, incumbent) {
        Ok(candidate) => stabilize(offers, candidate, incumbent, policy),
        Err(reason) => {
            return ModelSelectionPlan {
                action: ModelAction::Refuse(reason),
                assessments,
            };
        }
    };
    ModelSelectionPlan {
        action: consented_action(offers, winner, incumbent, policy),
        assessments,
    }
}

fn denied(reason: PlanRefusal) -> ModelSelectionPlan<'static> {
    ModelSelectionPlan {
        action: ModelAction::Refuse(reason),
        assessments: Vec::new(),
    }
}

fn invalid_policy_or_catalog(
    offers: &[ModelOffer<'_>],
    policy: &SelectionPolicy<'_>,
) -> Option<PlanRefusal> {
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
        return Some(PlanRefusal::InvalidPolicy);
    }
    let mut seen = HashSet::new();
    if offers
        .iter()
        .any(|model| model.model_id.is_empty() || !seen.insert(model.model_id))
    {
        return Some(PlanRefusal::DuplicateModelId);
    }
    None
}

fn evaluate<'a>(
    offers: &[ModelOffer<'a>],
    policy: &SelectionPolicy<'_>,
    compiled: &[RuntimeBackendCapabilities],
) -> (Vec<CandidateAssessment<'a>>, Vec<Eligible>) {
    let mut assessments = Vec::with_capacity(offers.len());
    let mut eligible = Vec::new();
    for (index, offer) in offers.iter().enumerate() {
        let verdict = super::assessment::assess(offer, policy, compiled);
        if let Ok(cost_ms) = verdict {
            eligible.push(Eligible { index, cost_ms });
        }
        assessments.push(CandidateAssessment {
            model_id: offer.model_id,
            refusal: verdict.err(),
            expected_p95_ms: verdict.ok(),
        });
    }
    (assessments, eligible)
}

fn best(candidates: &[Eligible], incumbent: Option<Eligible>) -> Result<Eligible, PlanRefusal> {
    let minimum = candidates
        .iter()
        .map(|c| c.cost_ms)
        .min()
        .unwrap_or(u64::MAX);
    let mut tied = candidates.iter().copied().filter(|c| c.cost_ms == minimum);
    let first = tied.next().expect("nonempty candidates");
    if tied.next().is_none() {
        return Ok(first);
    }
    if let Some(current) = incumbent.filter(|c| c.cost_ms == minimum) {
        return Ok(current);
    }
    Err(PlanRefusal::AmbiguousBestCandidate)
}

fn stabilize(
    offers: &[ModelOffer<'_>],
    preferred: Eligible,
    incumbent: Option<Eligible>,
    policy: &SelectionPolicy<'_>,
) -> Eligible {
    let Some(current) = incumbent else {
        return preferred;
    };
    if current.index == preferred.index {
        return current;
    }
    let savings = u128::from(current.cost_ms.saturating_sub(preferred.cost_ms)) * 10_000;
    let required = u128::from(current.cost_ms) * u128::from(policy.min_improvement_bps);
    let chosen = if policy.cooldown_active || savings < required {
        current
    } else {
        preferred
    };
    let next = &offers[chosen.index];
    if (next.resident_handle.is_none() && !policy.allow_cold_switch)
        || (next.model_id != offers[current.index].model_id
            && next.resident_handle.is_some()
            && !policy.allow_resident_reroute)
    {
        current
    } else {
        chosen
    }
}

fn action_for<'a>(
    selected: &ModelOffer<'a>,
    expected_p95_ms: u64,
    incumbent: Option<Eligible>,
    policy: &SelectionPolicy<'_>,
) -> ModelAction<'a> {
    if policy.current_model_id == Some(selected.model_id) {
        if let Some(handle_id) = selected.resident_handle {
            return ModelAction::UseResident {
                model_id: selected.model_id,
                handle_id,
                expected_p95_ms,
            };
        }
    }
    if policy.cooldown_active && incumbent.is_none() {
        return ModelAction::Refuse(PlanRefusal::CooldownWithoutEligibleIncumbent);
    }
    match selected.resident_handle {
        Some(handle_id) if policy.allow_resident_reroute => ModelAction::ProposeResidentReroute {
            model_id: selected.model_id,
            handle_id,
            expected_p95_ms,
        },
        Some(_) => ModelAction::Refuse(PlanRefusal::ResidentRerouteNotAuthorized),
        None if policy.allow_cold_switch => ModelAction::ProposeColdSwitch {
            model_id: selected.model_id,
            expected_p95_ms,
        },
        None => ModelAction::Refuse(PlanRefusal::ColdSwitchNotAuthorized),
    }
}

fn consented_action<'a>(
    offers: &[ModelOffer<'a>],
    winner: Eligible,
    incumbent: Option<Eligible>,
    policy: &SelectionPolicy<'_>,
) -> ModelAction<'a> {
    if requires_quality_escalation_approval(offers, winner, policy) {
        return match incumbent {
            Some(current) => action_for(
                &offers[current.index],
                current.cost_ms,
                incumbent,
                policy,
            ),
            None => ModelAction::Refuse(PlanRefusal::QualityEscalationNotAuthorized),
        };
    }
    action_for(&offers[winner.index], winner.cost_ms, incumbent, policy)
}

/// Missing or inconsistent incumbent quality data cannot silently grant
/// permission to escalate a request to another model.
fn requires_quality_escalation_approval(
    offers: &[ModelOffer<'_>],
    winner: Eligible,
    policy: &SelectionPolicy<'_>,
) -> bool {
    if policy.allow_quality_escalation || policy.pinned_model_id.is_some() {
        return false;
    }
    let Some(current_id) = policy.current_model_id else {
        return false;
    };
    let next = &offers[winner.index];
    if next.model_id == current_id {
        return false;
    }
    let previous_score = offers
        .iter()
        .find(|model| model.model_id == current_id)
        .and_then(|model| model.quality_bps)
        .filter(|score| *score <= 10_000);
    match (previous_score, next.quality_bps) {
        (Some(before), Some(after)) if after <= before => false,
        _ => true,
    }
}
