//! Model file integrity verification (closes SECURITY.md Known Gap 2).
//!
//! When a sibling `manifest.json` declares a `sha256`, the model file's actual
//! digest is computed (streaming) and compared before any backend touches the
//! bytes. A mismatch emits a critical `ModelHashMismatch` security event and
//! fails the load loudly. Models without a manifest are unchanged: there is
//! nothing declared to verify against (the path allowlist still applies).

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::engine::error::InferenceError;
use crate::models::manifest::ModelManifest;
use crate::telemetry::{log_security_event, SecurityEvent};

/// Compute the lowercase-hex SHA-256 of a file, streaming in 64 KiB chunks.
pub fn file_sha256_hex(path: &Path) -> Result<String, InferenceError> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| InferenceError::ModelError(format!("cannot open model for hashing: {}", e)))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| InferenceError::ModelError(format!("hashing read failed: {}", e)))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Verify the model file against the manifest's declared `sha256` (fail-loud).
pub fn verify_manifest_hash(
    model_path: &Path,
    manifest: &ModelManifest,
) -> Result<(), InferenceError> {
    let actual = file_sha256_hex(model_path)?;
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        log_security_event(
            SecurityEvent::ModelHashMismatch,
            "model file hash does not match its manifest",
            &[
                ("model_id", &manifest.model_id),
                ("path", &model_path.display().to_string()),
                ("declared", &manifest.sha256),
                ("actual", &actual),
            ],
        );
        return Err(InferenceError::ModelError(format!(
            "model hash mismatch for '{}': manifest declares sha256 {} but the file hashes to {}",
            manifest.model_id, manifest.sha256, actual
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::manifest::{ModelArchitecture, ModelCapability};

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

    // sha256("hello world") — fixed vector.
    const HELLO_SHA: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

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
}
