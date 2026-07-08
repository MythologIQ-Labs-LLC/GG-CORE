//! Synergy status snapshot.

use crate::engine::speculative_v2::SpeculativeConfig;
use crate::models::smart_loader::ModelTier;

use super::SynergyMode;

/// Current synergy status.
#[derive(Debug)]
pub struct SynergyStatus {
    pub mode: SynergyMode,
    /// [light, balanced, quality] availability
    pub available_tiers: Vec<bool>,
    pub loaded_tiers: Vec<ModelTier>,
    pub spec_config: SpeculativeConfig,
}
