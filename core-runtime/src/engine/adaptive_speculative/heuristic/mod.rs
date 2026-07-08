//! Heuristic implementations of [`ConfidenceEstimator`] and [`VerificationScheduler`].
//!
//! # Implementations
//!
//! - [`HeuristicConfidenceEstimator`] – signal-fusion estimator that combines
//!   draft log-probabilities, intra-block entropy spread, a temperature hint,
//!   repetition-penalty hint, and a historical acceptance rate.
//! - [`AdaptiveVerificationScheduler`] – mode-aware scheduler that selects a
//!   verification window within configured bounds and signals auto-disable when
//!   the rolling speedup estimate drops below the configured threshold.

#![cfg(feature = "advanced")]

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::engine::adaptive_speculative::{
    ConfidenceEstimator, DraftBlock, SurvivalProfile, VerificationPlan, VerificationScheduler,
};
use crate::models::speculative_config::{AdaptiveMode, AdaptiveSpeculativeConfig};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Rolling window length for acceptance-rate history.
pub(super) const HISTORY_LEN: usize = 32;

/// Log-prob below which a token is treated as near-certain.
const LOG_PROB_SATURATE: f32 = -0.1;

/// Log-prob at or below which a token is treated as rejected.
const LOG_PROB_FLOOR: f32 = -10.0;

// ── HeuristicConfidenceEstimator ──────────────────────────────────────────────

/// Signal-fusion confidence estimator that works without GPU or learned heads.
///
/// # Signals (all weighted equally in v1)
///
/// | Signal | Source |
/// |--------|--------|
/// | `log_prob` | `DraftBlock::log_probs` per token |
/// | entropy spread | std-dev of `log_probs` across the block |
/// | temperature | constructor hint (lower → higher confidence) |
/// | repetition penalty | constructor hint (higher → lower confidence) |
/// | historical acceptance | rolling mean from prior steps |
///
/// Low-confidence tail tokens receive *lower* scores but the window is never
/// widened to compensate — that is the scheduler's responsibility.
#[derive(Debug)]
pub struct HeuristicConfidenceEstimator {
    temperature_hint: f32,
    repetition_penalty_hint: f32,
    history: AcceptanceHistory,
}

impl HeuristicConfidenceEstimator {
    /// Create an estimator with explicit hints.
    ///
    /// Both hints default to `1.0` (neutral) when set to `1.0`,
    /// contributing no bias to the confidence score.
    pub fn new(temperature_hint: f32, repetition_penalty_hint: f32) -> Self {
        Self {
            temperature_hint: temperature_hint.max(0.01),
            repetition_penalty_hint: repetition_penalty_hint.max(0.01),
            history: AcceptanceHistory::new(),
        }
    }

    /// Neutral estimator (temperature = 1.0, repetition penalty = 1.0).
    pub fn neutral() -> Self {
        Self::new(1.0, 1.0)
    }

    /// Record an observed acceptance fraction in `[0.0, 1.0]` for future steps.
    pub fn record_acceptance(&self, fraction: f32) {
        self.history.push(fraction);
    }

    fn entropy_bias(log_probs: &[f32]) -> f32 {
        if log_probs.len() < 2 {
            return 0.0;
        }
        let valid: Vec<f32> = log_probs
            .iter()
            .copied()
            .filter(|p| p.is_finite())
            .collect();
        if valid.is_empty() {
            return 0.0;
        }
        let mean = valid.iter().copied().sum::<f32>() / valid.len() as f32;
        let var = valid.iter().map(|p| (p - mean).powi(2)).sum::<f32>() / valid.len() as f32;
        -(var.sqrt() / 5.0).min(0.3)
    }

    fn temperature_bias(temperature: f32) -> f32 {
        -(temperature - 1.0).clamp(0.0, 1.0) * 0.2
    }

    fn repetition_bias(penalty: f32) -> f32 {
        -(penalty - 1.0).clamp(0.0, 4.0) * 0.05
    }

    fn token_base_score(log_prob: f32) -> f32 {
        if log_prob >= LOG_PROB_SATURATE {
            return 1.0;
        }
        if log_prob <= LOG_PROB_FLOOR {
            return 0.0;
        }
        (log_prob - LOG_PROB_FLOOR) / (LOG_PROB_SATURATE - LOG_PROB_FLOOR)
    }
}

