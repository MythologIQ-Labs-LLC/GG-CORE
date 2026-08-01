//! Adaptive speculative decoding configuration.
//!
//! Disabled by default. Enable explicitly with `AdaptiveSpeculativeConfig { enabled: true, .. }`.

use serde::{Deserialize, Serialize};

/// Speculative decoding aggressiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveMode {
    /// Speculation disabled; always single-model decode.
    #[default]
    Disabled,
    /// Small windows, high acceptance floor — prioritizes correctness.
    Conservative,
    /// Balanced window sizing and acceptance thresholds.
    Balanced,
    /// Large windows, lower acceptance floor — prioritizes throughput.
    Aggressive,
}

/// Adaptive speculative decoding configuration.
///
/// Safe defaults: speculation is OFF. Enable with `enabled: true` and select a mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdaptiveSpeculativeConfig {
    /// Master enable switch. When false, all speculation is bypassed.
    pub enabled: bool,
    /// Aggressiveness mode (ignored when enabled=false).
    pub mode: AdaptiveMode,
    /// Max draft tokens per speculation step.
    pub max_draft_tokens: usize,
    /// Minimum verification window size (tokens).
    pub min_verification_tokens: usize,
    /// Maximum verification window size (tokens).
    pub max_verification_tokens: usize,
    /// Minimum confidence score to open a speculation window (0.0–1.0).
    pub confidence_floor: f32,
    /// Minimum token acceptance rate to continue speculation (0.0–1.0).
    pub acceptance_floor: f32,
    /// Automatically disable speculation when net speedup drops below threshold.
    pub auto_disable: bool,
    /// Net speedup ratio below which auto-disable triggers (e.g. 1.05 = 5% gain required).
    pub auto_disable_threshold: f32,
    /// Emit speculative decoding counters through the telemetry subsystem.
    pub telemetry_enabled: bool,
    /// Record per-step draft/verify cost for profiling (higher overhead).
    pub cost_profiling: bool,
    /// Use tier metadata (Light/Balanced/Quality) to restrict eligible pairings.
    pub tier_aware: bool,
    /// Trailing n-gram length for the model-free prompt-lookup draft (B-21f), used
    /// when no model draft pair is registered.
    pub prompt_lookup_ngram: usize,
}

impl Default for AdaptiveSpeculativeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: AdaptiveMode::Disabled,
            max_draft_tokens: 4,
            min_verification_tokens: 1,
            max_verification_tokens: 8,
            confidence_floor: 0.70,
            acceptance_floor: 0.60,
            auto_disable: true,
            auto_disable_threshold: 1.05,
            telemetry_enabled: true,
            cost_profiling: false,
            tier_aware: true,
            prompt_lookup_ngram: 3,
        }
    }
}

impl AdaptiveSpeculativeConfig {
    /// Returns true when speculation should run.
    pub fn is_active(&self) -> bool {
        self.enabled && self.mode != AdaptiveMode::Disabled
    }

    /// Clamp a requested draft token count to configured bounds.
    pub fn clamp_draft_tokens(&self, requested: usize) -> usize {
        requested.min(self.max_draft_tokens).max(1)
    }

    /// Clamp a requested verification window to configured bounds.
    pub fn clamp_verification_window(&self, requested: usize) -> usize {
        requested
            .max(self.min_verification_tokens)
            .min(self.max_verification_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_speculation_off() {
        let cfg = AdaptiveSpeculativeConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.mode, AdaptiveMode::Disabled);
        assert!(!cfg.is_active());
    }

    #[test]
    fn enabled_conservative_is_active() {
        let cfg = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Conservative,
            ..Default::default()
        };
        assert!(cfg.is_active());
    }

    #[test]
    fn enabled_disabled_mode_not_active() {
        let cfg = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Disabled,
            ..Default::default()
        };
        assert!(!cfg.is_active());
    }

    #[test]
    fn serde_roundtrip_default() {
        let cfg = AdaptiveSpeculativeConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AdaptiveSpeculativeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.enabled, cfg.enabled);
        assert_eq!(back.mode, cfg.mode);
        assert_eq!(back.max_draft_tokens, cfg.max_draft_tokens);
    }

    #[test]
    fn serde_roundtrip_enabled_balanced() {
        let cfg = AdaptiveSpeculativeConfig {
            enabled: true,
            mode: AdaptiveMode::Balanced,
            max_draft_tokens: 6,
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AdaptiveSpeculativeConfig = serde_json::from_str(&json).unwrap();
        assert!(back.enabled);
        assert_eq!(back.mode, AdaptiveMode::Balanced);
        assert_eq!(back.max_draft_tokens, 6);
    }

    #[test]
    fn clamp_draft_tokens_respects_max() {
        let cfg = AdaptiveSpeculativeConfig {
            max_draft_tokens: 4,
            ..Default::default()
        };
        assert_eq!(cfg.clamp_draft_tokens(10), 4);
        assert_eq!(cfg.clamp_draft_tokens(2), 2);
        assert_eq!(cfg.clamp_draft_tokens(0), 1);
    }

    #[test]
    fn clamp_verification_window_respects_bounds() {
        let cfg = AdaptiveSpeculativeConfig {
            min_verification_tokens: 2,
            max_verification_tokens: 6,
            ..Default::default()
        };
        assert_eq!(cfg.clamp_verification_window(1), 2);
        assert_eq!(cfg.clamp_verification_window(4), 4);
        assert_eq!(cfg.clamp_verification_window(10), 6);
    }
}
