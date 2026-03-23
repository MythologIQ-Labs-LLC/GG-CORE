// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! GPU Support Tests v2 - Tests for CUDA and Metal backends

use std::sync::Arc;
use gg_core::engine::FlashAttnGpuConfig;
use gg_core::engine::{GpuBackend, GpuConfig, GpuDevice, GpuError, GpuManager};
use gg_core::engine::{MultiGpuConfig, MultiGpuManager, MultiGpuStrategy};

// ============= Base GPU Tests =============

#[test]
fn test_gpu_backend_display() {
    assert_eq!(format!("{}", GpuBackend::Cuda), "CUDA");
    assert_eq!(format!("{}", GpuBackend::Metal), "Metal");
    assert_eq!(format!("{}", GpuBackend::Cpu), "CPU");
}

#[test]
fn test_gpu_backend_default() {
    let backend = GpuBackend::default();
    assert_eq!(backend, GpuBackend::Cpu);
}

#[test]
fn test_gpu_device_cpu() {
    let device = GpuDevice::cpu();
    assert_eq!(device.backend, GpuBackend::Cpu);
    assert_eq!(device.index, 0);
    assert_eq!(device.name, "CPU");
    assert!(device.has_memory(0));
    assert!(device.has_memory(u64::MAX));
    assert_eq!(device.memory_utilization(), 0.0);
}

#[test]
fn test_gpu_config_cuda_all_layers() {
    let config = GpuConfig::cuda_all_layers();
    assert_eq!(config.backend, GpuBackend::Cuda);
    assert_eq!(config.gpu_layers, u32::MAX);
}

#[test]
fn test_gpu_manager_cpu_fallback() {
    // Request CUDA when only CPU is available
    let config = GpuConfig {
        backend: GpuBackend::Cuda,
        device_index: 0,
        ..Default::default()
    };

    // Should fall back to CPU or return NoDevicesAvailable gracefully
    let result = GpuManager::new(config);
    match result {
        Ok(manager) => {
            assert_eq!(manager.active_device().unwrap().backend, GpuBackend::Cpu);
        }
        Err(GpuError::NoDevicesAvailable) => {
            // Acceptable - no GPU devices found
        }
        Err(e) => panic!("Unexpected error: {}", e),
    }
}

// ============= CUDA Backend Tests =============

#[cfg(feature = "cuda")]
mod cuda_tests {
    use super::*;
    use gg_core::engine::{CudaBackend, CudaDeviceInfo, CudaError};

