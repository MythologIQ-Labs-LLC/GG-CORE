//! Degraded-mode policy for constrained local inference (B-07).
//!
//! When a request meets resource pressure, GG-CORE degrades *intentionally* and
//! *explains* the tradeoff rather than failing blindly — the CONCEPT triage
//! thesis (system stability + fair allocation over individual-request ego).
//! The decision (`evaluate`) is pure and total over a neutral `ResourcePressure`
//! signal, independent of which `InferenceError` surfaced it; the mechanism
//! (prompt truncation) is the thin effectful edge in the engine run path.

/// Policy knobs for degraded-mode behavior under resource pressure.
#[derive(Debug, Clone)]
pub struct DegradedModeConfig {
    /// Truncate an over-budget prompt to the context limit instead of failing.
    pub allow_context_reduction: bool,
    /// Never reduce below this many tokens; below it, reject instead of truncating.
    pub min_context_tokens: usize,
}

impl Default for DegradedModeConfig {
    fn default() -> Self {
        Self {
            allow_context_reduction: true,
            min_context_tokens: 16,
        }
    }
}

/// A neutral resource-pressure signal, independent of the `InferenceError` enums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourcePressure {
    /// Prompt exceeds the context budget (`got` tokens vs `max`).
    Context { max: usize, got: usize },
    /// A call/total memory budget is exceeded.
    Memory { used: usize, limit: usize },
    /// The loaded backend does not support a requested capability.
    Capability { name: String },
}

/// The intentional, explained action degraded mode chooses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradedDecision {
    /// Proceed, reducing the effective context to this many tokens.
    ReduceContextTo { tokens: usize, reason: String },
    /// Fail loud, but with an explanation of the tradeoff.
    Reject { reason: String },
    // Future (BitNet, B-02..B-06): PreferModel { model_id, reason } — not implemented.
}

/// Decides degraded actions for resource-pressure signals.
#[derive(Debug, Clone, Default)]
pub struct DegradedModePolicy {
    config: DegradedModeConfig,
}

impl DegradedModePolicy {
    pub fn new(config: DegradedModeConfig) -> Self {
        Self { config }
    }

    /// Decide the degraded action for a pressure signal. Pure and total.
    pub fn evaluate(&self, pressure: ResourcePressure) -> DegradedDecision {
        match pressure {
            ResourcePressure::Context { max, got }
                if self.config.allow_context_reduction && max >= self.config.min_context_tokens =>
            {
                DegradedDecision::ReduceContextTo {
                    tokens: max,
                    reason: format!(
                        "context {got} tok over limit {max}; reduced to {max} (degraded mode)"
                    ),
                }
            }
            ResourcePressure::Context { max, got } => DegradedDecision::Reject {
                reason: format!("context {got} tok over limit {max}; reduction disabled"),
            },
            ResourcePressure::Memory { used, limit } => DegradedDecision::Reject {
                reason: format!("memory {used}B over limit {limit}B; no smaller model available"),
            },
            ResourcePressure::Capability { name } => DegradedDecision::Reject {
                reason: format!("capability '{name}' unsupported by the loaded backend"),
            },
        }
    }
}

/// Truncate `s` to the largest char boundary at or below `max_bytes`, never
/// splitting a UTF-8 codepoint. Returns the whole string when already within budget.
pub fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
#[path = "degraded_mode_tests.rs"]
mod tests;
