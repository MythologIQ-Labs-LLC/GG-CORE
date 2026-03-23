// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! GPU Support Tests

use std::sync::Arc;
use gg_core::engine::{GpuBackend, GpuConfig, GpuDevice, GpuError, GpuManager, GpuMemoryPool};

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
    assert!(device.has_memory(u64::MAX)); // CPU always has "memory"
    assert_eq!(device.memory_utilization(), 0.0);
}

#[test]
fn test_gpu_device_has_memory() {
    // Create a GPU device (not CPU) to test memory checking
    let device = GpuDevice {
        backend: GpuBackend::Cuda,
        index: 0,
        name: "Test GPU".to_string(),
        total_memory: 1000,
        available_memory: 500,
        compute_capability: Some((8, 0)),
    };

    assert!(device.has_memory(400));
    assert!(!device.has_memory(600));

    // CPU always has memory
    let cpu = GpuDevice::cpu();
    assert!(cpu.has_memory(u64::MAX));
}

#[test]
fn test_gpu_device_memory_utilization() {
    let mut device = GpuDevice::cpu();
    device.total_memory = 1000;
    device.available_memory = 250;

    assert!((device.memory_utilization() - 0.75).abs() < 0.01);
}

#[test]
fn test_gpu_config_default() {
    let config = GpuConfig::default();
    assert_eq!(config.backend, GpuBackend::Cpu);
    assert_eq!(config.device_index, 0);
    assert_eq!(config.memory_fraction, 0.9);
    assert!(config.flash_attention);
    assert_eq!(config.gpu_layers, 0);
    assert!(!config.multi_gpu);
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
}

#[test]
fn test_gpu_manager_cpu_only() {
    let config = GpuConfig::cpu();
    let manager = GpuManager::new(config).unwrap();

    assert!(manager.active_device().is_some());
    assert_eq!(manager.active_device().unwrap().backend, GpuBackend::Cpu);
    assert!(!manager.is_gpu_available());
}

#[test]
fn test_gpu_manager_available_devices() {
    let config = GpuConfig::cpu();
    let manager = GpuManager::new(config).unwrap();

    let devices = manager.available_devices();
    assert!(!devices.is_empty());
    assert!(devices.iter().any(|d| d.backend == GpuBackend::Cpu));
}

#[test]
fn test_gpu_manager_available_backends() {
    let config = GpuConfig::cpu();
    let manager = GpuManager::new(config).unwrap();

    // CPU-only system should have no GPU backends
    let backends = manager.available_backends();
    assert!(backends.is_empty());
}

#[test]
fn test_gpu_memory_pool_new() {
    let device = Arc::new(GpuDevice::cpu());
    let pool = GpuMemoryPool::new(device, 1024);

    assert_eq!(pool.utilization(), 0.0);
}

#[test]
fn test_gpu_memory_pool_allocate() {
    let device = Arc::new(GpuDevice::cpu());
    let mut pool = GpuMemoryPool::new(device, 1024);

    let mem = pool.allocate(512).unwrap();
    assert_eq!(mem.size, 512);
    assert!((pool.utilization() - 0.5).abs() < 0.01);
}

#[test]
fn test_gpu_memory_pool_multiple_allocations() {
    let device = Arc::new(GpuDevice::cpu());
    let mut pool = GpuMemoryPool::new(device, 1024);

    pool.allocate(256).unwrap();
    pool.allocate(256).unwrap();
    assert!((pool.utilization() - 0.5).abs() < 0.01);

    pool.allocate(512).unwrap();
    assert!((pool.utilization() - 1.0).abs() < 0.01);
}

#[test]
fn test_gpu_memory_pool_out_of_memory() {
    let device = Arc::new(GpuDevice::cpu());
    let mut pool = GpuMemoryPool::new(device, 1024);

    pool.allocate(512).unwrap();
    let result = pool.allocate(1024);

    assert!(matches!(result, Err(GpuError::OutOfMemory { .. })));

    if let Err(GpuError::OutOfMemory {
        required,
        available,
    }) = result
    {
        assert_eq!(required, 1024);
        assert_eq!(available, 512);
    }
}

#[test]
fn test_gpu_error_display() {
    let err = GpuError::NoDevicesAvailable;
    assert_eq!(format!("{}", err), "No GPU devices available");

    let err = GpuError::DeviceNotFound(2);
    assert_eq!(format!("{}", err), "Device not found: 2");

    let err = GpuError::OutOfMemory {
        required: 1024,
        available: 512,
    };
    assert!(format!("{}", err).contains("1024"));
    assert!(format!("{}", err).contains("512"));
}

#[test]
fn test_gpu_manager_select_device_fallback() {
    // Request CUDA when only CPU is available
    let config = GpuConfig {
        backend: GpuBackend::Cuda,
        device_index: 0,
        ..Default::default()
    };

    // Should fall back to CPU without error (when no CUDA devices available)
    // This tests graceful degradation
    let result = GpuManager::new(config);

    // Either succeeds with CPU fallback or fails gracefully
    match result {
        Ok(manager) => {
            assert_eq!(manager.active_device().unwrap().backend, GpuBackend::Cpu);
        }
        Err(GpuError::NoDevicesAvailable) => {
            // Also acceptable - no GPU devices found
        }
        Err(e) => panic!("Unexpected error: {}", e),
    }
}
