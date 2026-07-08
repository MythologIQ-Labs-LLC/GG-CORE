//! Synergy mode and result types.

use crate::models::registry::ModelHandle;

/// Synergy mode for tiered model usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynergyMode {
    /// Use single model (no speculation)
    Single,
    /// Speculative decoding with Light as draft, Quality as target
    SpeculativeLightQuality,
    /// Speculative decoding with Light as draft, Balanced as target
    SpeculativeLightBalanced,
    /// Speculative decoding with Balanced as draft, Quality as target
    SpeculativeBalancedQuality,
}

/// Result of a synergy-aware model request.
#[derive(Debug)]
pub struct SynergyResult {
    /// Primary model handle
    pub primary_handle: ModelHandle,
    /// Draft model handle (if speculative mode)
    pub draft_handle: Option<ModelHandle>,
    /// Active synergy mode
    pub mode: SynergyMode,
    /// Whether draft model is already loaded (zero additional latency)
    pub draft_ready: bool,
}
