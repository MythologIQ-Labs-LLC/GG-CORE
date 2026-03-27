//! ONNX inference backend using Candle.
//!
//! Provides classification and embedding models via pure Rust ONNX runtime.

mod classifier;
mod embedder;

pub use classifier::OnnxClassifier;
pub use embedder::OnnxEmbedder;

use std::path::Path;
use std::sync::Arc;

use crate::engine::{InferenceCapability, InferenceConfig, InferenceError};
use crate::engine::{InferenceInput, InferenceOutput};

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

/// Shared trait for ONNX models.
#[async_trait::async_trait]
pub trait OnnxModel: Send + Sync {
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> &[InferenceCapability];
    fn memory_usage(&self) -> usize;

    async fn infer(
        &self,
        input: &InferenceInput,
        config: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError>;

    async fn unload(&mut self) -> Result<(), InferenceError>;
}

/// Default embedding dimension for MiniLM-style models.
#[cfg(feature = "onnx")]
const DEFAULT_EMBEDDING_DIM: usize = 384;

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
    _config: &OnnxConfig,
) -> Result<Arc<dyn OnnxModel>, InferenceError> {
    let model = candle_onnx::read_file(path)
        .map_err(|e| InferenceError::ModelError(format!("load {path:?}: {e}")))?;
    let embedder = OnnxEmbedder::with_model(
        model_id.to_string(),
        DEFAULT_EMBEDDING_DIM,
        model,
    );
    Ok(Arc::new(embedder))
}

/// Stub for non-onnx builds.
#[cfg(not(feature = "onnx"))]
pub fn load_onnx_model(
    _path: &Path,
    _model_id: &str,
    _config: &OnnxConfig,
) -> Result<Arc<dyn OnnxModel>, InferenceError> {
    Err(InferenceError::ModelError(
        "ONNX support not compiled in. Enable 'onnx' feature.".into(),
    ))
}
