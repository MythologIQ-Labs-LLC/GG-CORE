//! Synthetic artifact custody controls; no real model or qualification claim.

use super::*;
use crate::models::integrity::file_sha256_hex;
use std::path::PathBuf;

struct Fixture {
    _root: tempfile::TempDir,
    loader: ModelLoader,
    model: PathBuf,
    sidecar: PathBuf,
    approved: ModelManifest,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let models = root.path().join("models");
        std::fs::create_dir(&models).unwrap();
        let model = models.join("m.gguf");
        std::fs::write(&model, b"hello world").unwrap();
        let approved = ModelManifest {
            model_id: "m".into(),
            name: "Model".into(),
            version: "1.0.0".into(),
            capabilities: vec![ModelCapability::TextGeneration],
            sha256: file_sha256_hex(&model).unwrap(),
            size_bytes: 11,
            architecture: ModelArchitecture::Gguf,
            license: "Apache-2.0".into(),
            labels: None,
        };
        let sidecar = models.join("m.gguf.manifest.json");
        std::fs::write(&sidecar, serde_json::to_vec(&approved).unwrap()).unwrap();
        let loader = ModelLoader::new(root.path().to_path_buf());
        Self {
            _root: root,
            loader,
            model,
            sidecar,
            approved,
        }
    }

    fn verify(&self) -> Result<PinnedGgufArtifact, InferenceError> {
        verify_pinned_gguf_artifact(&self.loader, "models/m.gguf", &self.approved)
    }

    fn write_manifest(&self, manifest: &ModelManifest) {
        std::fs::write(&self.sidecar, serde_json::to_vec(manifest).unwrap()).unwrap();
    }
}

#[test]
fn matching_approved_manifest_and_bytes_pass() {
    let f = Fixture::new();
    let artifact = f.verify().unwrap();
    assert_eq!(artifact.manifest(), &f.approved);
    artifact.recheck_path_identity().unwrap();
}

#[test]
fn replaced_bytes_and_matching_unapproved_manifest_fail() {
    let f = Fixture::new();
    std::fs::write(&f.model, b"hello w0rld").unwrap();
    let mut substituted = f.approved.clone();
    substituted.sha256 = file_sha256_hex(&f.model).unwrap();
    f.write_manifest(&substituted);
    verify_manifest_hash(&f.model, &substituted).unwrap();
    let error = f.verify().unwrap_err().to_string();
    assert!(error.contains("differs from approved manifest"), "{error}");
}

#[test]
fn modified_metadata_with_same_weights_fails() {
    let f = Fixture::new();
    let mut substituted = f.approved.clone();
    substituted.model_id = "another-model".into();
    f.write_manifest(&substituted);
    assert!(f.verify().is_err());
    substituted = f.approved.clone();
    substituted.license = "Other".into();
    f.write_manifest(&substituted);
    assert!(f.verify().is_err());
}

#[test]
fn missing_manifest_does_not_become_approved() {
    let f = Fixture::new();
    std::fs::remove_file(&f.sidecar).unwrap();
    assert!(f.verify().is_err());
}

#[test]
fn bad_bytes_fail_even_with_matching_manifest() {
    let f = Fixture::new();
    std::fs::write(&f.model, b"hello w0rld").unwrap();
    assert!(f.verify().unwrap_err().to_string().contains("hash mismatch"));
}

#[test]
fn inconsistent_size_fails_even_with_matching_pin() {
    let mut f = Fixture::new();
    f.approved.size_bytes = 12;
    f.write_manifest(&f.approved);
    assert!(f.verify().unwrap_err().to_string().contains("size mismatch"));
}

#[test]
fn unsupported_architecture_and_task_fail() {
    let mut f = Fixture::new();
    f.approved.architecture = ModelArchitecture::Onnx;
    f.write_manifest(&f.approved);
    assert!(f.verify().is_err());
    f.approved.architecture = ModelArchitecture::Gguf;
    f.approved.capabilities = vec![ModelCapability::Embedding];
    f.write_manifest(&f.approved);
    assert!(f.verify().is_err());
}

#[test]
fn disallowed_path_and_directory_fail() {
    let f = Fixture::new();
    let rejected = verify_pinned_gguf_artifact(&f.loader, "../m.gguf", &f.approved);
    assert!(rejected.is_err());
    std::fs::remove_file(&f.model).unwrap();
    std::fs::create_dir(&f.model).unwrap();
    assert!(f.verify().is_err());
}

#[test]
fn recheck_refuses_different_file_with_identical_bytes() {
    let f = Fixture::new();
    let artifact = f.verify().unwrap();
    let other = f.model.with_file_name("other.gguf");
    std::fs::write(&other, b"hello world").unwrap();
    std::fs::remove_file(&f.model).unwrap();
    std::fs::rename(&other, &f.model).unwrap();
    assert!(artifact.recheck_path_identity().is_err());
}

#[test]
fn dedicated_invalid_manifest_does_not_downgrade_to_legacy() {
    let f = Fixture::new();
    let legacy = f.model.with_file_name("manifest.json");
    std::fs::write(&legacy, serde_json::to_vec(&f.approved).unwrap()).unwrap();
    std::fs::write(&f.sidecar, b"invalid json").unwrap();
    assert!(f.verify().is_err());
    std::fs::remove_file(&f.sidecar).unwrap();
    assert!(f.verify().is_ok());
}
