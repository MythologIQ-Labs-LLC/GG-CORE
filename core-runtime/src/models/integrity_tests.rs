//! Tests for manifest hash verification and load-time file identity.

use super::*;
use crate::models::manifest::{ModelArchitecture, ModelCapability};

// sha256("hello world") — fixed vector.
const HELLO_SHA: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

fn manifest_with(sha256: &str) -> ModelManifest {
    ModelManifest {
        model_id: "m".into(),
        name: "m".into(),
        version: "1.0.0".into(),
        capabilities: vec![ModelCapability::TextGeneration],
        sha256: sha256.into(),
        size_bytes: 11,
        architecture: ModelArchitecture::Gguf,
        license: "Apache-2.0".into(),
        labels: None,
    }
}

fn hello_file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let p = dir.path().join("model.gguf");
    std::fs::write(&p, b"hello world").unwrap();
    p
}

#[test]
fn matching_hash_passes() {
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    assert_eq!(file_sha256_hex(&p).unwrap(), HELLO_SHA);
    verify_manifest_hash(&p, &manifest_with(HELLO_SHA)).unwrap();
}

#[test]
fn uppercase_declared_hash_passes() {
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    verify_manifest_hash(&p, &manifest_with(&HELLO_SHA.to_uppercase())).unwrap();
}

#[test]
fn mismatched_hash_fails_loud() {
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    let bad = "a".repeat(64);
    let err = verify_manifest_hash(&p, &manifest_with(&bad)).unwrap_err();
    assert!(err.to_string().contains("hash mismatch"), "got: {err}");
}

#[test]
fn tampered_file_fails_after_manifest_written() {
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    std::fs::write(&p, b"hello w0rld").unwrap(); // tamper
    let err = verify_manifest_hash(&p, &manifest_with(HELLO_SHA)).unwrap_err();
    assert!(err.to_string().contains("hash mismatch"));
}

#[test]
fn missing_file_fails_loud() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("absent.gguf");
    assert!(file_sha256_hex(&p).is_err());
}

#[test]
fn identity_is_stable_while_the_file_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    let identity = verify_manifest_hash(&p, &manifest_with(HELLO_SHA)).unwrap();
    check_identity_unchanged(&p, &identity).unwrap();
}

#[test]
fn identity_detects_a_file_swapped_in_after_hashing() {
    // Hash → attacker renames a different file over the path → load.
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    let identity = verify_manifest_hash(&p, &manifest_with(HELLO_SHA)).unwrap();
    let other = dir.path().join("other.gguf");
    std::fs::write(&other, b"hello world").unwrap(); // same bytes, different file
    std::fs::rename(&other, &p).unwrap();
    let err = check_identity_unchanged(&p, &identity).unwrap_err();
    assert!(
        err.to_string().contains("changed during load"),
        "got: {err}"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_model_file_is_refused_for_hashing() {
    let dir = tempfile::tempdir().unwrap();
    let real = hello_file(&dir);
    let link = dir.path().join("link.gguf");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = file_sha256_hex(&link).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "got: {err}");
}

#[cfg(unix)]
#[test]
fn identity_detects_a_symlink_swapped_in_after_hashing() {
    let dir = tempfile::tempdir().unwrap();
    let p = hello_file(&dir);
    let identity = verify_manifest_hash(&p, &manifest_with(HELLO_SHA)).unwrap();
    let elsewhere = dir.path().join("elsewhere.bin");
    std::fs::write(&elsewhere, b"hello world").unwrap();
    std::fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &p).unwrap();
    assert!(check_identity_unchanged(&p, &identity).is_err());
}
