//! TDD-Light tests for Phase A inference types.
//!
//! Validates config, input, output, and manifest types.

use gg_core::engine::{
    ChatMessage, ChatRole, ClassificationResult, EmbeddingResult, FinishReason,
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError,
    InferenceInput, InferenceOutput, MAX_BATCH_SIZE, MAX_TEXT_BYTES,
};
use gg_core::models::{ModelArchitecture, ModelCapability, ModelManifest};

// ============================================================================
// InferenceConfig Tests
// ============================================================================

#[test]
fn config_default_values_are_safe() {
    let config = InferenceConfig::default();
    assert!(config.validate().is_ok());
    assert_eq!(config.timeout_ms, 30_000);
    assert!(config.max_tokens.is_some());
}

#[test]
fn config_rejects_invalid_temperature() {
    let mut config = InferenceConfig::default();
    config.temperature = -0.1;
    assert!(matches!(
        config.validate(),
        Err(InferenceError::InputValidation(_))
    ));
}

#[test]
fn config_rejects_invalid_top_p() {
    let mut config = InferenceConfig::default();
    config.top_p = 0.0;
    assert!(config.validate().is_err());

    config.top_p = 1.5;
    assert!(config.validate().is_err());
}

#[test]
fn config_for_classification_is_deterministic() {
    let config = InferenceConfig::for_classification();
    assert!(config.validate().is_ok());
    assert_eq!(config.temperature, 0.0);
    assert!(config.max_tokens.is_none());
}

// ============================================================================
// InferenceInput Tests
// ============================================================================

#[test]
fn input_text_validates_non_empty() {
    let input = InferenceInput::Text("hello world".to_string());
    assert!(input.validate().is_ok());
}

#[test]
fn input_text_rejects_empty() {
    let input = InferenceInput::Text(String::new());
    assert!(matches!(
        input.validate(),
        Err(InferenceError::InputValidation(_))
    ));
}

#[test]
fn input_text_rejects_oversized() {
    let large_text = "x".repeat(MAX_TEXT_BYTES + 1);
    let input = InferenceInput::Text(large_text);
    assert!(input.validate().is_err());
}

#[test]
fn input_batch_validates_within_limit() {
    let batch = vec!["one".to_string(), "two".to_string()];
    let input = InferenceInput::TextBatch(batch);
    assert!(input.validate().is_ok());
}

#[test]
fn input_batch_rejects_oversized() {
    let batch: Vec<String> = (0..MAX_BATCH_SIZE + 1).map(|i| i.to_string()).collect();
    let input = InferenceInput::TextBatch(batch);
    assert!(input.validate().is_err());
}

#[test]
fn input_chat_validates_messages() {
    let messages = vec![
        ChatMessage { role: ChatRole::System, content: "You are helpful.".to_string() },
        ChatMessage { role: ChatRole::User, content: "Hello!".to_string() },
    ];
    let input = InferenceInput::ChatMessages(messages);
    assert!(input.validate().is_ok());
}

#[test]
fn input_byte_size_calculated_correctly() {
    let input = InferenceInput::Text("hello".to_string());
    assert_eq!(input.byte_size(), 5);
}

// ============================================================================
// InferenceOutput Tests
// ============================================================================

#[test]
fn output_classification_result_created() {
    let result = ClassificationResult {
        label: "positive".to_string(),
        confidence: 0.95,
        all_labels: vec![("positive".to_string(), 0.95), ("negative".to_string(), 0.05)],
    };
    let output = InferenceOutput::Classification(result);
    assert!(output.is_classification());
    assert!(!output.is_generation());
}

#[test]
fn output_generation_result_created() {
    let result = GenerationResult {
        text: "Generated text here".to_string(),
        tokens_generated: 10,
        finish_reason: FinishReason::Stop,
    };
    let output = InferenceOutput::Generation(result);
    assert!(output.is_generation());
}

#[test]
fn output_embedding_result_created() {
    let result = EmbeddingResult {
        vector: vec![0.1, 0.2, 0.3],
        dimensions: 3,
    };
    let output = InferenceOutput::Embedding(result);
    assert!(output.is_embedding());
}

// ============================================================================
// ModelManifest Tests
// ============================================================================

#[test]
fn manifest_parses_valid_json() {
    let json = r#"{
        "model_id": "test-model-v1",
        "name": "Test Model",
        "version": "1.0.0",
        "capabilities": ["text_classification"],
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "size_bytes": 1024,
        "architecture": "onnx",
        "license": "MIT"
    }"#;
    let manifest = ModelManifest::from_json(json);
    assert!(manifest.is_ok());
    let manifest = manifest.unwrap();
    assert_eq!(manifest.model_id, "test-model-v1");
    assert_eq!(manifest.architecture, ModelArchitecture::Onnx);
}

#[test]
fn manifest_validates_sha256_length() {
    let json = r#"{
        "model_id": "test",
        "name": "Test",
        "version": "1.0.0",
        "capabilities": ["text_classification"],
        "sha256": "tooshort",
        "size_bytes": 1024,
        "architecture": "onnx",
        "license": "MIT"
    }"#;
    let manifest = ModelManifest::from_json(json).unwrap();
    assert!(manifest.validate().is_err());
}

#[test]
fn manifest_has_capability_check() {
    let json = r#"{
        "model_id": "test",
        "name": "Test",
        "version": "1.0.0",
        "capabilities": ["text_classification", "embedding"],
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "size_bytes": 1024,
        "architecture": "onnx",
        "license": "MIT"
    }"#;
    let manifest = ModelManifest::from_json(json).unwrap();
    assert!(manifest.has_capability(ModelCapability::TextClassification));
    assert!(manifest.has_capability(ModelCapability::Embedding));
    assert!(!manifest.has_capability(ModelCapability::TextGeneration));
}

// ============================================================================
// InferenceCapability Tests
// ============================================================================

#[test]
fn capability_enum_equality() {
    assert_eq!(InferenceCapability::TextClassification, InferenceCapability::TextClassification);
    assert_ne!(InferenceCapability::TextClassification, InferenceCapability::Embedding);
}
