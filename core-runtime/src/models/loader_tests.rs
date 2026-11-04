//! Unit tests for lexical path normalization and allowlist validation.

use super::*;

#[test]
fn test_normalize_path() {
    assert_eq!(
        normalize_path(Path::new("/var/models/ok.bin")).unwrap(),
        PathBuf::from("/var/models/ok.bin")
    );
    assert_eq!(
        normalize_path(Path::new("/var/models/../tokenizers/ok.json")).unwrap(),
        PathBuf::from("/var/tokenizers/ok.json")
    );
    assert_eq!(
        normalize_path(Path::new("relative/../path")).unwrap(),
        PathBuf::from("path")
    );
    // Traversal above root should fail
    assert_eq!(normalize_path(Path::new("/../etc/passwd")), None);
    // Traversal above relative should fail
    assert_eq!(normalize_path(Path::new("../../etc/passwd")), None);
}

#[test]
fn test_validate_path() {
    let loader = ModelLoader::new(PathBuf::from("/base"));

    // Valid paths within allowed directories
    assert!(loader.validate_path("models/model1.bin").is_ok());
    assert!(loader.validate_path("tokenizers/tok1.json").is_ok());

    // Allowed paths using safe traversals
    assert!(loader
        .validate_path("models/../tokenizers/tok1.json")
        .is_ok());

    // Invalid paths (outside allowed directories)
    assert!(loader.validate_path("other/file.bin").is_err());
    assert!(loader.validate_path("../models.bin").is_err());

    // Dangerous traversal attempts
    assert!(loader.validate_path("models/../../etc/passwd").is_err());
    assert!(loader.validate_path("../../../etc/shadow").is_err());

    // NUL-byte injection is rejected at the seam (FFI truncation class).
    assert!(loader
        .validate_path("models/test\0../../etc/passwd")
        .is_err());
}
