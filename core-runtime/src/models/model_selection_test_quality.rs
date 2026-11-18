//! Adversarial model selection source tests. Synthetic offers only.

use super::test_support::{choose, light, policy, quality};
use super::*;

#[test]
fn quality_floor_first_then_lowest_total_cost_proposes_cold_switch() {
    let options = [light(), quality()];
    let result = choose(&options, &policy());
    assert_eq!(
        result.action,
        ModelAction::ProposeColdSwitch {
            model_id: "quality",
            expected_p95_ms: 340
        }
    );
    assert_eq!(result.assessments.len(), 2);
    assert!(result.assessments.iter().all(|a| a.refusal.is_none()));
}

#[test]
fn a_loaded_light_model_is_preferred_when_switch_cost_outweighs_gain() {
    let mut high = quality();
    high.p95_cold_load_ms = Some(210);
    let result = choose(&[light(), high], &policy());
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
    let result = choose(&[light(), quality()], &p);
    assert!(matches!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
}

#[test]
fn cooldown_never_serves_ineligible_incumbent_below_quality_floor() {
    let mut p = policy();
    p.cooldown_active = true;
    p.quality_floor_bps = 8_000;
    let result = choose(&[light(), quality()], &p);
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
fn equal_cost_unknown_preference_refuses_ambiguity() {
    let mut a = light();
    a.resident_handle = None;
    a.serving_ready = false;
    a.p95_cold_load_ms = Some(0);
    a.p95_inference_ms = Some(340);
    let result = choose(&[a, quality()], &policy());
    assert_eq!(
        result.action,
        ModelAction::Refuse(PlanRefusal::AmbiguousBestCandidate)
    );
}

#[test]
fn equal_cost_prefers_existing_resident_to_avoid_thrashing() {
    let mut high = quality();
    high.p95_inference_ms = Some(280);
    let result = choose(&[high, light()], &policy());
    assert!(matches!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
}

#[test]
fn no_switch_authority_prefers_an_eligible_incumbent_over_a_better_cold_option() {
    let mut p = policy();
    p.allow_cold_switch = false;
    let result = choose(&[light(), quality()], &p);
    assert!(matches!(
        result.action,
        ModelAction::UseResident {
            model_id: "light",
            ..
        }
    ));
}
