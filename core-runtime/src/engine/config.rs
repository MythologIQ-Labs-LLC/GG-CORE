//! Inference configuration types for CORE Runtime.
//!
//! All fields have safe defaults. Configuration is validated before use.

use super::error::InferenceError;

/// Per-call inference configuration.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Maximum tokens to generate (generation only). None = classification/embedding.
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (0.0 = deterministic, 1.0 = creative)
    pub temperature: f32,
    /// Top-p (nucleus) sampling threshold (0.0–1.0)
    pub top_p: f32,
    /// Top-k sampling limit (0 = disabled)
    pub top_k: u32,
    /// Repetition penalty (1.0 = none, >1.0 = penalize repeats)
    pub repetition_penalty: f32,
    /// Hard timeout in milliseconds — inference killed after this
    pub timeout_ms: u64,
    /// Maximum memory allowed for this call (bytes). None = use global limit.
    pub max_memory_bytes: Option<usize>,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            max_tokens: Some(256),
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repetition_penalty: 1.1,
            timeout_ms: 30_000,
            max_memory_bytes: Some(1024 * 1024 * 1024), // 1GB
        }
    }
}

impl InferenceConfig {
    /// Validate configuration values. Returns error on invalid values.
    pub fn validate(&self) -> Result<(), InferenceError> {
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err(InferenceError::InputValidation(
                "temperature must be finite and in [0, 2]".into(),
            ));
        }
        if !self.top_p.is_finite() || self.top_p <= 0.0 || self.top_p > 1.0 {
            return Err(InferenceError::InputValidation(
                "top_p must be finite and in (0, 1]".into(),
            ));
        }
        if !self.repetition_penalty.is_finite() || self.repetition_penalty < 1.0 {
            return Err(InferenceError::InputValidation(
                "repetition_penalty must be finite and >= 1.0".into(),
            ));
        }
        if self.max_tokens == Some(0) {
            return Err(InferenceError::InputValidation(
                "max_tokens must be > 0 when specified".into(),
            ));
        }
        if self.timeout_ms == 0 {
            return Err(InferenceError::InputValidation(
                "timeout_ms must be > 0".into(),
            ));
        }
        Ok(())
    }

    /// Create a config for deterministic classification (no sampling).
    pub fn for_classification() -> Self {
        Self {
            max_tokens: None,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 0,
            repetition_penalty: 1.0,
            timeout_ms: 5_000,
            max_memory_bytes: Some(512 * 1024 * 1024), // 512MB
        }
    }

    /// Create a config for embedding generation.
    pub fn for_embedding() -> Self {
        Self {
            max_tokens: None,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 0,
            repetition_penalty: 1.0,
            timeout_ms: 2_000,
            max_memory_bytes: Some(256 * 1024 * 1024), // 256MB
        }
    }
}

#[cfg(test)]
mod numeric_validation_tests {
    use super::*;

    #[test]
    fn valid_inference_profiles_and_boundaries_remain_accepted() {
        for config in [
            InferenceConfig::default(),
            InferenceConfig::for_classification(),
            InferenceConfig::for_embedding(),
        ] {
            assert!(config.validate().is_ok());
        }
        let config = InferenceConfig {
            max_tokens: Some(1),
            temperature: 2.0,
            top_p: 1.0,
            repetition_penalty: 1.0,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn direct_config_rejects_nan_infinity_and_invalid_ranges() {
        let mut config = InferenceConfig::default();
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 3.0] {
            config.temperature = value;
            assert!(config.validate().is_err(), "accepted temperature {value:?}");
        }
        config.temperature = 0.7;
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 0.0, 1.01] {
            config.top_p = value;
            assert!(config.validate().is_err(), "accepted top_p {value:?}");
        }
        config.top_p = 0.9;
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.99] {
            config.repetition_penalty = value;
            assert!(config.validate().is_err(), "accepted repetition_penalty {value:?}");
        }
        config.repetition_penalty = 1.1;
        config.max_tokens = Some(0);
        assert!(config.validate().is_err(), "zero max tokens admitted");
    }
}
