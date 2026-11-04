//! Manifest-driven backend dispatch for the production model-load path.
//!
//! Resolves an optional sibling `manifest.json` next to a model file and selects
//! the backend. Absent manifest → GGUF (every existing GGUF load ships no
//! manifest and is unchanged). A manifest that is *present* must parse and
//! validate, or the load fails: a malformed manifest never silently downgrades
//! to "no integrity expectations". Only `architecture: onnx` routes to the ONNX
//! manifest dispatcher (B-29a); both loaders return `Arc<dyn Model>` (B-29b-1),
//! so the registry/lifecycle never cares which backend produced the model.

use std::path::Path;
use std::sync::Arc;

use crate::engine::error::InferenceError;
use crate::engine::gguf::{load_gguf_model, GgufConfig};
use crate::engine::onnx::{load_onnx_from_manifest, OnnxConfig};
use crate::engine::Model;
use crate::models::integrity::{check_identity_unchanged, verify_manifest_hash, FileIdentity};
use crate::models::manifest::{ModelArchitecture, ModelManifest};

/// Which backend a manifest selects. `Onnx` carries the parsed
/// manifest forward so the loader needs no `unwrap`/`expect`.
#[derive(Debug)]
pub enum BackendChoice {
    /// A sibling manifest declares ONNX architecture.
    Onnx(Box<ModelManifest>),
    /// No manifest, or a non-ONNX manifest — load as GGUF (default behavior).
    GgufDefault,
}

/// Decide the backend from an optional manifest, without IO. Consumes the
/// manifest, moving it into the `Onnx` variant when it applies.
pub fn choose_backend(manifest: Option<ModelManifest>) -> BackendChoice {
    match manifest {
        Some(m) if m.architecture == ModelArchitecture::Onnx => BackendChoice::Onnx(Box::new(m)),
        _ => BackendChoice::GgufDefault,
    }
}

/// Read the sibling `manifest.json`: `None` only when it does not exist.
/// Present-but-unreadable, malformed, or invalid manifests are errors.
pub fn read_sibling_manifest(model_path: &Path) -> Result<Option<ModelManifest>, InferenceError> {
    let manifest_path = model_path.with_file_name("manifest.json");
    match std::fs::symlink_metadata(&manifest_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(InferenceError::ModelError(format!(
                "manifest present but unreadable ({}): {}",
                e,
                manifest_path.display()
            )))
        }
        Ok(_) => {}
    }
    let manifest = ModelManifest::from_file(&manifest_path)?;
    manifest.validate()?;
    Ok(Some(manifest))
}

/// Load the backend a model file selects, resolving an optional sibling
/// `manifest.json`. A parsed manifest's declared `sha256` is verified against
/// the file BEFORE any backend reads it, and the file's identity is re-checked
/// AFTER the backend loaded it by path, so a substitution in between fails the
/// load (nothing is registered; the model is dropped).
pub fn load_model_dispatch(
    model_path: &Path,
    model_id: &str,
) -> Result<Arc<dyn Model>, InferenceError> {
    let manifest = read_sibling_manifest(model_path)?;
    let hashed: Option<FileIdentity> = match &manifest {
        Some(m) => Some(verify_manifest_hash(model_path, m)?),
        None => None,
    };
    let model = match choose_backend(manifest) {
        BackendChoice::Onnx(m) => load_onnx_from_manifest(&m, model_path, &OnnxConfig::default()),
        BackendChoice::GgufDefault => load_gguf_model(model_path, model_id, &GgufConfig::default()),
    }?;
    if let Some(identity) = hashed {
        check_identity_unchanged(model_path, &identity)?;
    }
    Ok(model)
}

#[cfg(test)]
#[path = "backend_dispatch_tests.rs"]
mod tests;
