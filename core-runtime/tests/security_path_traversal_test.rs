//! Security tests for path traversal prevention.
//!
//! Tests that model loading rejects attempts to escape the allowed directories.

use gg_core::models::ModelLoader;

fn create_test_loader() -> ModelLoader {
    // Use a temporary directory as base for tests
    let base = std::env::temp_dir().join("core_runtime_test");
    std::fs::create_dir_all(base.join("models")).ok();
    ModelLoader::new(base)
}

#[test]
fn reject_relative_path_escape() {
    let loader = create_test_loader();

    // Attempt to escape via ../
    let result = loader.validate_path("../../../etc/passwd");
    assert!(result.is_err());

    // The error should indicate path is not allowed
    let err_str = result.unwrap_err().to_string();
    assert!(
        err_str.contains("not allowed") || err_str.contains("not found"),
        "Expected path rejection, got: {}", err_str
    );
}

#[test]
fn reject_double_dot_in_middle() {
    let loader = create_test_loader();

    // Attempt to escape via models/../../../etc/passwd
    let result = loader.validate_path("models/../../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn reject_absolute_path_unix() {
    let loader = create_test_loader();

    // Absolute path should be rejected (treated as relative to base)
    let result = loader.validate_path("/etc/passwd");
    assert!(result.is_err());
}

#[test]
fn reject_absolute_path_windows() {
    let loader = create_test_loader();

    // Windows-style absolute paths
    let result = loader.validate_path("C:\\Windows\\System32\\config\\SAM");
    assert!(result.is_err());
}

#[test]
fn reject_unc_paths() {
    let loader = create_test_loader();

    // UNC paths should be rejected
    let result = loader.validate_path("\\\\server\\share\\file");
    assert!(result.is_err());
}

#[test]
fn reject_url_encoded_traversal() {
    let loader = create_test_loader();

    // URL-encoded path traversal: %2e%2e%2f = ../
    // Note: This tests the raw string - actual URL decoding is caller's responsibility
    let result = loader.validate_path("%2e%2e%2f%2e%2e%2fetc/passwd");
    assert!(result.is_err());
}

#[test]
fn accept_valid_model_path() {
    // Use canonicalized base path to ensure consistent path handling
    let base = std::env::temp_dir().join("core_runtime_test_valid");
    std::fs::create_dir_all(base.join("models")).ok();

    // Create loader with canonicalized base path to match validation behavior
    let canonical_base = base.canonicalize().unwrap_or(base.clone());
    let loader = gg_core::models::ModelLoader::new(canonical_base);

    // Create a valid model file in the allowed directory
    let model_path = base.join("models").join("test_model.bin");
    std::fs::write(&model_path, b"test").ok();

    let result = loader.validate_path("models/test_model.bin");

    // Clean up
    std::fs::remove_file(&model_path).ok();
    std::fs::remove_dir(base.join("models")).ok();
    std::fs::remove_dir(&base).ok();

    // Should succeed for valid path within allowed directory
    assert!(result.is_ok());
}

#[test]
fn reject_null_byte_in_path() {
    let loader = create_test_loader();

    // Null byte injection attempt
    let result = loader.validate_path("models/test\0../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn allowed_dirs_enforced() {
    let loader = create_test_loader();
    let base = std::env::temp_dir().join("core_runtime_test");

    // Create a file outside allowed directories
    let outside_path = base.join("outside").join("secret.txt");
    std::fs::create_dir_all(base.join("outside")).ok();
    std::fs::write(&outside_path, b"secret").ok();

    let result = loader.validate_path("outside/secret.txt");

    // Clean up
    std::fs::remove_file(&outside_path).ok();
    std::fs::remove_dir(base.join("outside")).ok();

    // Should fail - not in allowed dirs (models, tokenizers)
    assert!(result.is_err());
}
