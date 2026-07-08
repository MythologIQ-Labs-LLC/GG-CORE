// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Multi-GPU support: model/pipeline/tensor/expert parallelism across GPUs.

use std::sync::Arc;
use thiserror::Error;

use super::{GpuBackend, GpuDevice};

#[derive(Debug, Error)]
pub enum MultiGpuError {
    #[error("No multi-GPU configuration available")]
    NoMultiGpuConfig,

    #[error("Insufficient GPUs: requested {required}, available {available}")]
    InsufficientGpus { required: usize, available: usize },

    #[error("GPU {index} not available: {reason}")]
    GpuNotAvailable { index: usize, reason: String },

    #[error("Model partitioning failed: {0}")]
    PartitioningFailed(String),

    #[error("Cross-GPU communication failed: {0}")]
    CommunicationFailed(String),

    #[error("Load balancing failed: {0}")]
    LoadBalancingFailed(String),

    #[error("Synchronization failed: {0}")]
    SynchronizationFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiGpuStrategy {
    LayerParallelism,
    TensorParallelism,
    PipelineParallelism,
    ExpertParallelism,
    Auto,
}

impl Default for MultiGpuStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

/// GPU partition configuration
#[derive(Debug, Clone)]
pub struct GpuPartition {
    /// GPU index
    pub gpu_index: usize,
    /// Layers assigned to this GPU (for layer parallelism)
    pub layers: std::ops::Range<usize>,
    /// Memory budget for this partition in bytes
    pub memory_budget: u64,
    /// Percentage of model parameters on this GPU
    pub parameter_fraction: f32,
}

/// Multi-GPU Configuration
#[derive(Debug, Clone)]
pub struct MultiGpuConfig {
    /// Strategy for multi-GPU distribution
    pub strategy: MultiGpuStrategy,
    /// Number of GPUs to use (0 = all available)
    pub num_gpus: usize,
    /// Main GPU for coordination
    pub main_gpu: usize,
    /// Enable peer-to-peer communication
    pub enable_p2p: bool,
    /// Maximum memory per GPU (0 = auto)
    pub max_memory_per_gpu: u64,
    /// Load balancing threshold (0.0 - 1.0)
    pub load_balance_threshold: f32,
    /// Enable gradient checkpointing for memory efficiency
    pub gradient_checkpointing: bool,
}

impl Default for MultiGpuConfig {
    fn default() -> Self {
        Self {
            strategy: MultiGpuStrategy::Auto,
            num_gpus: 0,
            main_gpu: 0,
            enable_p2p: true,
            max_memory_per_gpu: 0,
            load_balance_threshold: 0.1,
            gradient_checkpointing: false,
        }
    }
}

/// Multi-GPU Manager - handles coordination across multiple GPUs
pub struct MultiGpuManager {
    /// Available GPU devices
    devices: Vec<Arc<GpuDevice>>,
    /// Configuration
    config: MultiGpuConfig,
    /// Partition assignments
    partitions: Vec<GpuPartition>,
    /// Active strategy
    active_strategy: MultiGpuStrategy,
}

impl MultiGpuManager {
    /// Create a new multi-GPU manager
    pub fn new(
        devices: Vec<Arc<GpuDevice>>,
        config: MultiGpuConfig,
    ) -> Result<Self, MultiGpuError> {
        if devices.len() < 2 {
            return Err(MultiGpuError::InsufficientGpus {
                required: 2,
                available: devices.len(),
            });
        }

        // Filter to only GPU devices (not CPU)
        let gpu_devices: Vec<Arc<GpuDevice>> = devices
            .into_iter()
            .filter(|d| d.backend != GpuBackend::Cpu)
            .collect();

        if gpu_devices.len() < 2 {
            return Err(MultiGpuError::InsufficientGpus {
                required: 2,
                available: gpu_devices.len(),
            });
        }

        let num_gpus = if config.num_gpus == 0 {
            gpu_devices.len()
        } else {
            config.num_gpus.min(gpu_devices.len())
        };

        let devices_to_use: Vec<Arc<GpuDevice>> = gpu_devices.into_iter().take(num_gpus).collect();

        let manager = Self {
            devices: devices_to_use,
            config,
            partitions: Vec::new(),
            active_strategy: MultiGpuStrategy::Auto,
        };

        Ok(manager)
    }

    /// Get available GPUs
    pub fn devices(&self) -> &[Arc<GpuDevice>] {
        &self.devices
    }

    /// Get number of GPUs in use
    pub fn num_gpus(&self) -> usize {
        self.devices.len()
    }

    /// Partition a model across GPUs
    pub fn partition_model(
        &mut self,
        num_layers: usize,
        model_size_bytes: u64,
    ) -> Result<&[GpuPartition], MultiGpuError> {
        use super::multi_gpu_partition as part;

        self.active_strategy = part::determine_strategy(&self.config, &self.devices, num_layers);

        self.partitions = match self.active_strategy {
            MultiGpuStrategy::LayerParallelism | MultiGpuStrategy::Auto => {
                part::partition_by_layers(&self.devices, num_layers, model_size_bytes)?
            }
            MultiGpuStrategy::TensorParallelism => {
                part::partition_by_tensors(&self.devices, model_size_bytes)?
            }
            MultiGpuStrategy::PipelineParallelism => {
                part::partition_by_pipeline(&self.devices, num_layers, model_size_bytes)?
            }
            MultiGpuStrategy::ExpertParallelism => {
                part::partition_by_experts(&self.devices, model_size_bytes)?
            }
        };

        Ok(&self.partitions)
    }

    /// Compute variance in GPU memory sizes
    #[cfg(test)]
    pub(crate) fn compute_memory_variance(&self) -> f32 {
        super::multi_gpu_partition::compute_memory_variance(&self.devices)
    }

    /// Get current partitions
    pub fn partitions(&self) -> &[GpuPartition] {
        &self.partitions
    }

    /// Get active strategy
    pub fn active_strategy(&self) -> MultiGpuStrategy {
        self.active_strategy
    }

    /// Check if load is balanced across GPUs
    pub fn is_load_balanced(&self) -> bool {
        if self.partitions.is_empty() {
            return true;
        }

        let fractions: Vec<f32> = self
            .partitions
            .iter()
            .map(|p| p.parameter_fraction)
            .collect();

        let mean = fractions.iter().sum::<f32>() / fractions.len() as f32;

        fractions
            .iter()
            .all(|&f| (f - mean).abs() <= self.config.load_balance_threshold)
    }

    /// Get total memory across all GPUs
    pub fn total_memory(&self) -> u64 {
        self.devices.iter().map(|d| d.total_memory).sum()
    }

    /// Get total available memory across all GPUs
    pub fn total_available_memory(&self) -> u64 {
        self.devices.iter().map(|d| d.available_memory).sum()
    }

    /// Get memory utilization across all GPUs
    pub fn memory_utilization(&self) -> f32 {
        let total = self.total_memory();
        if total == 0 {
            return 0.0;
        }
        let available = self.total_available_memory();
        ((total - available) as f64 / total as f64) as f32
    }
}

#[cfg(test)]
#[path = "multi_gpu_tests.rs"]
mod tests;
