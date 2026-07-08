// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Flash Attention GPU Kernels
//!
//! Implements memory-efficient attention on GPU using CUDA or Metal.
//! Flash Attention reduces memory from O(n²) to O(n) by tiling computation.

use thiserror::Error;

/// Flash Attention GPU Error
#[derive(Debug, Error)]
pub enum FlashAttnGpuError {
    #[error("Flash Attention requires compute capability 8.0+")]
    InsufficientComputeCapability,

    #[error("GPU memory allocation failed: {0}")]
    AllocationFailed(String),

    #[error("Kernel launch failed: {0}")]
    KernelLaunchFailed(String),

    #[error("Flash Attention not supported on this device")]
    NotSupported,

    #[error("Invalid attention parameters: {0}")]
    InvalidParams(String),
}

/// Flash Attention Configuration for GPU
#[derive(Debug, Clone)]
pub struct FlashAttnGpuConfig {
    /// Sequence length
    pub seq_len: usize,
    /// Number of attention heads
    pub num_heads: usize,
    /// Head dimension
    pub head_dim: usize,
    /// Number of key-value heads (for GQA/MQA)
    pub num_kv_heads: usize,
    /// Block size for tiling
    pub block_size: usize,
    /// Whether to use causal attention
    pub causal: bool,
    /// Softmax scale (typically 1/sqrt(head_dim))
    pub scale: f32,
    /// Dropout probability (0.0 for inference)
    pub dropout: f32,
}

impl Default for FlashAttnGpuConfig {
    fn default() -> Self {
        Self {
            seq_len: 2048,
            num_heads: 32,
            head_dim: 128,
            num_kv_heads: 32,
            block_size: 128,
            causal: true,
            scale: 0.0884, // 1/sqrt(128)
            dropout: 0.0,
        }
    }
}

impl FlashAttnGpuConfig {
    /// Create a new configuration
    pub fn new(seq_len: usize, num_heads: usize, head_dim: usize) -> Self {
        let scale = 1.0 / (head_dim as f32).sqrt();
        Self {
            seq_len,
            num_heads,
            head_dim,
            num_kv_heads: num_heads,
            block_size: 128,
            causal: true,
            scale,
            dropout: 0.0,
        }
    }

    /// Set number of KV heads for grouped-query attention
    pub fn with_kv_heads(mut self, num_kv_heads: usize) -> Self {
        self.num_kv_heads = num_kv_heads;
        self
    }

    /// Set causal attention
    pub fn with_causal(mut self, causal: bool) -> Self {
        self.causal = causal;
        self
    }

    /// Calculate memory required for attention (in bytes)
    pub fn memory_required(&self, batch_size: usize) -> usize {
        // Q, K, V tensors
        let qkv_size = batch_size
            * self.seq_len
            * (self.num_heads + 2 * self.num_kv_heads)
            * self.head_dim
            * 2; // f16

        // Output tensor
        let output_size = batch_size * self.seq_len * self.num_heads * self.head_dim * 2;

        // Softmax LSE (log-sum-exp) - per head per sequence position
        let lse_size = batch_size * self.num_heads * self.seq_len * 4; // f32

        qkv_size + output_size + lse_size
    }

    /// Get number of blocks for sequence
    pub fn num_blocks(&self) -> usize {
        (self.seq_len + self.block_size - 1) / self.block_size
    }
}

/// Flash Attention GPU Kernel trait
pub trait FlashAttnGpuKernel {
    /// Run forward pass
    fn forward(
        &self,
        query: &[f16],
        key: &[f16],
        value: &[f16],
        batch_size: usize,
    ) -> Result<Vec<f16>, FlashAttnGpuError>;

    /// Get the configuration
    fn config(&self) -> &FlashAttnGpuConfig;
}

/// f16 type alias (would use half crate in production)
#[allow(non_camel_case_types)]
pub type f16 = half::f16;

// CUDA implementation
#[cfg(feature = "cuda")]
pub mod cuda_impl {
    use super::*;
    use crate::engine::cuda::{CudaDevice, CudaError, CudaMemoryBuffer};
    use std::sync::Arc;

