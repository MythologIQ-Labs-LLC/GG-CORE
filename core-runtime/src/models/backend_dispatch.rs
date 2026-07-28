//! Manifest-driven backend dispatch for the production model-load path.
//!
//! Resolves an optional sibling `manifest.json` next to a model file and selects
//! the GGUF or ONNX loader from its declared architecture. Absent, unparseable,
//! or non-ONNX manifests default to GGUF — every existing GGUF load (which ships
//! no manifest) is unchanged. Only `architecture: onnx` routes to the ONNX
//! manifest dispatcher (B-29a); both loaders return `Arc<dyn Model>` (B-29b-1),
//! so either arm flows into the lifecycle unchanged.

use std::path::Path;
use std::sync::Arc;

use crate::engine::gguf::{load_gguf_model, GgufConfig};
use crate::engine::onnx::{load_onnx_from_manifest, OnnxConfig};
use crate::engine::{InferenceError, Model};
use crate::models::manifest::{ModelArchitecture, ModelManifest};

/// Which backend a model file resolves to. The ONNX variant carries the parsed
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

/// Load the backend a model file selects, resolving an optional sibling
/// `manifest.json`. Absent/unparseable/non-ONNX manifest → GGUF (unchanged
/// behavior); `architecture: onnx` → the ONNX manifest dispatcher (B-29a).
pub fn load_model_dispatch(
    model_path: &Path,
    model_id: &str,
) -> Result<Arc<dyn Model>, InferenceError> {
    let manifest = ModelManifest::from_file(&model_path.with_file_name("manifest.json")).ok();
    match choose_backend(manifest) {
        BackendChoice::Onnx(m) => load_onnx_from_manifest(&m, model_path, &OnnxConfig::default()),
        BackendChoice::GgufDefault => load_gguf_model(model_path, model_id, &GgufConfig::default()),
    }
}

#[cfg(test)]
#[path = "backend_dispatch_tests.rs"]
mod tests;
