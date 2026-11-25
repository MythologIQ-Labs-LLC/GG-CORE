//! ONNX inference backend using Candle.
//!
//! Provides classification and embedding models via pure Rust ONNX runtime.

mod classifier;
mod dispatch;
mod embedder;
#[cfg(feature = "onnx")]
mod tensor_ops;
#[cfg(feature = "onnx")]
mod tokenizer;

pub use classifier::OnnxClassifier;
pub use dispatch::{load_onnx_from_manifest, plan_onnx_load, OnnxLoadPlan};
pub use embedder::OnnxEmbedder;

use std::path::Path;
use std::sync::Arc;

use crate::engine::{InferenceError, Model};

/// Configuration for ONNX model loading.
#[derive(Debug, Clone)]
pub struct OnnxConfig {
    /// Maximum batch size for batched inference.
    pub max_batch_size: usize,
    /// Device to run inference on (cpu only for sandboxed runtime).
    pub device: OnnxDevice,
}

impl Default for OnnxConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 32,
            device: OnnxDevice::Cpu,
        }
    }
}

/// Device for ONNX inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnnxDevice {
    Cpu,
}

/// Default embedding dimension for MiniLM-style models.
#[cfg(feature = "onnx")]
const DEFAULT_EMBEDDING_DIM: usize = 384;

/// The configured limit cannot disable all batching or bypass the shared
/// 32-item input hard limit. Validate before opening potentially large models.
#[cfg(feature = "onnx")]
fn checked_embedding_batch_limit(max_batch_size: usize) -> Result<usize, InferenceError> {
    if !(1..=crate::engine::input::MAX_BATCH_SIZE).contains(&max_batch_size) {
        return Err(InferenceError::InputValidation(format!(
            "ONNX max_batch_size must be between 1 and {}",
            crate::engine::input::MAX_BATCH_SIZE
        )));
    }
    Ok(max_batch_size)
}

/// Load an ONNX model from a file path.
///
/// # Arguments
/// * `path` - Path to the .onnx model file
/// * `model_id` - Unique identifier for this model instance
/// * `config` - ONNX configuration options
///
/// # Errors
/// Returns error if model cannot be loaded or is invalid format.
#[cfg(feature = "onnx")]
pub fn load_onnx_model(
    path: &Path,
    model_id: &str,
    config: &OnnxConfig,
) -> Result<Arc<dyn Model>, InferenceError> {
    let batch_limit = checked_embedding_batch_limit(config.max_batch_size)?;
    let model = candle_onnx::read_file(path)
        .map_err(|e| InferenceError::ModelError(format!("load {path:?}: {e}")))?;
    let tok = tokenizer::OnnxTokenizer::for_model(path)?;
    let embedder =
        OnnxEmbedder::with_model(model_id.to_string(), DEFAULT_EMBEDDING_DIM, model, tok)
            .with_batch_limit(batch_limit);
    Ok(Arc::new(embedder))
}

/// Stub for non-onnx builds.
#[cfg(not(feature = "onnx"))]
pub fn load_onnx_model(
    _path: &Path,
    _model_id: &str,
    _config: &OnnxConfig,
) -> Result<Arc<dyn Model>, InferenceError> {
    Err(InferenceError::ModelError(
        "ONNX support not compiled in. Enable 'onnx' feature.".into(),
    ))
}

/// Load an ONNX sequence-classification model, returning a classifier bound to
/// the given ordered `labels` (label `i` corresponds to logit `i`).
///
/// # Errors
/// Returns error if the model cannot be loaded or is an invalid format.
#[cfg(feature = "onnx")]
pub fn load_onnx_classifier(
    path: &Path,
    model_id: &str,
    labels: Vec<String>,
    _config: &OnnxConfig,
) -> Result<Arc<dyn Model>, InferenceError> {
    let model = candle_onnx::read_file(path)
        .map_err(|e| InferenceError::ModelError(format!("load {path:?}: {e}")))?;
    let tok = tokenizer::OnnxTokenizer::for_model(path)?;
    let classifier = OnnxClassifier::with_model(model_id.to_string(), labels, model, tok);
    Ok(Arc::new(classifier))
}

/// Stub for non-onnx builds.
#[cfg(not(feature = "onnx"))]
pub fn load_onnx_classifier(
    _path: &Path,
    _model_id: &str,
    _labels: Vec<String>,
    _config: &OnnxConfig,
) -> Result<Arc<dyn Model>, InferenceError> {
    Err(InferenceError::ModelError(
        "ONNX support not compiled in. Enable 'onnx' feature.".into(),
    ))
}
