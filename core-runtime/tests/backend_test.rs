//! TDD-Light tests for inference backends (ONNX and GGUF).
//!
//! Note: Tests that require actual inference return errors since no model is loaded.
//! This is the correct behavior - no mock fallbacks.

use gg_core::engine::{
    InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
};
use gg_core::engine::onnx::{OnnxClassifier, OnnxEmbedder, OnnxModel};
use gg_core::engine::gguf::{GgufGenerator, GgufModel};

// ============================================================================
// ONNX Classifier Tests
// ============================================================================

#[tokio::test]
async fn onnx_classifier_has_correct_capabilities() {
    let classifier = OnnxClassifier::new(
        "test-classifier".to_string(),
        vec!["positive".to_string(), "negative".to_string()],
    );

    let caps = classifier.capabilities();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0], InferenceCapability::TextClassification);
}

#[tokio::test]
async fn onnx_classifier_returns_model_id() {
    let classifier = OnnxClassifier::new(
        "test-model-001".to_string(),
        vec!["label".to_string()],
    );

    assert_eq!(classifier.model_id(), "test-model-001");
}

#[tokio::test]
async fn onnx_classifier_infers_text_requires_loaded_model() {
    let classifier = OnnxClassifier::new(
        "test-classifier".to_string(),
        vec!["positive".to_string(), "negative".to_string()],
    );

    let input = InferenceInput::Text("This is a test sentence.".to_string());
    let config = InferenceConfig::for_classification();

    // No model loaded - should return error (no mock fallback)
    let result = classifier.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::ModelError(_))));
}

#[tokio::test]
async fn onnx_classifier_rejects_empty_text() {
    let classifier = OnnxClassifier::new(
        "test-classifier".to_string(),
        vec!["label".to_string()],
    );

    let input = InferenceInput::Text(String::new());
    let config = InferenceConfig::for_classification();

    let result = classifier.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::InputValidation(_))));
}

#[tokio::test]
async fn onnx_classifier_rejects_chat_messages() {
    let classifier = OnnxClassifier::new(
        "test-classifier".to_string(),
        vec!["label".to_string()],
    );

    let input = InferenceInput::ChatMessages(vec![]);
    let config = InferenceConfig::for_classification();

    let result = classifier.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::InputValidation(_))));
}

// ============================================================================
// ONNX Embedder Tests
// ============================================================================

#[tokio::test]
async fn onnx_embedder_has_correct_capabilities() {
    let embedder = OnnxEmbedder::new("test-embedder".to_string(), 384);

    let caps = embedder.capabilities();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0], InferenceCapability::Embedding);
}

#[tokio::test]
async fn onnx_embedder_requires_loaded_model() {
    let embedder = OnnxEmbedder::new("test-embedder".to_string(), 384);

    let input = InferenceInput::Text("Test text for embedding.".to_string());
    let config = InferenceConfig::for_embedding();

    // No model loaded - should return error (no mock fallback)
    let result = embedder.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::ModelError(_))));
}

// ============================================================================
// GGUF Generator Tests
// ============================================================================

#[tokio::test]
async fn gguf_generator_has_correct_capabilities() {
    let generator = GgufGenerator::new("test-generator".to_string(), 2048);

    let caps = generator.capabilities();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0], InferenceCapability::TextGeneration);
}

#[tokio::test]
async fn gguf_generator_returns_model_id() {
    let generator = GgufGenerator::new("phi-3-mini".to_string(), 2048);

    assert_eq!(generator.model_id(), "phi-3-mini");
}

#[tokio::test]
async fn gguf_generator_requires_loaded_model() {
    let generator = GgufGenerator::new("test-generator".to_string(), 2048);

    let input = InferenceInput::Text("Once upon a time".to_string());
    let config = InferenceConfig::default();

    // No model loaded - should return error (no mock fallback)
    let result = generator.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::ModelError(_))));
}

#[tokio::test]
async fn gguf_generator_chat_requires_loaded_model() {
    use gg_core::engine::{ChatMessage, ChatRole};

    let generator = GgufGenerator::new("test-generator".to_string(), 2048);

    let input = InferenceInput::ChatMessages(vec![
        ChatMessage {
            role: ChatRole::System,
            content: "You are a helpful assistant.".to_string(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: "Hello!".to_string(),
        },
    ]);
    let config = InferenceConfig::default();

    // No model loaded - should return error (no mock fallback)
    let result = generator.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::ModelError(_))));
}

#[tokio::test]
async fn gguf_generator_rejects_batch_input() {
    let generator = GgufGenerator::new("test-generator".to_string(), 2048);

    let input = InferenceInput::TextBatch(vec!["text1".to_string(), "text2".to_string()]);
    let config = InferenceConfig::default();

    let result = generator.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::CapabilityNotSupported(_))));
}

#[tokio::test]
async fn gguf_generator_rejects_empty_prompt() {
    let generator = GgufGenerator::new("test-generator".to_string(), 2048);

    let input = InferenceInput::Text(String::new());
    let config = InferenceConfig::default();

    let result = generator.infer(&input, &config).await;
    assert!(matches!(result, Err(InferenceError::InputValidation(_))));
}
