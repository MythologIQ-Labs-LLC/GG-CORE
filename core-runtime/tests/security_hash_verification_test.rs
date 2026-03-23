//! Security tests for hash verification.
//!
//! Tests that model integrity verification correctly accepts valid hashes
//! and rejects invalid, malformed, or mismatched hashes.

use gg_core::models::manifest::{ModelManifest, ModelCapability, ModelArchitecture};

fn create_test_manifest(sha256: &str) -> ModelManifest {
    ModelManifest {
        model_id: "test-model".to_string(),
        name: "Test Model".to_string(),
        version: "1.0.0".to_string(),
        capabilities: vec![ModelCapability::TextClassification],
        sha256: sha256.to_string(),
        size_bytes: 1024,
        architecture: ModelArchitecture::Onnx,
        license: "MIT".to_string(),
    }
}

#[test]
fn accept_valid_64_char_hash() {
    // Valid 64-character hex SHA-256 hash
    let valid_hash = "a".repeat(64);
    let manifest = create_test_manifest(&valid_hash);

    let result = manifest.validate();
    assert!(result.is_ok());
}

#[test]
fn reject_short_hash() {
    // Hash shorter than 64 characters
    let short_hash = "abcd1234";
    let manifest = create_test_manifest(short_hash);

    let result = manifest.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("64 hex characters"));
}

#[test]
fn reject_long_hash() {
    // Hash longer than 64 characters
    let long_hash = "a".repeat(65);
    let manifest = create_test_manifest(&long_hash);

    let result = manifest.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("64 hex characters"));
}

#[test]
fn reject_empty_hash() {
    let manifest = create_test_manifest("");

    let result = manifest.validate();
    assert!(result.is_err());
}

#[test]
fn error_contains_hash_info() {
    let invalid_hash = "too_short";
    let manifest = create_test_manifest(invalid_hash);

    let result = manifest.validate();
    assert!(result.is_err());

    let err_msg = result.unwrap_err().to_string();
    // Error should indicate what's wrong
    assert!(
        err_msg.contains("sha256") || err_msg.contains("64"),
        "Error should mention sha256 requirement: {}", err_msg
    );
}

#[test]
fn accept_lowercase_hex_hash() {
    let lowercase_hash = "0123456789abcdef".repeat(4); // 64 chars
    let manifest = create_test_manifest(&lowercase_hash);

    let result = manifest.validate();
    assert!(result.is_ok());
}

#[test]
fn accept_uppercase_hex_hash() {
    let uppercase_hash = "0123456789ABCDEF".repeat(4); // 64 chars
    let manifest = create_test_manifest(&uppercase_hash);

    let result = manifest.validate();
    assert!(result.is_ok());
}

#[test]
fn manifest_from_json_valid() {
    let json = r#"{
        "model_id": "test-classifier",
        "name": "Test Classifier",
        "version": "1.0.0",
        "capabilities": ["text_classification"],
        "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "size_bytes": 65536,
        "architecture": "onnx",
        "license": "MIT"
    }"#;

    let result = ModelManifest::from_json(json);
    assert!(result.is_ok());

    let manifest = result.unwrap();
    assert_eq!(manifest.model_id, "test-classifier");
    assert!(manifest.validate().is_ok());
}

#[test]
fn manifest_from_json_invalid() {
    let invalid_json = r#"{ "not": "valid manifest" }"#;

    let result = ModelManifest::from_json(invalid_json);
    assert!(result.is_err());
}

#[test]
fn reject_empty_model_id() {
    let mut manifest = create_test_manifest(&"a".repeat(64));
    manifest.model_id = String::new();

    let result = manifest.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("model_id"));
}

#[test]
fn reject_empty_capabilities() {
    let mut manifest = create_test_manifest(&"a".repeat(64));
    manifest.capabilities = vec![];

    let result = manifest.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("capabilities"));
}
