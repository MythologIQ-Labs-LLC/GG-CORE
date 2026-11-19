//! Shortlist only transitions already authorized by host policy.
//! Ineligible or unconsented models must never hide an eligible alternative.

use super::{requires_quality_escalation_approval, Eligible};
use crate::models::model_selection_policy::{ModelOffer, SelectionPolicy};

pub(super) fn authorized_choices(
    offers: &[ModelOffer<'_>],
    viable: &[Eligible],
    policy: &SelectionPolicy<'_>,
) -> Vec<Eligible> {
    viable
        .iter()
        .copied()
        .filter(|candidate| {
            if requires_quality_escalation_approval(offers, *candidate, policy) {
                return false;
            }
            let offer = &offers[candidate.index];
            if policy.current_model_id == Some(offer.model_id) && offer.resident_handle.is_some() {
                return true;
            }
            if offer.resident_handle.is_some() {
                policy.allow_resident_reroute
            } else {
                policy.allow_cold_switch
            }
        })
        .collect()
}
