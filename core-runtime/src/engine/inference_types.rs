//! Types for the inference engine.
//!
//! Extracted from `inference.rs` for Section 4 compliance.

use thiserror::Error;

use crate::engine::InferenceConfig;

#[derive(Error, Debug)]
pub enum InferenceError {
    #[error("Model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Inference failed: {0}")]
    ExecutionFailed(String),

    #[error("Context length exceeded: max {max}, got {got}")]
    ContextExceeded { max: usize, got: usize },

    #[error("Memory limit exceeded: used {used} bytes, limit {limit} bytes")]
    MemoryExceeded { used: usize, limit: usize },

    #[error("{0}")]
    SecurityRejected(String),

    /// The caller cancelled the request (cancel_request, disconnect, or drop).
    #[error("Request cancelled")]
    Cancelled,

    /// The request's `timeout_ms` elapsed before it completed.
    #[error("Deadline exceeded: request did not complete within its {timeout_ms} ms timeout")]
    DeadlineExceeded { timeout_ms: u64 },
}

/// Parameters controlling inference behavior (IPC protocol).
///
/// Every field is optional on the wire; a missing field takes the documented
/// default (`docs/IPC_PROTOCOL_SCHEMA.md`: 256 / 0.7 / 0.9 / 40, no streaming,
/// no deadline). Serialization still writes every field.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct InferenceParams {
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: usize,
    /// Enable token-by-token streaming response.
    #[serde(default)]
    pub stream: bool,
    /// Request timeout in milliseconds. None = no timeout.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl Default for InferenceParams {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            stream: false,
            timeout_ms: None,
        }
    }
}

impl InferenceParams {
    pub fn validate(&self) -> Result<(), InferenceError> {
        if self.max_tokens == 0 || u32::try_from(self.max_tokens).is_err() {
            return Err(InferenceError::InvalidParams(
                "max_tokens must be in the range 1..=u32::MAX".into(),
            ));
        }
        if u32::try_from(self.top_k).is_err() {
            return Err(InferenceError::InvalidParams(
                "top_k must be <= u32::MAX".into(),
            ));
        }
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err(InferenceError::InvalidParams(
                "temperature must be finite and in [0, 2]".into(),
            ));
        }
        if !self.top_p.is_finite() || self.top_p <= 0.0 || self.top_p > 1.0 {
            return Err(InferenceError::InvalidParams(
                "top_p must be finite and in (0, 1]".into(),
            ));
        }
        if self.timeout_ms == Some(0) {
            return Err(InferenceError::InvalidParams(
                "timeout_ms must be > 0".into(),
            ));
        }
        Ok(())
    }

    /// Convert to internal InferenceConfig format.
    ///
    /// Call `validate` before using this configuration for inference. For
    /// callers that convert an unvalidated value, bound integer narrowing
    /// saturates rather than wrapping a large request to zero or a smaller
    /// token count; normal admitted requests never need the fallback.
    pub fn to_config(&self) -> InferenceConfig {
        InferenceConfig {
            max_tokens: Some(u32::try_from(self.max_tokens).unwrap_or(u32::MAX)),
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: u32::try_from(self.top_k).unwrap_or(u32::MAX),
            repetition_penalty: 1.1,
            timeout_ms: self.timeout_ms.unwrap_or(30_000),
            max_memory_bytes: None,
        }
    }
}

/// Result of inference execution.
#[derive(Debug, Clone)]
pub struct InferenceResult {
    /// Generated text output.
    pub output: String,
    pub tokens_generated: usize,
    pub finished: bool,
}

#[cfg(test)]
mod parameter_admission_tests {
    use super::*;

    #[test]
    fn finite_range_boundaries_are_accepted_and_invalid_values_refused() {
        let mut params = InferenceParams::default();
        assert!(params.validate().is_ok());
        for temperature in [0.0_f32, 2.0] {
            params.temperature = temperature;
            assert!(params.validate().is_ok());
        }
        for temperature in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 2.01] {
            params.temperature = temperature;
            assert!(params.validate().is_err(), "accepted temperature {temperature:?}");
        }
        params.temperature = 0.7;
        for top_p in [0.000_1_f32, 1.0] {
            params.top_p = top_p;
            assert!(params.validate().is_ok());
        }
        for top_p in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 0.0, 1.01] {
            params.top_p = top_p;
            assert!(params.validate().is_err(), "accepted top_p {top_p:?}");
        }
    }

    #[test]
    fn oversized_wire_integer_limits_cannot_wrap_backend_config() {
        let mut params = InferenceParams::default();
        params.max_tokens = 0;
        assert!(params.validate().is_err());
        params.max_tokens = 1;
        assert!(params.validate().is_ok());
        if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
            params.max_tokens = too_large;
            assert!(params.validate().is_err(), "oversized max_tokens admitted");
            assert_eq!(params.to_config().max_tokens, Some(u32::MAX));
            params.max_tokens = 1;
            params.top_k = too_large;
            assert!(params.validate().is_err(), "oversized top_k admitted");
            assert_eq!(params.to_config().top_k, u32::MAX);
        }
    }

    #[test]
    fn valid_parameters_convert_without_mutation() {
        let params = InferenceParams {
            max_tokens: 512,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 60,
            stream: false,
            timeout_ms: Some(12_345),
        };
        assert!(params.validate().is_ok());
        let converted = params.to_config();
        assert_eq!(converted.max_tokens, Some(512));
        assert_eq!(converted.top_k, 60);
        assert_eq!(converted.temperature, 0.0);
        assert_eq!(converted.top_p, 1.0);
        assert_eq!(converted.timeout_ms, 12_345);
    }
}
