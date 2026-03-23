//! Tests for GPU backend support.
//!
//! Extracted from `gpu.rs` for Section 4 compliance.

use super::*;
use crate::engine::gpu_allocator::MockGpuAllocator;
use crate::engine::gpu_manager::GpuManager;
use std::sync::Arc;

#[test]
fn test_gpu_backend_display() {
    assert_eq!(format!("{}", GpuBackend::Cuda), "CUDA");
    assert_eq!(format!("{}", GpuBackend::Metal), "Metal");
    assert_eq!(format!("{}", GpuBackend::Cpu), "CPU");
}

#[test]
fn test_gpu_device_cpu() {
    let device = GpuDevice::cpu();
    assert_eq!(device.backend, GpuBackend::Cpu);
    assert!(device.has_memory(0));
    assert_eq!(device.memory_utilization(), 0.0);
}

#[test]
fn test_gpu_config_default() {
    let config = GpuConfig::default();
    assert_eq!(config.backend, GpuBackend::Cpu);
    assert_eq!(config.gpu_layers, 0);
}

#[test]
fn test_gpu_config_cpu() {
    let config = GpuConfig::cpu();
    assert_eq!(config.backend, GpuBackend::Cpu);
    assert_eq!(config.gpu_layers, 0);
}

#[test]
fn test_gpu_config_cuda_all_layers() {
    let config = GpuConfig::cuda_all_layers();
    assert_eq!(config.backend, GpuBackend::Cuda);
    assert_eq!(config.gpu_layers, u32::MAX);
    assert!(config.gpu_enabled);
}

#[test]
fn test_gpu_manager_cpu_only() {
    let config = GpuConfig::cpu();
    let manager = GpuManager::new(config).unwrap();

    assert!(manager.active_device().is_some());
    assert_eq!(manager.active_device().unwrap().backend, GpuBackend::Cpu);
}

#[test]
fn test_gpu_manager_allocate_and_drop() {
    let config = GpuConfig::cpu();
    let allocator = Arc::new(MockGpuAllocator::new(4096, 0));
    let manager = GpuManager::with_allocator(config, allocator.clone()).unwrap();

    let mem = manager.allocate_memory(1024).unwrap();
    assert_eq!(allocator.allocated_bytes(), 1024);

    drop(mem);
    assert_eq!(allocator.allocated_bytes(), 0);
}

#[test]
fn test_device_placement_default_is_cpu() {
    let placement = DevicePlacement::default();
    assert_eq!(placement, DevicePlacement::Cpu);
}

#[test]
fn test_device_placement_gpu_variant() {
    let placement = DevicePlacement::Gpu { device_index: 0, layers: Some(32) };
    assert!(matches!(placement, DevicePlacement::Gpu { .. }));
}

#[test]
fn test_device_placement_split_variant() {
    let placement = DevicePlacement::Split { devices: vec![0, 1] };
    assert!(matches!(placement, DevicePlacement::Split { .. }));
}

#[test]
fn test_gpu_memory_cpu_only_no_alloc() {
    let device = Arc::new(GpuDevice::cpu());
    let mem = GpuMemory::cpu_only(512, device);
    assert_eq!(mem.size, 512);
    // Drop should be a no-op (no allocator)
}
