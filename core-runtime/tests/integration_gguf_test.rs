//! Integration tests for GGUF model operations.
//!
//! Tests GGUF model configuration, generation structures, and memory-mapped loading.

use gg_core::engine::{
    FinishReason, GenerationResult, GgufConfig, InferenceOutput,
    InferenceParams, ChatMessage, ChatRole,
};
use gg_core::models::ModelLoader;

fn create_test_loader() -> ModelLoader {
    let base = std::env::temp_dir().join("core_runtime_gguf_test");
    std::fs::create_dir_all(base.join("models")).ok();
    ModelLoader::new(base)
}

#[test]
fn gguf_config_defaults_valid() {
    let config = GgufConfig::default();

    assert!(config.n_ctx > 0, "Context size should be positive");
    // n_threads of 0 means auto-detect, which is valid
    assert_eq!(config.n_gpu_layers, 0, "GPU layers should be 0 for sandbox");
}

#[test]
fn gguf_model_requires_valid_path() {
    let loader = create_test_loader();

    // Attempting to load non-existent model should fail
    let result = loader.validate_path("models/nonexistent.gguf");
    assert!(result.is_err(), "Should fail for non-existent model");
}

#[test]
fn generation_result_structure_valid() {
    // Test that GenerationResult has expected structure
    let result = GenerationResult {
        text: "Generated text output".to_string(),
        tokens_generated: 5,
        finish_reason: FinishReason::Stop,
    };

    assert!(!result.text.is_empty());
    assert_eq!(result.tokens_generated, 5);
    assert_eq!(result.finish_reason, FinishReason::Stop);
}

#[test]
fn stop_reason_variants_correct() {
    // Verify all FinishReason variants exist
    let reasons = vec![
        FinishReason::Stop,
        FinishReason::MaxTokens,
        FinishReason::Timeout,
        FinishReason::ContentFiltered,
    ];

    assert_eq!(reasons.len(), 4, "Should have 4 finish reasons");
}

#[test]
fn inference_params_valid_values() {
    let params = InferenceParams {
        max_tokens: 100,
        temperature: 2.5, // High temperature is allowed
        top_p: 1.0,
        top_k: 50,
        stream: false,
        timeout_ms: None,
    };

    // Temperature should be usable even if high
    assert!(params.temperature > 0.0);
    assert!(params.max_tokens > 0);
}

#[test]
fn inference_params_typical_defaults() {
    let params = InferenceParams {
        max_tokens: 256,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        stream: false,
        timeout_ms: None,
    };

    assert!(params.max_tokens > 0);
    assert!(params.temperature >= 0.0 && params.temperature <= 2.0);
    assert!(params.top_p >= 0.0 && params.top_p <= 1.0);
    assert!(params.top_k > 0);
}

#[test]
fn chat_message_structure_valid() {
    let message = ChatMessage {
        role: ChatRole::User,
        content: "Hello, assistant!".to_string(),
    };

    assert_eq!(message.role, ChatRole::User);
    assert!(!message.content.is_empty());
}

#[test]
fn chat_roles_all_variants() {
    let roles = vec![
        ChatRole::System,
        ChatRole::User,
        ChatRole::Assistant,
    ];

    assert_eq!(roles.len(), 3, "Should have 3 chat roles");
}

#[test]
fn inference_output_generation_identification() {
    let generation = GenerationResult {
        text: "Output".to_string(),
        tokens_generated: 1,
        finish_reason: FinishReason::Stop,
    };
    let output = InferenceOutput::Generation(generation);

    assert!(!output.is_classification());
    assert!(output.is_generation());
    assert!(!output.is_embedding());
}

#[test]
fn max_tokens_param_respected() {
    // Test that params structure supports max_tokens
    let params = InferenceParams {
        max_tokens: 10,
        temperature: 0.0,
        top_p: 1.0,
        top_k: 1,
        stream: false,
        timeout_ms: None,
    };

    assert_eq!(params.max_tokens, 10);
}

// Memory-mapped loading tests

fn create_test_file_for_mmap(test_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("core_runtime_mmap_{}", test_name));
    let models_dir = base.join("models");
    std::fs::create_dir_all(&models_dir).expect("Failed to create test directory");

    let test_file = models_dir.join("test_model.bin");
    let test_data = b"GGUF test model data for memory-mapped loading test";
    std::fs::write(&test_file, test_data).expect("Failed to create test file");

    (base, test_file)
}

#[test]
fn mmap_load_valid_file() {
    let (base, _test_file) = create_test_file_for_mmap("load_valid");
    let loader = ModelLoader::new(base.clone());

    let model_path = loader.validate_path("models/test_model.bin")
        .expect("Should validate test file path");
    let mapped = loader.load_mapped(&model_path);

    assert!(mapped.is_ok(), "Should successfully map existing file");

    // Cleanup
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn mmap_load_missing_file() {
    let base = std::env::temp_dir().join("core_runtime_mmap_missing");
    std::fs::create_dir_all(base.join("models")).ok();
    let loader = ModelLoader::new(base.clone());

    // validate_path will fail for missing file
    let result = loader.validate_path("models/nonexistent.bin");
    assert!(result.is_err(), "Should fail for non-existent file");

    // Cleanup
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn mmap_data_accessible() {
    let (base, _test_file) = create_test_file_for_mmap("data_accessible");
    let loader = ModelLoader::new(base.clone());

    let model_path = loader.validate_path("models/test_model.bin")
        .expect("Should validate test file path");
    let mapped = loader.load_mapped(&model_path)
        .expect("Should map file");

    let data = mapped.as_bytes();
    assert!(data.starts_with(b"GGUF"), "Should be able to read mapped data");

    // Cleanup
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn mmap_len_matches_file_size() {
    let (base, test_file) = create_test_file_for_mmap("len_matches");
    let loader = ModelLoader::new(base.clone());

    let model_path = loader.validate_path("models/test_model.bin")
        .expect("Should validate test file path");
    let mapped = loader.load_mapped(&model_path)
        .expect("Should map file");

    let file_size = std::fs::metadata(&test_file)
        .expect("Should read file metadata").len();

    assert_eq!(mapped.len(), file_size as usize,
        "Mapped length should match file size");
    assert!(!mapped.is_empty(), "Mapped model should not be empty");

    // Cleanup
    std::fs::remove_dir_all(&base).ok();
}