    /// CUDA Flash Attention Kernel
    pub struct CudaFlashAttn {
        config: FlashAttnGpuConfig,
        device: Arc<CudaDevice>,
        /// Query buffer
        query_buf: Option<CudaMemoryBuffer>,
        /// Key buffer
        key_buf: Option<CudaMemoryBuffer>,
        /// Value buffer
        value_buf: Option<CudaMemoryBuffer>,
        /// Output buffer
        output_buf: Option<CudaMemoryBuffer>,
    }

    impl CudaFlashAttn {
        /// Create a new CUDA Flash Attention kernel
        pub fn new(
            device: Arc<CudaDevice>,
            config: FlashAttnGpuConfig,
        ) -> Result<Self, FlashAttnGpuError> {
            // Check compute capability
            let (major, minor) = device.compute_capability().unwrap_or((0, 0));
            if (major, minor) < (8, 0) {
                return Err(FlashAttnGpuError::InsufficientComputeCapability);
            }

            Ok(Self {
                config,
                device,
                query_buf: None,
                key_buf: None,
                value_buf: None,
                output_buf: None,
            })
        }

        /// Allocate GPU buffers
        pub fn allocate_buffers(&mut self, batch_size: usize) -> Result<(), FlashAttnGpuError> {
            let q_size =
                batch_size * self.config.seq_len * self.config.num_heads * self.config.head_dim * 2;
            let k_size = batch_size
                * self.config.seq_len
                * self.config.num_kv_heads
                * self.config.head_dim
                * 2;
            let v_size = k_size;
            let o_size = q_size;

            self.query_buf = Some(
                CudaMemoryBuffer::alloc(self.device.clone(), q_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );
            self.key_buf = Some(
                CudaMemoryBuffer::alloc(self.device.clone(), k_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );
            self.value_buf = Some(
                CudaMemoryBuffer::alloc(self.device.clone(), v_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );
            self.output_buf = Some(
                CudaMemoryBuffer::alloc(self.device.clone(), o_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );

            Ok(())
        }
    }

    impl FlashAttnGpuKernel for CudaFlashAttn {
        fn forward(
            &self,
            query: &[f16],
            key: &[f16],
            value: &[f16],
            batch_size: usize,
        ) -> Result<Vec<f16>, FlashAttnGpuError> {
            // Copy inputs to GPU
            if let Some(ref q_buf) = self.query_buf {
                let q_bytes: Vec<u8> = query.iter().flat_map(|f| f.to_le_bytes()).collect();
                q_buf
                    .copy_from_host(&q_bytes)
                    .map_err(|e| FlashAttnGpuError::KernelLaunchFailed(e.to_string()))?;
            }

            // ... kernel launch would go here ...

            // FAIL-FAST: kernel not yet implemented - do not return placeholder data
            Err(FlashAttnGpuError::KernelLaunchFailed(
                "CUDA Flash Attention kernel not implemented - production requires real kernel"
                    .into(),
            ))
        }

        fn config(&self) -> &FlashAttnGpuConfig {
            &self.config
        }
    }
}

// Metal implementation
#[cfg(all(feature = "metal", target_os = "macos"))]
pub mod metal_impl {
    use super::*;
    use crate::engine::metal::{MetalBuffer, MetalDevice, MetalError};
    use std::sync::Arc;

    /// Metal Flash Attention Kernel
    pub struct MetalFlashAttn {
        config: FlashAttnGpuConfig,
        device: Arc<MetalDevice>,
        /// Query buffer
        query_buf: Option<MetalBuffer>,
        /// Key buffer
        key_buf: Option<MetalBuffer>,
        /// Value buffer
        value_buf: Option<MetalBuffer>,
        /// Output buffer
        output_buf: Option<MetalBuffer>,
    }

    impl MetalFlashAttn {
        /// Create a new Metal Flash Attention kernel
        pub fn new(
            device: Arc<MetalDevice>,
            config: FlashAttnGpuConfig,
        ) -> Result<Self, FlashAttnGpuError> {
            Ok(Self {
                config,
                device,
                query_buf: None,
                key_buf: None,
                value_buf: None,
                output_buf: None,
            })
        }

        /// Allocate Metal buffers
        pub fn allocate_buffers(&mut self, batch_size: usize) -> Result<(), FlashAttnGpuError> {
            let q_size =
                batch_size * self.config.seq_len * self.config.num_heads * self.config.head_dim * 2;
            let k_size = batch_size
                * self.config.seq_len
                * self.config.num_kv_heads
                * self.config.head_dim
                * 2;
            let v_size = k_size;
            let o_size = q_size;

            self.query_buf = Some(
                MetalBuffer::new(&self.device, q_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );
            self.key_buf = Some(
                MetalBuffer::new(&self.device, k_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );
            self.value_buf = Some(
                MetalBuffer::new(&self.device, v_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );
            self.output_buf = Some(
                MetalBuffer::new(&self.device, o_size)
                    .map_err(|e| FlashAttnGpuError::AllocationFailed(e.to_string()))?,
            );

            Ok(())
        }
    }

    impl FlashAttnGpuKernel for MetalFlashAttn {
        fn forward(
            &self,
            query: &[f16],
            key: &[f16],
            value: &[f16],
            batch_size: usize,
        ) -> Result<Vec<f16>, FlashAttnGpuError> {
            // Copy inputs to Metal buffer
            if let Some(ref q_buf) = self.query_buf {
                let q_bytes: Vec<u8> = query.iter().flat_map(|f| f.to_le_bytes()).collect();
                // Metal buffers support direct memory access on Apple Silicon
            }

            // ... kernel launch would go here ...

            // FAIL-FAST: kernel not yet implemented - do not return placeholder data
            Err(FlashAttnGpuError::KernelLaunchFailed(
                "Metal Flash Attention kernel not implemented - production requires real kernel"
                    .into(),
            ))
        }

        fn config(&self) -> &FlashAttnGpuConfig {
            &self.config
        }
    }
}

// Re-export based on available features
#[cfg(feature = "cuda")]
pub use cuda_impl::CudaFlashAttn;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub use metal_impl::MetalFlashAttn;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flash_attn_config_default() {
        let config = FlashAttnGpuConfig::default();
        assert_eq!(config.seq_len, 2048);
        assert_eq!(config.num_heads, 32);
        assert_eq!(config.head_dim, 128);
        assert!(config.causal);
    }

    #[test]
    fn test_flash_attn_config_new() {
        let config = FlashAttnGpuConfig::new(4096, 48, 64);
        assert_eq!(config.seq_len, 4096);
        assert_eq!(config.num_heads, 48);
        assert_eq!(config.head_dim, 64);
        assert!((config.scale - 0.125).abs() < 0.01); // 1/sqrt(64)
    }

    #[test]
    fn test_flash_attn_memory_required() {
        let config = FlashAttnGpuConfig::new(2048, 32, 128);
        let memory = config.memory_required(1);

        // Q: 2048 * 32 * 128 * 2 = 16MB
        // K: 2048 * 32 * 128 * 2 = 16MB
        // V: 2048 * 32 * 128 * 2 = 16MB
        // O: 2048 * 32 * 128 * 2 = 16MB
        // LSE: 1 * 32 * 2048 * 4 = 256KB
        // Total: ~64MB + overhead
        assert!(memory > 60_000_000);
        assert!(memory < 80_000_000);
    }

    #[test]
    fn test_flash_attn_num_blocks() {
        let config = FlashAttnGpuConfig::new(2048, 32, 128);
        assert_eq!(config.num_blocks(), 16); // 2048 / 128

        let config = FlashAttnGpuConfig::new(4096, 32, 128);
        assert_eq!(config.num_blocks(), 32); // 4096 / 128
    }

    #[test]
    fn test_flash_attn_with_kv_heads() {
        let config = FlashAttnGpuConfig::new(2048, 32, 128).with_kv_heads(8); // GQA with 8 KV heads

        assert_eq!(config.num_heads, 32);
        assert_eq!(config.num_kv_heads, 8);
    }
}
