//! Inference error types for CORE Runtime.
//!
//! All errors are fail-closed: invalid inputs are rejected, not truncated.

use thiserror::Error;

/// Errors that can occur during inference operations.
#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("Model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("Input validation failed: {0}")]
    InputValidation(String),

    #[error("Input validation failed for batch item {index}: {error}")]
    BatchItemValidation {
        index: usize,
        error: Box<InferenceError>,
    },

    #[error("Inference timeout after {0}ms")]
    Timeout(u64),

    #[error("Memory limit exceeded: used {used} bytes, limit {limit} bytes")]
    MemoryExceeded { used: usize, limit: usize },

    #[error("Output filter rejected: {reason}")]
    OutputFiltered { reason: String },

    #[error("Model error: {0}")]
    ModelError(String),

    #[error("Rate limit exceeded")]
    RateLimited,

    #[error("Queue full: {current}/{max} pending requests")]
    QueueFull { current: usize, max: usize },

    #[error("Capability not supported: {0}")]
    CapabilityNotSupported(String),

    #[error("Hash mismatch for model {model_id}: expected {expected}, got {actual}")]
    HashMismatch {
        model_id: String,
        expected: String,
        actual: String,
    },

    #[error("Invalid model format: {0}")]
    InvalidFormat(String),
}

impl InferenceError {
    /// Returns true if this error should be logged as a warning.
    pub fn is_warning(&self) -> bool {
        matches!(self, Self::RateLimited | Self::QueueFull { .. })
    }

    /// Returns true if this error indicates a security concern.
    pub fn is_security_concern(&self) -> bool {
        matches!(
            self,
            Self::HashMismatch { .. }
                | Self::InputValidation(_)
                | Self::BatchItemValidation { .. }
                | Self::OutputFiltered { .. }
        )
    }
}
