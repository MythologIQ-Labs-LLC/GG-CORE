//! Shared value types for speculative decoding.
//!
//! Configuration, statistics, and per-step verification results consumed by the
//! adaptive speculative executor (`engine/adaptive_speculative/`), the GGUF
//! `verify_draft_tokens` return contract, and the `tier_synergy` public API.
//! Pure values — no behavior, no engine coupling.

/// Configuration for speculative decoding.
#[derive(Debug, Clone)]
pub struct SpeculativeConfig {
    /// Number of draft tokens to generate before verification.
    pub draft_tokens: usize,
    /// Acceptance threshold (0.0 to 1.0) for probability-based acceptance.
    pub acceptance_threshold: f32,
    /// Enable speculative decoding.
    pub enabled: bool,
    /// Minimum acceptance rate before reducing draft tokens.
    pub min_acceptance_rate: f32,
    /// Maximum draft tokens to generate.
    pub max_draft_tokens: usize,
    /// Adapt draft token count based on acceptance rate.
    pub adaptive: bool,
}

impl Default for SpeculativeConfig {
    fn default() -> Self {
        Self {
            draft_tokens: 4,
            acceptance_threshold: 0.9,
            enabled: true,
            min_acceptance_rate: 0.5,
            max_draft_tokens: 8,
            adaptive: true,
        }
    }
}

/// Statistics for speculative decoding performance.
#[derive(Debug, Default, Clone)]
pub struct SpeculativeStats {
    /// Total draft tokens generated.
    pub total_draft_tokens: u64,
    /// Total tokens accepted.
    pub total_accepted: u64,
    /// Total tokens rejected.
    pub total_rejected: u64,
    /// Total verification steps.
    pub total_verifications: u64,
    /// Total time spent in draft generation.
    pub draft_time_ns: u64,
    /// Total time spent in verification.
    pub verify_time_ns: u64,
}

impl SpeculativeStats {
    /// Calculate acceptance rate.
    pub fn acceptance_rate(&self) -> f64 {
        if self.total_draft_tokens == 0 {
            return 0.0;
        }
        self.total_accepted as f64 / self.total_draft_tokens as f64
    }

    /// Calculate average tokens per verification.
    pub fn avg_tokens_per_verification(&self) -> f64 {
        if self.total_verifications == 0 {
            return 0.0;
        }
        self.total_accepted as f64 / self.total_verifications as f64
    }

    /// Calculate speedup estimate.
    pub fn estimated_speedup(&self) -> f64 {
        if self.total_verifications == 0 {
            return 1.0;
        }
        // Speedup = accepted tokens / verifications (each verification is one forward pass)
        self.avg_tokens_per_verification()
    }
}

/// Result from the verification phase.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// Number of draft tokens accepted.
    pub accepted_count: usize,
    /// Correction token if verification diverged.
    pub correction_token: Option<u32>,
    /// Acceptance probabilities for each token.
    pub probabilities: Vec<f32>,
}

impl VerifyResult {
    /// Create a result where all tokens are accepted.
    pub fn accept_all(count: usize) -> Self {
        Self {
            accepted_count: count,
            correction_token: None,
            probabilities: vec![1.0; count],
        }
    }

    /// Create a result where verification diverged.
    pub fn diverge_at(accepted: usize, correction: u32) -> Self {
        Self {
            accepted_count: accepted,
            correction_token: Some(correction),
            probabilities: vec![1.0; accepted],
        }
    }

    /// Create a result with probabilities.
    pub fn with_probabilities(accepted: usize, correction: Option<u32>, probs: Vec<f32>) -> Self {
        Self {
            accepted_count: accepted,
            correction_token: correction,
            probabilities: probs,
        }
    }
}
