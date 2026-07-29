//! Degraded-mode context resolution for `InferenceEngine` (B-07).
//!
//! Extracted from `inference.rs` (Section 4 Razor) as a child module so it
//! retains access to the engine's private `degraded` policy and
//! `max_context_length`.

use std::borrow::Cow;

use super::{InferenceEngine, InferenceError};
use crate::engine::degraded_mode::truncate_on_char_boundary;
use crate::engine::{DegradedDecision, ResourcePressure};

impl InferenceEngine {
    /// Resolve the prompt under the degraded-mode policy: within the context
    /// budget it is returned unchanged; over budget it is either truncated to
    /// the limit (logged) or rejected with an explanation.
    pub(super) fn apply_degraded_context<'p>(
        &self,
        prompt: &'p str,
    ) -> Result<Cow<'p, str>, InferenceError> {
        let got = prompt.len() / Self::BYTES_PER_TOKEN;
        if got <= self.max_context_length {
            return Ok(Cow::Borrowed(prompt));
        }
        match self.degraded.evaluate(ResourcePressure::Context {
            max: self.max_context_length,
            got,
        }) {
            DegradedDecision::ReduceContextTo { tokens, reason } => {
                tracing::warn!(target: "gg_core::degraded", "{reason}");
                let byte_budget = tokens * Self::BYTES_PER_TOKEN;
                Ok(Cow::Owned(truncate_on_char_boundary(prompt, byte_budget)))
            }
            DegradedDecision::Reject { reason } => {
                tracing::warn!(target: "gg_core::degraded", "reject: {reason}");
                Err(InferenceError::ContextExceeded {
                    max: self.max_context_length,
                    got,
                })
            }
        }
    }
}
