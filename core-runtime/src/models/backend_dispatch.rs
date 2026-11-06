//! Manifest-driven backend dispatch for the production model-load path.
//!
//! Resolves an optional sibling `manifest.json` next to a model file and
//! selects a supported backend explicitly. A missing manifest preserves the
//! historical GGUF default for GGUF files; manifest-driven ONNX embedding and
//! classification are dispatched separately. Unsupported formats and
//! incompatible declared capabilities fail closed, never silently masquerade
//! as GGUF. Backends return `Arc<dyn Model>` (B-29b-1), keeping the model
//! lifecycle neutral to architecture.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use crate::engine::error::InferenceError;
use crate::engine::gguf::{load_gguf_model, GgufConfig};
use crate::engine::onnx::{load_onnx_from_manifest, plan_onnx_load, OnnxConfig};
use crate::engine::Model;
use crate::models::integrity::{check_identity_unchanged, verify_manifest_hash, FileIdentity};
use crate::models::manifest::{ModelArchitecture, ModelManifest};

/// Selected model backend. A manifest declaring an unknown serving format
/// MUST NOT downgrade into a different backend. Explicit GGUF manifests are
/// retained to validate their claimed capabilities before any model IO.
#[derive(Debug)]
pub enum BackendChoice {
    /// ONNX capability + labels must pass the ONNX load planner.
    Onnx(Box<ModelManifest>),
    /// GGUF with a manifest; validate the declared served capabilities.
    GgufManifest(Box<ModelManifest>),
    /// Historical, manifest-free GGUF path.
    GgufDefault,
    /// Known file type with no implemented inference backend.
    Unsupported(ModelArchitecture),
}

/// Pure, manifest-driven architecture routing. Qwen is only one model family
/// using GGUF, not the backend's identity or default model.
pub fn choose_backend(manifest: Option<ModelManifest>) -> BackendChoice {
    match manifest {
        None => BackendChoice::GgufDefault,
        Some(m) => match m.architecture {
            ModelArchitecture::Onnx => BackendChoice::Onnx(Box::new(m)),
            ModelArchitecture::Gguf => BackendChoice::GgufManifest(Box::new(m)),
            ModelArchitecture::SafeTensors => {
                BackendChoice::Unsupported(ModelArchitecture::SafeTensors)
            }
        },
    }
}

/// GGUF inference currently exposes text generation, not arbitrary manifest
/// capabilities. Fail closed rather than silently registering a GGUF model
/// as an embedder or classifier. Future added GGUF capabilities must earn
/// their own real-model serving-path qualification and update this contract.
fn validate_gguf_manifest(manifest: &ModelManifest) -> Result<(), InferenceError> {
    if manifest.capabilities.as_slice()
        != [crate::models::manifest::ModelCapability::TextGeneration]
    {
        return Err(InferenceError::ModelError(
            "GGUF serving currently supports exactly text_generation; manifest declares unsupported GGUF capabilities".into(),
        ));
    }
    Ok(())
}

/// Read a bounded, local manifest without following final symlinks.
///
/// Each model may now have its own adjacent `<filename>.manifest.json`
/// (e.g., `qwen.gguf.manifest.json`). This supports multiple independently
/// hashed GGUF/ONNX models in one allowed directory. A model-specific file
/// takes precedence; otherwise legacy single-model `manifest.json` remains
/// supported. A malformed/present dedicated file never falls back to legacy.
pub fn read_sibling_manifest(model_path: &Path) -> Result<Option<ModelManifest>, InferenceError> {
    const MAX_MANIFEST_BYTES: u64 = 1_048_576;
    let Some(file_name) = model_path.file_name() else {
        return Err(InferenceError::ModelError(
            "model path has no file name".into(),
        ));
    };
    let mut specific_name = file_name.to_os_string();
    specific_name.push(".manifest.json");
    let dedicated = model_path.with_file_name(specific_name);
    let legacy = model_path.with_file_name("manifest.json");

    // Metadata lookup distinguishes an absent dedicated manifest from a
    // present-but-invalid one; the latter must never downgrade to legacy.
    let manifest_path = match std::fs::symlink_metadata(&dedicated) {
        Ok(_) => dedicated,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => legacy,
        Err(e) => {
            return Err(InferenceError::ModelError(format!(
                "model-specific manifest cannot be inspected: {e}"
            )));
        }
    };
    let meta = match std::fs::symlink_metadata(&manifest_path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(InferenceError::ModelError(format!(
                "manifest present but unreadable: {e}"
            )));
        }
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(InferenceError::ModelError(
            "manifest must be a regular non-symlink file".into(),
        ));
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(InferenceError::ModelError(
            "model manifest exceeds 1 MiB metadata limit".into(),
        ));
    }

    // The final-component no-follow open also prevents Unix symlink swaps
    // between the metadata check and open. Windows reparse/path race
    // qualification remains a required native security gate.
    let file = crate::models::integrity::open_nofollow(&manifest_path)?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| InferenceError::ModelError(format!("cannot read model manifest: {e}")))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(InferenceError::ModelError(
            "model manifest exceeds 1 MiB metadata limit".into(),
        ));
    }
    let json = std::str::from_utf8(&bytes)
        .map_err(|_| InferenceError::ModelError("model manifest is not UTF-8".into()))?;
    let manifest = ModelManifest::from_json(json)?;
    manifest.validate()?;
    Ok(Some(manifest))
}

