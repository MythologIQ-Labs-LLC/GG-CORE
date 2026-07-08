//! Aggregate speculative decoding telemetry and auto-disable tracking.
//!
//! Provides [`SpeculativeTelemetry`] for accumulating per-step statistics and
//! producing immutable [`SpeculativeSessionStats`] snapshots for the CLI and
//! monitoring layer.
//!
//! # Security (T3)
//!
//! No prompt text, output text, or user-identifying data is stored in any
//! field. Every field is a numeric aggregate or a structured reason code.
//!
//! # Example
//!
//! ```rust,ignore
//! # #[cfg(feature = "advanced")]
//! # {
//! use gg_core::engine::adaptive_speculative::telemetry::{
//!     AutoDisableReason, SpeculativeTelemetry,
//! };
//!
//! let t = SpeculativeTelemetry::new();
//! t.record_step(4, 3, 120, 85);
//! t.record_step(4, 4, 115, 80);
//! let s = t.snapshot();
//! assert_eq!(s.verification_steps, 2);
//! assert_eq!(s.accepted_tokens, 7);
//! # }
//! ```

#![cfg(feature = "advanced")]

use std::sync::Mutex;

// ── AutoDisableReason ─────────────────────────────────────────────────────────

/// Structured reason codes for speculative auto-disable events.
///
/// Each variant is a machine-readable code — never user content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDisableReason {
    /// Rolling acceptance rate fell below the configured minimum.
    AcceptanceRateLow,
    /// Net speedup estimate dropped below the configured threshold.
    SpeedupBelowThreshold,
    /// Draft/target model pair reported an incompatible hardware profile.
    PairingIncompatible,
    /// Caller explicitly disabled speculative decoding.
    ExplicitDisable,
}

impl AutoDisableReason {
    /// Stable string code for serialisation (never contains user data).
    pub fn as_code(&self) -> &'static str {
        match self {
            Self::AcceptanceRateLow => "ACCEPTANCE_RATE_LOW",
            Self::SpeedupBelowThreshold => "SPEEDUP_BELOW_THRESHOLD",
            Self::PairingIncompatible => "PAIRING_INCOMPATIBLE",
            Self::ExplicitDisable => "EXPLICIT_DISABLE",
        }
    }
}

// ── SpeculativeSessionStats ───────────────────────────────────────────────────

/// Immutable snapshot of aggregate speculative decoding statistics.
///
/// All fields are numeric aggregates or structured codes.
/// No field contains prompt text, output text, or user identifiers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpeculativeSessionStats {
    /// Total draft tokens proposed across all steps.
    pub draft_tokens_generated: u64,
    /// Number of draft-verify cycles executed.
    pub verification_steps: u64,
    /// Total draft tokens accepted by the target model.
    pub accepted_tokens: u64,
    /// Total draft tokens rejected by the target model.
    pub rejected_tokens: u64,
    /// Mean accepted tokens per verification step.
    pub mean_accepted_length: f32,
    /// Fraction of draft tokens accepted — `accepted / draft_tokens_generated`.
    pub acceptance_rate: f32,
    /// Mean draft-generation latency in microseconds.
    pub mean_draft_latency_us: f32,
    /// Mean verification latency in microseconds.
    pub mean_verify_latency_us: f32,
    /// Estimated net speedup relative to single-token baseline (≥ 1.0 is beneficial).
    pub net_speedup: f32,
    /// Number of times auto-disable has fired during this session.
    pub auto_disable_count: u32,
    /// Most-recent auto-disable reason code, if any.
    pub auto_disable_reason: Option<String>,
}

// ── Internal accumulator ──────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Inner {
    draft_tokens_generated: u64,
    verification_steps: u64,
    accepted_tokens: u64,
    rejected_tokens: u64,
    total_draft_latency_us: u64,
    total_verify_latency_us: u64,
    auto_disable_count: u32,
    auto_disable_reason: Option<String>,
}

// ── SpeculativeTelemetry ──────────────────────────────────────────────────────

/// Thread-safe accumulator for speculative decoding telemetry.
///
/// Hold behind an `Arc` shared across the executor loop. All mutations use
/// interior mutability via `Mutex<Inner>` so `&SpeculativeTelemetry` suffices.
#[derive(Debug)]
pub struct SpeculativeTelemetry {
    inner: Mutex<Inner>,
}

impl Default for SpeculativeTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeculativeTelemetry {
    /// Construct a zeroed telemetry accumulator.
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()) }
    }

    /// Record one draft-verify cycle.
    ///
    /// - `draft_count`: tokens proposed by the draft model this step.
    /// - `accepted`: tokens accepted by the target model (≤ `draft_count`).
    /// - `draft_us`: microseconds spent in the draft forward-pass.
    /// - `verify_us`: microseconds spent in the target verification pass.
    pub fn record_step(&self, draft_count: u32, accepted: u32, draft_us: u64, verify_us: u64) {
        let rejected = draft_count.saturating_sub(accepted);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.draft_tokens_generated += u64::from(draft_count);
        g.verification_steps += 1;
        g.accepted_tokens += u64::from(accepted);
        g.rejected_tokens += u64::from(rejected);
        g.total_draft_latency_us += draft_us;
        g.total_verify_latency_us += verify_us;
    }

    /// Record an auto-disable event with a structured reason code.
    ///
    /// The stored reason is always [`AutoDisableReason::as_code`] — a static
    /// string, never user data.
    pub fn record_auto_disable(&self, reason: &AutoDisableReason) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.auto_disable_count += 1;
        g.auto_disable_reason = Some(reason.as_code().to_string());
    }

    /// Produce an immutable snapshot of the current aggregate state.
    ///
    /// Derived fields are computed at snapshot time from the raw accumulators.
    pub fn snapshot(&self) -> SpeculativeSessionStats {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let steps = g.verification_steps;
        let steps_f = steps as f32;

        let acceptance_rate = if g.draft_tokens_generated == 0 {
            0.0_f32
        } else {
            g.accepted_tokens as f32 / g.draft_tokens_generated as f32
        };
        let mean_accepted_length =
            if steps == 0 { 0.0_f32 } else { g.accepted_tokens as f32 / steps_f };
        let mean_draft_latency_us =
            if steps == 0 { 0.0_f32 } else { g.total_draft_latency_us as f32 / steps_f };
        let mean_verify_latency_us =
            if steps == 0 { 0.0_f32 } else { g.total_verify_latency_us as f32 / steps_f };
        // Values > 1.0 mean speculative decoding is beneficial.
        let net_speedup = mean_accepted_length.max(1.0);

        SpeculativeSessionStats {
            draft_tokens_generated: g.draft_tokens_generated,
            verification_steps: steps,
            accepted_tokens: g.accepted_tokens,
            rejected_tokens: g.rejected_tokens,
            mean_accepted_length,
            acceptance_rate,
            mean_draft_latency_us,
            mean_verify_latency_us,
            net_speedup,
            auto_disable_count: g.auto_disable_count,
            auto_disable_reason: g.auto_disable_reason.clone(),
        }
    }

    /// Reset all accumulators to zero.
    ///
    /// Used when starting a new session or when the model is hot-swapped.
    pub fn reset(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *g = Inner::default();
    }
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