impl ConfidenceEstimator for HeuristicConfidenceEstimator {
    fn estimate(&self, _context: &[u32], draft: &DraftBlock) -> SurvivalProfile {
        if draft.tokens.is_empty() {
            return SurvivalProfile { scores: vec![] };
        }
        let shared_bias = (Self::entropy_bias(&draft.log_probs)
            + Self::temperature_bias(self.temperature_hint)
            + Self::repetition_bias(self.repetition_penalty_hint)
            + (self.history.mean() - 0.5) * 0.2)
            .clamp(-0.5, 0.5);

        let scores = draft
            .log_probs
            .iter()
            .map(|&lp| (Self::token_base_score(lp) + shared_bias).clamp(0.0, 1.0))
            .collect();

        SurvivalProfile { scores }
    }
}

// ── AdaptiveVerificationScheduler ─────────────────────────────────────────────

/// Mode-aware scheduler that adapts the verification window to observed speedup.
///
/// Window selection: mean profile score scaled by a per-mode multiplier, then
/// clamped to `[min_verification_tokens, max_verification_tokens]`.
///
/// Auto-disable: when the rolling speedup estimate drops below
/// `auto_disable_threshold`, [`VerificationPlan::fallback`] is returned and
/// [`AdaptiveVerificationScheduler::auto_disable_fired`] returns `true`.
#[derive(Debug)]
pub struct AdaptiveVerificationScheduler {
    config: AdaptiveSpeculativeConfig,
    history: AcceptanceHistory,
    auto_disable_fired: Mutex<bool>,
}

impl AdaptiveVerificationScheduler {
    /// Create a scheduler from a config snapshot.
    pub fn new(config: AdaptiveSpeculativeConfig) -> Self {
        Self {
            config,
            history: AcceptanceHistory::new(),
            auto_disable_fired: Mutex::new(false),
        }
    }

    /// Record the result of the last verification step.
    ///
    /// Call this after every `verify` call.  `accepted` / `window` is
    /// stored as the acceptance fraction for that step.
    pub fn record_result(&self, accepted: usize, window: usize) {
        let fraction = if window == 0 {
            0.0
        } else {
            accepted as f32 / window as f32
        };
        self.history.push(fraction);
    }

    /// Returns `true` when auto-disable has fired for this scheduler instance.
    pub fn auto_disable_fired(&self) -> bool {
        *self
            .auto_disable_fired
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn mode_multiplier(mode: AdaptiveMode) -> f32 {
        match mode {
            AdaptiveMode::Conservative => 0.6,
            AdaptiveMode::Balanced => 1.0,
            AdaptiveMode::Aggressive => 1.4,
            AdaptiveMode::Disabled => 0.0,
        }
    }

    fn mean_score(profile: &SurvivalProfile) -> f32 {
        if profile.scores.is_empty() {
            return 0.0;
        }
        profile.scores.iter().copied().sum::<f32>() / profile.scores.len() as f32
    }

    fn compute_window(&self, draft_len: usize, profile: &SurvivalProfile) -> usize {
        let raw = (draft_len as f32
            * Self::mean_score(profile)
            * Self::mode_multiplier(self.config.mode))
        .round() as usize;
        self.config.clamp_verification_window(raw)
    }

    fn should_auto_disable(&self) -> bool {
        self.config.auto_disable && (1.0 + self.history.mean()) < self.config.auto_disable_threshold
    }
}

impl VerificationScheduler for AdaptiveVerificationScheduler {
    fn plan(&self, draft: &DraftBlock, profile: &SurvivalProfile) -> VerificationPlan {
        if !self.config.is_active() {
            return VerificationPlan::fallback();
        }
        if self.should_auto_disable() {
            *self
                .auto_disable_fired
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = true;
            return VerificationPlan::fallback();
        }
        let window = self.compute_window(draft.tokens.len(), profile);
        if window == 0 {
            return VerificationPlan::fallback();
        }
        VerificationPlan {
            window,
            emit_correction: true,
        }
    }
}

// ── Shared acceptance history ──────────────────────────────────────────────────

#[derive(Debug)]
pub(super) struct AcceptanceHistory(Mutex<VecDeque<f32>>);

impl AcceptanceHistory {
    pub(super) fn new() -> Self {
        Self(Mutex::new(VecDeque::with_capacity(HISTORY_LEN)))
    }

    pub(super) fn push(&self, fraction: f32) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() == HISTORY_LEN {
            q.pop_front();
        }
        q.push_back(fraction.clamp(0.0, 1.0));
    }

    pub(super) fn mean(&self) -> f32 {
        let q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if q.is_empty() {
            return 0.5;
        }
        q.iter().copied().sum::<f32>() / q.len() as f32
    }
}

#[cfg(test)]
mod tests;
