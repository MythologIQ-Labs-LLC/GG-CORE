//! Inference engine module for CORE Runtime.
//!
//! Handles tokenization, inference execution, and token streaming.
//! Provides the `InferenceModel` trait and supporting types.
//!
//! The `advanced` feature enables proprietary modules (GPU kernels,
//! quantization, SIMD, speculative decoding, multi-GPU). These are
//! provided by GG-CORE-TierSynergy.

// --- Core modules (always available) ---
pub mod config;
pub mod decode;
pub mod degraded_mode;
pub mod error;
pub mod filter;
pub mod flash_attn;
pub mod gguf;
pub mod gpu;
pub mod gpu_allocator;
pub mod gpu_manager;
pub mod gpu_pool;
pub mod inference;
pub mod inference_types;
pub mod input;
pub mod model;
pub mod moe;
#[cfg(feature = "advanced")]
pub mod multi_gpu_exec;
#[cfg(feature = "advanced")]
pub mod multi_gpu_partition;
#[cfg(feature = "advanced")]
pub mod multi_gpu_pipeline;
#[cfg(feature = "advanced")]
pub mod multi_gpu_tensor;
pub mod onnx;
pub mod output;
pub mod prefill;
pub mod simd_tokenizer;
mod streaming;
mod tokenizer;

// --- Advanced modules (requires `advanced` feature, provided by TierSynergy) ---
#[cfg(feature = "advanced")]
pub mod adaptive_speculative;
#[cfg(feature = "advanced")]
pub mod flash_attn_gpu;
#[cfg(feature = "advanced")]
pub mod multi_gpu;
#[cfg(feature = "advanced")]
pub mod quantize;
#[cfg(feature = "advanced")]
pub mod simd_matmul;
#[cfg(feature = "advanced")]
mod simd_neon;
#[cfg(feature = "advanced")]
pub mod simd_tokenizer_v2;
#[cfg(feature = "advanced")]
pub mod speculative_v2;

// --- GPU backend modules (conditionally compiled) ---
#[cfg(feature = "cuda")]
pub mod cuda;
#[cfg(all(feature = "metal", target_os = "macos"))]
pub mod metal;

// --- Core re-exports (always available) ---
pub use config::InferenceConfig;
pub use decode::{DecodeConfig, DecodeExecutor, DecodeStepResult};
pub use degraded_mode::{
    DegradedDecision, DegradedModeConfig, DegradedModePolicy, ResourcePressure,
};
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
pub use streaming::{StreamItem, StreamTerminal, TokenStream, TokenStreamSender};
pub use tokenizer::{TokenizerError, TokenizerWrapper};

// Backend re-exports
#[cfg(feature = "gguf")]
pub use gguf::LlamaBackendInner;
pub use gguf::{GgufConfig, GgufGenerator};
pub use gpu::{DevicePlacement, GpuBackend, GpuConfig, GpuDevice, GpuError, GpuMemory};
#[cfg(feature = "cuda")]
pub use gpu_allocator::CudaGpuAllocator;
#[cfg(feature = "metal")]
pub use gpu_allocator::MetalGpuAllocator;
pub use gpu_allocator::{GpuAllocation, GpuAllocator, MockGpuAllocator};
pub use gpu_manager::GpuManager;
pub use gpu_pool::GpuMemoryPool;
pub use model::Model;
pub use onnx::{OnnxClassifier, OnnxConfig, OnnxEmbedder};

// --- Advanced re-exports (behind feature gate) ---
#[cfg(feature = "advanced")]
pub use flash_attn_gpu::{FlashAttnGpuConfig, FlashAttnGpuError, FlashAttnGpuKernel};
#[cfg(feature = "advanced")]
pub use multi_gpu::{
    GpuPartition, MultiGpuConfig, MultiGpuError, MultiGpuManager, MultiGpuStrategy,
};
#[cfg(feature = "advanced")]
pub use quantize::{QuantFormat, QuantizedTensor, QUANT_BLOCK_SIZE};
#[cfg(feature = "advanced")]
pub use simd_matmul::{dot_q4, dot_q8, init_simd};
#[cfg(feature = "advanced")]
pub use simd_tokenizer_v2::{
    SimdTokenizer as SimdTokenizerV2, TokenizerError as TokenizerV2Error, TokenizerStats,
};
#[cfg(feature = "advanced")]
pub use speculative_v2::{
    DraftModel, SpeculativeConfig, SpeculativeDecoder, SpeculativeStats, TargetModel, VerifyResult,
};

// Adaptive speculative telemetry (requires `advanced` feature)
#[cfg(feature = "advanced")]
pub use adaptive_speculative::telemetry::{
    AutoDisableReason, SpeculativeSessionStats, SpeculativeTelemetry,
};

// Multi-GPU execution (requires `advanced` feature — depends on multi_gpu types)
#[cfg(feature = "advanced")]
pub use multi_gpu_exec::{
    ExecutionResult, LayerParallelExecutor, MockPartitionExecutor, PartitionExecutor, TensorData,
};
#[cfg(feature = "advanced")]
pub use multi_gpu_partition::{CrossGpuCommunication, TransferMethod, TransferResult};
#[cfg(feature = "advanced")]
pub use multi_gpu_pipeline::PipelineParallelExecutor;
#[cfg(feature = "advanced")]
pub use multi_gpu_tensor::TensorParallelExecutor;

// CUDA backend re-exports
#[cfg(feature = "cuda")]
pub use cuda::{
    CudaBackend, CudaDeviceInfo, CudaError, CudaExecutionStream, CudaMemoryBuffer, FlashAttention,
};

// Metal backend re-exports
#[cfg(all(feature = "metal", target_os = "macos"))]
pub use metal::{
    MetalBackend, MetalBuffer, MetalCommandEncoder, MetalComputePipeline, MetalDeviceInfo,
    MetalError, MetalGpuFamily,
};

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
