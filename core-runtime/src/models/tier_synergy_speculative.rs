//! Speculative execution plan selection for TierSynergy.
//!
//! Decoupled from `tier_synergy.rs` (which is already at the 250-line limit)
//! so that plan logic can evolve independently without scope creep.
//!
//! # Example
//!
//! ```rust,ignore
//! # #[cfg(feature = "advanced")]
//! # {
//! use gg_core::models::tier_synergy_speculative::{
//!     TierSpeculativePlan, HardwareProfile,
//! };
//! use gg_core::models::{AdaptiveSpeculativeConfig, SmartModelTier};
//!
//! let config = AdaptiveSpeculativeConfig { enabled: true, ..Default::default() };
//! let plan = TierSpeculativePlan::select(
//!     &[SmartModelTier::Light, SmartModelTier::Quality],
//!     None,
//!     HardwareProfile::SingleGpu,
//!     0.75,
//!     &config,
//! );
//! assert!(plan.is_speculative);
//! # }
//! ```

#![cfg(feature = "advanced")]

use crate::models::smart_loader::ModelTier;
use crate::models::speculative_config::AdaptiveSpeculativeConfig;
use crate::models::tier_synergy::SynergyMode;

// ── Hardware profile ──────────────────────────────────────────────────────────

/// Coarse hardware classification used to gate unsafe pairings.
///
/// `NoGpu` disallows Balanced→Quality pairings (too slow on CPU).
/// `SingleGpu` permits all three pairing types.
/// `MultiGpu` permits all pairings and may unlock future tensor-parallel paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareProfile {
    /// CPU-only host.
    NoGpu,
    /// Single discrete GPU.
    SingleGpu,
    /// Two or more discrete GPUs.
    MultiGpu,
}

// ── Compatibility check ───────────────────────────────────────────────────────

/// Tokenizer compatibility between a draft/target pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityCheck {
    /// Same tokenizer family — safe to pair.
    Compatible,
    /// Different tokenizer family — pairing would corrupt outputs.
    FamilyMismatch,
    /// Family unknown at plan time; caller must verify before use.
    Unknown,
}

// ── Plan ─────────────────────────────────────────────────────────────────────

/// A complete speculative execution plan.
///
/// Produced by [`TierSpeculativePlan::select`]. When `is_speculative` is
/// `false` the caller should use `primary_tier` alone and ignore
/// `draft_tier` / `pairing`.
#[derive(Debug, Clone)]
pub struct TierSpeculativePlan {
    /// The target (verification) model tier.
    pub primary_tier: ModelTier,
    /// Draft model tier, present only in speculative plans.
    pub draft_tier: Option<ModelTier>,
    /// The `SynergyMode` that matches this plan.
    pub pairing: SynergyMode,
    /// Tokenizer compatibility assessment.
    pub compatibility: CompatibilityCheck,
    /// Whether this plan uses speculative decoding.
    pub is_speculative: bool,
    /// Human-readable reason when the plan fell back to single-model.
    pub fallback_reason: Option<String>,
}

impl TierSpeculativePlan {
    /// Select the best speculative plan given runtime context.
    ///
    /// Priority order (highest first):
    /// 1. Light → Quality
    /// 2. Light → Balanced
    /// 3. Balanced → Quality
    /// 4. Single-model fallback
    ///
    /// Fallback is forced when:
    /// - `config.enabled` is `false`
    /// - No valid pairing exists in `available_tiers`
    /// - Hardware cannot support the pairing safely
    /// - Observed `acceptance_rate` is below `config.acceptance_floor`
    pub fn select(
        available_tiers: &[ModelTier],
        load_hint: Option<ModelTier>,
        hardware: HardwareProfile,
        acceptance_rate: f32,
        config: &AdaptiveSpeculativeConfig,
    ) -> Self {
        if !config.is_active() {
            return Self::single_fallback(
                best_single_tier(available_tiers, load_hint),
                "speculation disabled via config",
            );
        }

        if acceptance_rate < config.acceptance_floor {
            return Self::single_fallback(
                best_single_tier(available_tiers, load_hint),
                "acceptance rate below floor",
            );
        }

        let has = |t: ModelTier| available_tiers.contains(&t);

        if has(ModelTier::Light) && has(ModelTier::Quality) {
            return Self::speculative(
                ModelTier::Quality,
                ModelTier::Light,
                SynergyMode::SpeculativeLightQuality,
            );
        }

        if has(ModelTier::Light) && has(ModelTier::Balanced) {
            return Self::speculative(
                ModelTier::Balanced,
                ModelTier::Light,
                SynergyMode::SpeculativeLightBalanced,
            );
        }

        if has(ModelTier::Balanced) && has(ModelTier::Quality) && gpu_can_pair(hardware) {
            return Self::speculative(
                ModelTier::Quality,
                ModelTier::Balanced,
                SynergyMode::SpeculativeBalancedQuality,
            );
        }

        let reason = if has(ModelTier::Balanced) && has(ModelTier::Quality) {
            "Balanced→Quality requires GPU; NoGpu host"
        } else {
            "no compatible tier pairing available"
        };

        Self::single_fallback(best_single_tier(available_tiers, load_hint), reason)
    }

    // ── Private constructors ─────────────────────────────────────────────────

    fn speculative(primary: ModelTier, draft: ModelTier, pairing: SynergyMode) -> Self {
        Self {
            primary_tier: primary,
            draft_tier: Some(draft),
            pairing,
            compatibility: CompatibilityCheck::Unknown,
            is_speculative: true,
            fallback_reason: None,
        }
    }

    fn single_fallback(primary: ModelTier, reason: &str) -> Self {
        Self {
            primary_tier: primary,
            draft_tier: None,
            pairing: SynergyMode::Single,
            compatibility: CompatibilityCheck::Compatible,
            is_speculative: false,
            fallback_reason: Some(reason.to_owned()),
        }
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Returns `true` when the hardware can sustain a two-model pairing.
fn gpu_can_pair(hw: HardwareProfile) -> bool {
    matches!(hw, HardwareProfile::SingleGpu | HardwareProfile::MultiGpu)
}

/// Choose the best single tier from what is available.
///
/// Preference follows the load hint when provided; otherwise Quality >
/// Balanced > Light.
fn best_single_tier(available: &[ModelTier], hint: Option<ModelTier>) -> ModelTier {
    if let Some(h) = hint {
        if available.contains(&h) {
            return h;
        }
    }
    for &preferred in &[ModelTier::Quality, ModelTier::Balanced, ModelTier::Light] {
        if available.contains(&preferred) {
            return preferred;
        }
    }
    // Caller must have at least one tier; fall back gracefully.
    ModelTier::Light
}

#[cfg(test)]
#[path = "tier_synergy_speculative_tests.rs"]
mod tests;
