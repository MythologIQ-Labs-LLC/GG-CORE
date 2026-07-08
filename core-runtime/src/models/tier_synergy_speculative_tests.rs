//! Tests for `TierSpeculativePlan::select`.
//!
//! Extracted from `tier_synergy_speculative.rs` for Section 4 compliance.

#![cfg(feature = "advanced")]

use super::*;
use crate::models::speculative_config::{AdaptiveMode, AdaptiveSpeculativeConfig};

fn enabled_cfg() -> AdaptiveSpeculativeConfig {
    AdaptiveSpeculativeConfig {
        enabled: true,
        mode: AdaptiveMode::Balanced,
        acceptance_floor: 0.60,
        ..Default::default()
    }
}

#[test]
fn light_quality_pairing_selected() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Light, ModelTier::Quality],
        None,
        HardwareProfile::SingleGpu,
        0.80,
        &enabled_cfg(),
    );
    assert!(plan.is_speculative);
    assert_eq!(plan.primary_tier, ModelTier::Quality);
    assert_eq!(plan.draft_tier, Some(ModelTier::Light));
    assert_eq!(plan.pairing, SynergyMode::SpeculativeLightQuality);
    assert!(plan.fallback_reason.is_none());
}

#[test]
fn light_balanced_pairing_selected() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Light, ModelTier::Balanced],
        None,
        HardwareProfile::SingleGpu,
        0.80,
        &enabled_cfg(),
    );
    assert!(plan.is_speculative);
    assert_eq!(plan.pairing, SynergyMode::SpeculativeLightBalanced);
    assert_eq!(plan.draft_tier, Some(ModelTier::Light));
}

#[test]
fn balanced_quality_pairing_selected_with_gpu() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Balanced, ModelTier::Quality],
        None,
        HardwareProfile::SingleGpu,
        0.80,
        &enabled_cfg(),
    );
    assert!(plan.is_speculative);
    assert_eq!(plan.pairing, SynergyMode::SpeculativeBalancedQuality);
    assert_eq!(plan.draft_tier, Some(ModelTier::Balanced));
}

#[test]
fn balanced_quality_falls_back_without_gpu() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Balanced, ModelTier::Quality],
        None,
        HardwareProfile::NoGpu,
        0.80,
        &enabled_cfg(),
    );
    assert!(!plan.is_speculative);
    assert_eq!(plan.pairing, SynergyMode::Single);
    assert!(plan.fallback_reason.as_deref().unwrap().contains("NoGpu"));
}

#[test]
fn disabled_config_falls_back() {
    let cfg = AdaptiveSpeculativeConfig::default(); // enabled=false
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Light, ModelTier::Quality],
        None,
        HardwareProfile::MultiGpu,
        0.90,
        &cfg,
    );
    assert!(!plan.is_speculative);
    assert!(plan
        .fallback_reason
        .as_deref()
        .unwrap()
        .contains("disabled"));
}

#[test]
fn low_acceptance_rate_forces_fallback() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Light, ModelTier::Quality],
        None,
        HardwareProfile::SingleGpu,
        0.40, // below acceptance_floor of 0.60
        &enabled_cfg(),
    );
    assert!(!plan.is_speculative);
    assert!(plan
        .fallback_reason
        .as_deref()
        .unwrap()
        .contains("acceptance"));
}

#[test]
fn single_tier_only_falls_back() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Quality],
        None,
        HardwareProfile::SingleGpu,
        0.80,
        &enabled_cfg(),
    );
    assert!(!plan.is_speculative);
    assert_eq!(plan.primary_tier, ModelTier::Quality);
}

#[test]
fn load_hint_respected_in_single_fallback() {
    let plan = TierSpeculativePlan::select(
        &[ModelTier::Light, ModelTier::Balanced, ModelTier::Quality],
        Some(ModelTier::Balanced),
        HardwareProfile::SingleGpu,
        0.40, // forces fallback
        &enabled_cfg(),
    );
    assert!(!plan.is_speculative);
    assert_eq!(plan.primary_tier, ModelTier::Balanced);
}
