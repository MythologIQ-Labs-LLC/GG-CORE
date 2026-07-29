//! Manifest-driven ONNX loader dispatch.
//!
//! Separates the loader *decision* (`plan_onnx_load`, pure and total) from the
//! *effect* (`load_onnx_from_manifest`, which performs model IO). The decision
//! is fully unit-testable without a model file or the `onnx` feature.

use std::path::Path;
use std::sync::Arc;

use crate::engine::error::InferenceError;
use crate::models::manifest::{ModelArchitecture, ModelCapability, ModelManifest};

#[cfg(feature = "onnx")]
use super::{load_onnx_classifier, load_onnx_model};
use super::{OnnxConfig, OnnxModel};

/// The loader a manifest resolves to, with its required inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnnxLoadPlan {
    /// Sequence-classification model bound to ordered labels.
    Classifier(Vec<String>),
    /// Embedding model.
    Embedder,
}

/// Decide which ONNX loader a manifest selects, without performing IO.
///
/// Fails loud (returns `Err`) when the manifest is not ONNX architecture,
/// declares no ONNX-servable capability, declares more than one (ambiguous),
/// or declares `TextClassification` without non-empty `labels`.
pub fn plan_onnx_load(manifest: &ModelManifest) -> Result<OnnxLoadPlan, InferenceError> {
    if manifest.architecture != ModelArchitecture::Onnx {
        return Err(InferenceError::ModelError(format!(
            "manifest architecture is {:?}, expected Onnx",
            manifest.architecture
        )));
    }
    let classify = manifest.has_capability(ModelCapability::TextClassification);
    let embed = manifest.has_capability(ModelCapability::Embedding);
    match (classify, embed) {
        (true, true) => Err(InferenceError::ModelError(
            "manifest declares both TextClassification and Embedding; \
             ONNX dispatch is ambiguous"
                .into(),
        )),
        (true, false) => match &manifest.labels {
            Some(labels) if !labels.is_empty() => Ok(OnnxLoadPlan::Classifier(labels.clone())),
            _ => Err(InferenceError::ModelError(
                "TextClassification manifest requires non-empty `labels` \
                 for ONNX classifier dispatch"
                    .into(),
            )),
        },
        (false, true) => Ok(OnnxLoadPlan::Embedder),
        (false, false) => Err(InferenceError::ModelError(
            "manifest declares no ONNX-servable capability \
             (TextClassification or Embedding)"
                .into(),
        )),
    }
}

/// Dispatch: load the ONNX model a manifest selects, using its declared
/// capability + labels. The decision is delegated to `plan_onnx_load`; this
/// wrapper only performs the IO.
#[cfg(feature = "onnx")]
pub fn load_onnx_from_manifest(
    manifest: &ModelManifest,
    path: &Path,
    config: &OnnxConfig,
) -> Result<Arc<dyn OnnxModel>, InferenceError> {
    match plan_onnx_load(manifest)? {
        OnnxLoadPlan::Classifier(labels) => {
            load_onnx_classifier(path, &manifest.model_id, labels, config)
        }
        OnnxLoadPlan::Embedder => load_onnx_model(path, &manifest.model_id, config),
    }
}

/// Stub for non-onnx builds. The decision (`plan_onnx_load`) still runs so a
/// malformed manifest fails loud with the same message; only the load is gated
/// out.
#[cfg(not(feature = "onnx"))]
pub fn load_onnx_from_manifest(
    manifest: &ModelManifest,
    _path: &Path,
    _config: &OnnxConfig,
) -> Result<Arc<dyn OnnxModel>, InferenceError> {
    plan_onnx_load(manifest)?;
    Err(InferenceError::ModelError(
        "ONNX support not compiled in. Enable 'onnx' feature.".into(),
    ))
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