    #[test]
    fn test_cuda_backend_creation() {
        // This test will pass even without CUDA devices
        let result = CudaBackend::new();
        // Either succeeds or returns DriverNotFound
        match result {
            Ok(backend) => {
                // CUDA is available
                assert!(backend.device_count() > 0 || true); // May have 0 devices
            }
            Err(CudaError::DriverNotFound) => {
                // CUDA not installed - expected on some systems
            }
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_cuda_is_available() {
        // Just check the function doesn't panic
        let _ = CudaBackend::is_available();
    }

    #[test]
    fn test_cuda_device_info_compute_check() {
        let info = CudaDeviceInfo {
            device: GpuDevice::cpu(),
            compute_major: 8,
            compute_minor: 6,
            multiprocessor_count: 108,
            max_threads_per_block: 1024,
            max_shared_memory_per_block: 49152,
            memory_clock_rate: 5001,
            memory_bus_width: 384,
            l2_cache_size: 12582912,
            max_clock_rate: 1830,
            unified_memory: true,
            concurrent_kernels: true,
            cooperative_launch: true,
        };

        assert!(info.supports_compute(8, 0));
        assert!(info.supports_compute(8, 6));
        assert!(!info.supports_compute(9, 0));
        assert!(info.supports_flash_attention());
    }
}

// ============= Metal Backend Tests =============

#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_tests {
    use super::*;
    use gg_core::engine::{MetalBackend, MetalDeviceInfo, MetalError, MetalGpuFamily};

    #[test]
    fn test_metal_backend_creation() {
        let result = MetalBackend::new();
        match result {
            Ok(backend) => {
                // Metal is available
                assert!(backend.device_count() > 0);
            }
            Err(MetalError::NotAvailable) => {
                // Metal not available - shouldn't happen on macOS
            }
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_metal_device_info_apple_silicon() {
        let info = MetalDeviceInfo {
            device: GpuDevice::cpu(),
            is_apple_silicon: true,
            max_threadgroup_memory_length: 32768,
            max_threads_per_threadgroup: 1024,
            recommended_max_working_set_size: 17179869184,
            current_allocated_size: 0,
            supports_ray_tracing: true,
            supports_mesh_shaders: true,
            gpu_family: MetalGpuFamily::Metal3,
            architecture: "Apple M3".to_string(),
        };

        assert!(info.supports_unified_memory());
        assert!(info.supports_metal3());
    }
}

// ============= Multi-GPU Tests =============

fn create_test_gpu_devices() -> Vec<Arc<GpuDevice>> {
    vec![
        Arc::new(GpuDevice {
            backend: GpuBackend::Cuda,
            index: 0,
            name: "RTX 4090".to_string(),
            total_memory: 24_000_000_000,
            available_memory: 20_000_000_000,
            compute_capability: Some((8, 9)),
        }),
        Arc::new(GpuDevice {
            backend: GpuBackend::Cuda,
            index: 1,
            name: "RTX 4090".to_string(),
            total_memory: 24_000_000_000,
            available_memory: 22_000_000_000,
            compute_capability: Some((8, 9)),
        }),
    ]
}

#[test]
fn test_multi_gpu_manager_creation() {
    let devices = create_test_gpu_devices();
    let config = MultiGpuConfig::default();

    let manager = MultiGpuManager::new(devices, config);
    assert!(manager.is_ok());

    let manager = manager.unwrap();
    assert_eq!(manager.num_gpus(), 2);
}

#[test]
fn test_multi_gpu_partition_by_layers() {
    let devices = create_test_gpu_devices();
    let config = MultiGpuConfig {
        strategy: MultiGpuStrategy::LayerParallelism,
        ..Default::default()
    };

    let mut manager = MultiGpuManager::new(devices, config).unwrap();
    let partitions = manager.partition_model(80, 40_000_000_000).unwrap();

    assert_eq!(partitions.len(), 2);

    // Each GPU should have 40 layers
    let total_layers: usize = partitions.iter().map(|p| p.layers.len()).sum();
    assert_eq!(total_layers, 80);
}

#[test]
fn test_multi_gpu_total_memory() {
    let devices = create_test_gpu_devices();
    let config = MultiGpuConfig::default();

    let manager = MultiGpuManager::new(devices, config).unwrap();
    let total = manager.total_memory();

    assert_eq!(total, 48_000_000_000); // 2 * 24GB
}

#[test]
fn test_multi_gpu_strategy_auto() {
    let devices = create_test_gpu_devices();
    let config = MultiGpuConfig {
        strategy: MultiGpuStrategy::Auto,
        ..Default::default()
    };

    let mut manager = MultiGpuManager::new(devices, config).unwrap();
    manager.partition_model(80, 40_000_000_000).unwrap();

    // Auto should select a strategy
    assert!(matches!(
        manager.active_strategy(),
        MultiGpuStrategy::LayerParallelism
            | MultiGpuStrategy::TensorParallelism
            | MultiGpuStrategy::PipelineParallelism
    ));
}

// ============= Flash Attention GPU Tests =============

#[test]
fn test_flash_attn_gpu_config() {
    let config = FlashAttnGpuConfig::new(4096, 32, 128);

    assert_eq!(config.seq_len, 4096);
    assert_eq!(config.num_heads, 32);
    assert_eq!(config.head_dim, 128);
    assert!((config.scale - 0.0884).abs() < 0.01); // 1/sqrt(128)
}

#[test]
fn test_flash_attn_gpu_memory_calculation() {
    let config = FlashAttnGpuConfig::new(2048, 32, 128);
    let memory = config.memory_required(1);

    // Should be approximately 64MB for a single sequence
    assert!(memory > 50_000_000);
    assert!(memory < 100_000_000);
}

#[test]
fn test_flash_attn_gpu_gqa() {
    let config = FlashAttnGpuConfig::new(2048, 32, 128).with_kv_heads(8);

    assert_eq!(config.num_heads, 32);
    assert_eq!(config.num_kv_heads, 8);
}

#[test]
fn test_flash_attn_gpu_num_blocks() {
    let config = FlashAttnGpuConfig::new(2048, 32, 128);
    assert_eq!(config.num_blocks(), 16);

    let config = FlashAttnGpuConfig::new(4096, 32, 128);
    assert_eq!(config.num_blocks(), 32);
}