/// Map the already parsed daemon settings into actual model-load settings.
/// This stays pure so the contract can be tested without mutating the test
/// process's environment or loading a large native GGUF fixture.
fn gguf_load_config(env: &crate::config::GgufEnvConfig) -> GgufConfig {
    GgufConfig {
        n_ctx: env.n_ctx,
        n_threads: env.n_threads,
        ..GgufConfig::default()
    }
}

/// Load the backend a model file selects, resolving a model-specific or
/// legacy sibling `manifest.json`. A parsed manifest's declared `sha256` is verified against
/// the file BEFORE any backend reads it, and the file's identity is re-checked
/// AFTER the backend loaded it by path, so a substitution in between fails the
/// load (nothing is registered; the model is dropped).
pub fn load_model_dispatch(
    model_path: &Path,
    model_id: &str,
) -> Result<Arc<dyn Model>, InferenceError> {
    let manifest = read_sibling_manifest(model_path)?;
    let choice = choose_backend(manifest);

    // Do not fall through to the GGUF loader just because a non-GGUF model
    // lacks the manifest required to describe its architecture/capability.
    if matches!(&choice, BackendChoice::GgufDefault)
        && model_path
            .extension()
            .and_then(|v| v.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("onnx") || ext.eq_ignore_ascii_case("safetensors")
            })
    {
        return Err(InferenceError::ModelError(
            "non-GGUF model requires a validated sibling manifest.json and a supported backend"
                .into(),
        ));
    }

    // Reject unsupported declarations before hashing/loading untrusted
    // model bytes. ONNX must be a supported embedding/classifier plan;
    // text generation remains unimplemented until independently qualified.
    match &choice {
        BackendChoice::Unsupported(arch) => {
            return Err(InferenceError::ModelError(format!(
                "unsupported model backend: {arch:?}; GGUF text generation and ONNX embedding/classification are implemented paths"
            )));
        }
        BackendChoice::GgufManifest(m) => validate_gguf_manifest(m)?,
        BackendChoice::Onnx(m) => {
            plan_onnx_load(m)?;
        }
        BackendChoice::GgufDefault => {}
    }

    let hashed: Option<FileIdentity> = match &choice {
        BackendChoice::Onnx(m) | BackendChoice::GgufManifest(m) => {
            Some(verify_manifest_hash(model_path, m)?)
        }
        BackendChoice::GgufDefault | BackendChoice::Unsupported(_) => None,
    };
    let model = match choice {
        BackendChoice::Onnx(m) => load_onnx_from_manifest(&m, model_path, &OnnxConfig::default()),
        BackendChoice::GgufManifest(_) | BackendChoice::GgufDefault => {
            // Both manifest-free and verified GGUF models use the same
            // configured loader, with no model-family-specific behavior.
            let env = crate::config::load_gguf_config();
            let gguf = gguf_load_config(&env);
            load_gguf_model(model_path, model_id, &gguf)
        }
        BackendChoice::Unsupported(_) => unreachable!("unsupported backend rejected before load"),
    }?;
    if let Some(identity) = hashed {
        check_identity_unchanged(model_path, &identity)?;
    }
    Ok(model)
}

#[cfg(test)]
#[path = "backend_dispatch_tests.rs"]
mod tests;

/// The daemon's advertised context and thread env settings must actually
/// reach the native GGUF loader, not just its diagnostic output.
#[cfg(test)]
mod gguf_config_dispatch_tests {
    use super::*;

    #[test]
    fn configured_context_and_threads_reach_the_model_loader() {
        let runtime = crate::config::GgufEnvConfig {
            n_ctx: 8192,
            n_threads: 6,
        };
        let load = gguf_load_config(&runtime);
        assert_eq!(load.n_ctx, 8192);
        assert_eq!(load.n_threads, 6);
        assert_eq!(load.n_gpu_layers, 0);
    }
}
