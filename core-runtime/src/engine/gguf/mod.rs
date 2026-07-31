//! GGUF inference backend using llama-cpp-rs.
//!
//! Provides text generation models via llama.cpp bindings.

#[cfg(all(feature = "gguf", feature = "advanced"))]
pub mod adaptive_speculative;
#[cfg(feature = "gguf")]
pub mod backend;
mod generator;
#[cfg(all(feature = "gguf", feature = "advanced"))]
pub mod speculative;

#[cfg(all(feature = "gguf", feature = "advanced"))]
pub use adaptive_speculative::{GgufBlockDraftModel, GgufTargetVerifier};
#[cfg(feature = "gguf")]
pub use backend::LlamaBackendInner;
pub use generator::GgufGenerator;
#[cfg(all(feature = "gguf", feature = "advanced"))]
pub use speculative::{GgufDraftModel, GgufTargetModel};

use std::path::Path;
use std::sync::Arc;

use crate::engine::{InferenceError, Model};

/// Configuration for GGUF model loading.
#[derive(Debug, Clone)]
pub struct GgufConfig {
    /// Number of threads for inference (0 = auto).
    pub n_threads: u32,
    /// Context size for generation (max tokens in context window).
    pub n_ctx: u32,
    /// Number of layers to offload to GPU (0 = CPU only).
    pub n_gpu_layers: u32,
}

impl Default for GgufConfig {
    fn default() -> Self {
        Self {
            n_threads: 0,    // Auto-detect
            n_ctx: 2048,     // Default context
            n_gpu_layers: 0, // CPU only for sandbox
        }
    }
}

/// Load a GGUF model from a file path using llama-cpp-2.
///
/// # Errors
/// Returns error if model file is missing, invalid, or fails to load.
#[cfg(feature = "gguf")]
pub fn load_gguf_model(
    path: &Path,
    model_id: &str,
    config: &GgufConfig,
) -> Result<Arc<dyn Model>, InferenceError> {
    if !path.exists() {
        return Err(InferenceError::ModelError(format!(
            "model file not found: {}",
            path.display()
        )));
    }
    let generator = GgufGenerator::load(model_id.to_string(), path, config)?;
    Ok(Arc::new(generator))
}

/// Stub for non-gguf builds.
#[cfg(not(feature = "gguf"))]
pub fn load_gguf_model(
    _path: &Path,
    _model_id: &str,
    _config: &GgufConfig,
) -> Result<Arc<dyn Model>, InferenceError> {
    Err(InferenceError::ModelError(
        "GGUF support not compiled in. Enable 'gguf' feature.".into(),
    ))
}

/// Validate that a file has the GGUF magic bytes.
pub fn is_valid_gguf(path: &Path) -> Result<bool, std::io::Error> {
    use std::fs::File;
    use std::io::Read;

    const GGUF_MAGIC: [u8; 4] = [0x47, 0x47, 0x55, 0x46]; // "GGUF"

    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    Ok(magic == GGUF_MAGIC)
}
