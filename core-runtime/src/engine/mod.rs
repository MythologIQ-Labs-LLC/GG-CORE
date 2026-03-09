//! Inference engine module for CORE Runtime.
//!
//! Handles tokenization, inference execution, and token streaming.
//! Provides the `InferenceModel` trait and supporting types.

pub mod config;
pub mod decode;
pub mod error;
pub mod filter;
pub mod flash_attn;
pub mod gguf;
pub mod gpu;
pub mod input;
pub mod onnx;
pub mod output;
pub mod prefill;
pub mod simd_tokenizer;

// GPU backend modules (conditionally compiled)
pub mod moe;

pub mod inference;
mod streaming;
mod tokenizer;

pub use config::InferenceConfig;
pub use decode::{DecodeConfig, DecodeExecutor, DecodeStepResult};
pub use error::InferenceError;
pub use filter::{FilterConfig, OutputFilter};
pub use flash_attn::{FlashAttn, FlashAttnConfig};
pub use inference::{InferenceEngine, InferenceParams, InferenceResult};
pub use input::{ChatMessage, ChatRole, InferenceInput};
pub use input::{MAX_BATCH_SIZE, MAX_INPUT_TOKENS, MAX_TEXT_BYTES};
pub use output::{ClassificationResult, EmbeddingResult, EntityResult};
pub use output::{FinishReason, GenerationResult, InferenceOutput};
pub use prefill::{PrefillConfig, PrefillExecutor, PrefillResult};
pub use simd_tokenizer::SimdTokenizer;
pub use streaming::{StreamingOutput, TokenStream, TokenStreamSender};
pub use tokenizer::{TokenizerError, TokenizerWrapper};

// Backend re-exports
#[cfg(feature = "gguf")]
pub use gguf::LlamaBackendInner;
pub use gguf::{GgufConfig, GgufGenerator, GgufModel};
pub use gpu::{GpuBackend, GpuConfig, GpuDevice, GpuError, GpuManager, GpuMemory, GpuMemoryPool};
pub use onnx::{OnnxClassifier, OnnxConfig, OnnxEmbedder, OnnxModel};

// CUDA backend re-exports

// Metal backend re-exports

// Multi-GPU support

// Mixture of Experts support
pub use moe::{
    ExpertCombiner, ExpertDeviceAssignment, ExpertOutput, LinearRouter, MoeConfig, MoeError,
    MoeExecutor, MoeRouter, RoutingDecision,
};

/// What a model can do — used by the InferenceModel trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceCapability {
    TextClassification,
    TextGeneration,
    Embedding,
    NamedEntityRecognition,
}
