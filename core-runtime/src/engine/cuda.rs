// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! CUDA Backend Implementation
//!
//! Provides GPU acceleration using NVIDIA CUDA. This module implements
//! device detection, memory management, and kernel execution for CUDA GPUs.

#[cfg(feature = "cuda")]
use cudarc::driver::{
    CudaContext, CudaDevice, CudaFunction, CudaModule, CudaStream, DevicePtr, DeviceRepr,
    LaunchArgs, LaunchConfig,
};
#[cfg(feature = "cuda")]
use cudarc::nvrtc::Ptx;

use std::sync::Arc;
use thiserror::Error;

use super::{GpuBackend, GpuDevice, GpuError};

/// CUDA-specific error types
#[derive(Debug, Error)]
pub enum CudaError {
    #[error("CUDA driver not installed or not found")]
    DriverNotFound,

    #[error("No CUDA-capable devices detected")]
    NoDevicesDetected,

    #[error("CUDA device {index} not found: {reason}")]
    DeviceNotFound { index: usize, reason: String },

    #[error("CUDA memory allocation failed: {0}")]
    AllocationFailed(String),

    #[error("CUDA memory copy failed: {0}")]
    CopyFailed(String),

    #[error("CUDA kernel launch failed: {0}")]
    KernelLaunchFailed(String),

    #[error("CUDA module load failed: {0}")]
    ModuleLoadFailed(String),

    #[error("CUDA context creation failed: {0}")]
    ContextCreationFailed(String),

    #[error("CUDA error: {0}")]
    Other(String),
}

/// CUDA Device Information (extended from base GpuDevice)
#[derive(Debug)]
pub struct CudaDeviceInfo {
    /// Base GPU device info
    pub device: GpuDevice,
    /// CUDA compute capability major version
    pub compute_major: u32,
    /// CUDA compute capability minor version
    pub compute_minor: u32,
    /// Number of streaming multiprocessors
    pub multiprocessor_count: u32,
    /// Maximum threads per block
    pub max_threads_per_block: u32,
    /// Maximum shared memory per block
    pub max_shared_memory_per_block: u64,
    /// Memory clock rate in kHz
    pub memory_clock_rate: u32,
    /// Memory bus width in bits
    pub memory_bus_width: u32,
    /// L2 cache size in bytes
    pub l2_cache_size: u64,
    /// Maximum clock rate in kHz
    pub max_clock_rate: u32,
    /// Whether the device supports unified memory
    pub unified_memory: bool,
    /// Whether the device supports concurrent kernels
    pub concurrent_kernels: bool,
    /// Whether the device supports cooperative launch
    pub cooperative_launch: bool,
}

impl CudaDeviceInfo {
    /// Check if device supports a minimum compute capability
    pub fn supports_compute(&self, major: u32, minor: u32) -> bool {
        (self.compute_major, self.compute_minor) >= (major, minor)
    }

    /// Check if device supports flash attention (requires compute 8.0+)
    pub fn supports_flash_attention(&self) -> bool {
        self.supports_compute(8, 0)
    }

    /// Get theoretical memory bandwidth in GB/s
    pub fn theoretical_bandwidth_gbps(&self) -> f64 {
        let memory_clock_mhz = self.memory_clock_rate as f64 / 1000.0;
        let bus_width_bytes = self.memory_bus_width as f64 / 8.0;
        // DDR doubles the effective rate
        (2.0 * memory_clock_mhz * bus_width_bytes) / 1_000_000.0
    }
}

/// CUDA Backend - manages CUDA devices and memory
#[cfg(feature = "cuda")]
pub struct CudaBackend {
    /// Available CUDA devices
    devices: Vec<CudaDeviceInfo>,
    /// Active CUDA device handle
    active_device: Option<Arc<CudaDevice>>,
    /// Active device index
    active_index: Option<usize>,
}

#[cfg(feature = "cuda")]
impl CudaBackend {
    /// Create a new CUDA backend
    pub fn new() -> Result<Self, CudaError> {
        let mut backend = Self {
            devices: Vec::new(),
            active_device: None,
            active_index: None,
        };

        backend.detect_devices()?;
        Ok(backend)
    }

    /// Detect all available CUDA devices
    pub fn detect_devices(&mut self) -> Result<(), CudaError> {
        self.devices.clear();

        // Get number of CUDA devices
        let count = match CudaDevice::count() {
            Ok(n) => n,
            Err(e) => {
                // CUDA driver not available
                return Err(CudaError::DriverNotFound);
            }
        };

        if count == 0 {
            return Err(CudaError::NoDevicesDetected);
        }

        // Query each device
        for i in 0..count {
            if let Ok(device) = CudaDevice::new(i as i32) {
                let info = self.query_device_info(i, &device);
                self.devices.push(info);
            }
        }

        Ok(())
    }

    /// Query detailed device information
    fn query_device_info(&self, index: usize, device: &CudaDevice) -> CudaDeviceInfo {
        // Get device attributes using cudarc
        let (compute_major, compute_minor) = device.compute_capability().unwrap_or((0, 0));
        let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        let total_memory = device.total_memory().unwrap_or(0);

        // Get additional attributes
        let multiprocessor_count = device
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT,
            )
            .unwrap_or(0) as u32;

        let max_threads_per_block = device
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MAX_THREADS_PER_BLOCK,
            )
            .unwrap_or(0) as u32;

        let max_shared_memory_per_block = device.attribute(
            cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MAX_SHARED_MEMORY_PER_BLOCK
        ).unwrap_or(0) as u64;

        let memory_clock_rate = device
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MEMORY_CLOCK_RATE,
            )
            .unwrap_or(0) as u32;

        let memory_bus_width = device.attribute(
            cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_GLOBAL_MEMORY_BUS_WIDTH
        ).unwrap_or(0) as u32;

        let l2_cache_size = device
            .attribute(cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_L2_CACHE_SIZE)
            .unwrap_or(0) as u64;

        let max_clock_rate = device
            .attribute(cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_CLOCK_RATE)
            .unwrap_or(0) as u32;

        let unified_memory = device
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_UNIFIED_ADDRESSING,
            )
            .unwrap_or(0)
            != 0;

        let concurrent_kernels = device
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_CONCURRENT_KERNELS,
            )
            .unwrap_or(0)
            != 0;

        let cooperative_launch = device
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_COOPERATIVE_LAUNCH,
            )
            .unwrap_or(0)
            != 0;

        // Get available memory (free memory)
        let available_memory = device.available_memory().unwrap_or(total_memory);

        CudaDeviceInfo {
            device: GpuDevice {
                backend: GpuBackend::Cuda,
                index,
                name,
                total_memory,
                available_memory,
                compute_capability: Some((compute_major, compute_minor)),
            },
            compute_major,
            compute_minor,
            multiprocessor_count,
            max_threads_per_block,
            max_shared_memory_per_block,
            memory_clock_rate,
            memory_bus_width,
            l2_cache_size,
            max_clock_rate,
            unified_memory,
            concurrent_kernels,
            cooperative_launch,
        }
    }

    /// Select and initialize a CUDA device
    pub fn select_device(&mut self, index: usize) -> Result<Arc<CudaDevice>, CudaError> {
        if index >= self.devices.len() {
            return Err(CudaError::DeviceNotFound {
                index,
                reason: "Device index out of range".to_string(),
            });
        }

        let device = CudaDevice::new(index as i32).map_err(|e| CudaError::DeviceNotFound {
            index,
            reason: format!("Failed to initialize device: {:?}", e),
        })?;

        self.active_device = Some(device.clone());
        self.active_index = Some(index);

        Ok(device)
    }

    /// Get the active device
    pub fn active_device(&self) -> Option<&Arc<CudaDevice>> {
        self.active_device.as_ref()
    }

    /// Get all detected devices
    pub fn devices(&self) -> &[CudaDeviceInfo] {
        &self.devices
    }

    /// Get number of devices
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Check if any CUDA device is available
    pub fn is_available() -> bool {
        CudaDevice::count().map(|n| n > 0).unwrap_or(false)
    }
}

/// CUDA Memory Buffer - manages GPU memory allocation
#[cfg(feature = "cuda")]
pub struct CudaMemoryBuffer {
    /// Device pointer
    device_ptr: DevicePtr,
    /// Size in bytes
    size: usize,
    /// Owning device
    device: Arc<CudaDevice>,
}

#[cfg(feature = "cuda")]
impl CudaMemoryBuffer {
    /// Allocate a new buffer on the device
    pub fn alloc(device: Arc<CudaDevice>, size: usize) -> Result<Self, CudaError> {
        let device_ptr = device.alloc::<u8>(size).map_err(|e| {
            CudaError::AllocationFailed(format!("Failed to allocate {} bytes: {:?}", size, e))
        })?;

        Ok(Self {
            device_ptr,
            size,
            device,
        })
    }

    /// Get the device pointer
    pub fn as_ptr(&self) -> DevicePtr {
        self.device_ptr
    }

    /// Get the size in bytes
    pub fn size(&self) -> usize {
        self.size
    }

    /// Copy data from host to device
    pub fn copy_from_host(&self, data: &[u8]) -> Result<(), CudaError> {
        if data.len() != self.size {
            return Err(CudaError::CopyFailed(format!(
                "Size mismatch: host {} bytes, device {} bytes",
                data.len(),
                self.size
            )));
        }

        self.device
            .htod_copy(self.device_ptr, data)
            .map_err(|e| CudaError::CopyFailed(format!("Host to device copy failed: {:?}", e)))?;

        Ok(())
    }

    /// Copy data from device to host
    pub fn copy_to_host(&self, dest: &mut [u8]) -> Result<(), CudaError> {
        if dest.len() != self.size {
            return Err(CudaError::CopyFailed(format!(
                "Size mismatch: host {} bytes, device {} bytes",
                dest.len(),
                self.size
            )));
        }

        self.device
            .dtoth_copy(self.device_ptr, dest)
            .map_err(|e| CudaError::CopyFailed(format!("Device to host copy failed: {:?}", e)))?;

        Ok(())
    }

    /// Copy data from another device buffer
    pub fn copy_from_device(&self, src: &CudaMemoryBuffer) -> Result<(), CudaError> {
        if src.size != self.size {
            return Err(CudaError::CopyFailed(format!(
                "Size mismatch: src {} bytes, dest {} bytes",
                src.size, self.size
            )));
        }

        self.device
            .dtod_copy(self.device_ptr, src.device_ptr, self.size)
            .map_err(|e| CudaError::CopyFailed(format!("Device to device copy failed: {:?}", e)))?;

        Ok(())
    }
}

#[cfg(feature = "cuda")]
impl Drop for CudaMemoryBuffer {
    fn drop(&mut self) {
        // Memory is automatically freed when the device is dropped
        // cudarc handles this via Arc reference counting
    }
}

/// CUDA Stream for asynchronous execution
#[cfg(feature = "cuda")]
pub struct CudaExecutionStream {
    /// The CUDA stream
    stream: CudaStream,
    /// Owning device
    device: Arc<CudaDevice>,
}

#[cfg(feature = "cuda")]
impl CudaExecutionStream {
    /// Create a new execution stream
    pub fn new(device: Arc<CudaDevice>) -> Result<Self, CudaError> {
        let stream = device.new_stream().map_err(|e| {
            CudaError::ContextCreationFailed(format!("Failed to create stream: {:?}", e))
        })?;

        Ok(Self { stream, device })
    }

    /// Synchronize the stream
    pub fn synchronize(&self) -> Result<(), CudaError> {
        self.stream.synchronize().map_err(|e| {
            CudaError::KernelLaunchFailed(format!("Stream synchronization failed: {:?}", e))
        })
    }
}

/// Flash Attention CUDA kernel support
#[cfg(feature = "cuda")]
pub struct FlashAttention {
    /// Device for execution
    device: Arc<CudaDevice>,
    /// Loaded kernel module
    module: Option<Arc<CudaModule>>,
    /// Minimum compute capability required
    min_compute: (u32, u32),
}

#[cfg(feature = "cuda")]
impl FlashAttention {
    /// Create a new Flash Attention instance
    pub fn new(device: Arc<CudaDevice>) -> Result<Self, CudaError> {
        // Flash Attention requires compute capability 8.0+
        let (major, minor) = device.compute_capability().unwrap_or((0, 0));

        if (major, minor) < (8, 0) {
            return Err(CudaError::Other(
                "Flash Attention requires compute capability 8.0 or higher".to_string(),
            ));
        }

        Ok(Self {
            device,
            module: None,
            min_compute: (8, 0),
        })
    }

    /// Check if flash attention is supported on the device
    pub fn is_supported(&self) -> bool {
        let (major, minor) = self.device.compute_capability().unwrap_or((0, 0));
        (major, minor) >= self.min_compute
    }

    /// Load the flash attention kernel
    pub fn load_kernel(&mut self, ptx: &str) -> Result<(), CudaError> {
        let module = self
            .device
            .load_ptx(ptx.into(), "flash_attention", &[])
            .map_err(|e| {
                CudaError::ModuleLoadFailed(format!(
                    "Failed to load flash attention kernel: {:?}",
                    e
                ))
            })?;

        self.module = Some(module);
        Ok(())
    }
}

// Non-CUDA fallback stubs
#[cfg(not(feature = "cuda"))]
pub struct CudaBackend;

#[cfg(not(feature = "cuda"))]
impl CudaBackend {
    pub fn new() -> Result<Self, CudaError> {
        Err(CudaError::DriverNotFound)
    }

    pub fn is_available() -> bool {
        false
    }

    pub fn device_count(&self) -> usize {
        0
    }
}

#[cfg(not(feature = "cuda"))]
pub struct CudaMemoryBuffer;

#[cfg(not(feature = "cuda"))]
pub struct CudaDeviceInfo {
    pub device: GpuDevice,
    pub compute_major: u32,
    pub compute_minor: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_cuda_device_info_bandwidth() {
        let info = CudaDeviceInfo {
            device: GpuDevice::cpu(),
            compute_major: 8,
            compute_minor: 6,
            multiprocessor_count: 108,
            max_threads_per_block: 1024,
            max_shared_memory_per_block: 49152,
            memory_clock_rate: 5001, // 5001 MHz
            memory_bus_width: 384,   // 384 bits
            l2_cache_size: 12582912,
            max_clock_rate: 1830,
            unified_memory: true,
            concurrent_kernels: true,
            cooperative_launch: true,
        };

        // Theoretical bandwidth = 2 * 5001 * (384/8) / 1000000 GB/s
        let bandwidth = info.theoretical_bandwidth_gbps();
        assert!(bandwidth > 400.0); // Should be around 480 GB/s
        assert!(bandwidth < 600.0);
    }

    #[test]
    fn test_cuda_error_display() {
        let err = CudaError::DriverNotFound;
        assert!(format!("{}", err).contains("not installed"));

        let err = CudaError::NoDevicesDetected;
        assert!(format!("{}", err).contains("No CUDA-capable"));

        let err = CudaError::DeviceNotFound {
            index: 2,
            reason: "test".to_string(),
        };
        assert!(format!("{}", err).contains("Device 2"));
    }

    #[test]
    #[cfg(not(feature = "cuda"))]
    fn test_cuda_backend_unavailable_without_feature() {
        assert!(!CudaBackend::is_available());
        assert!(CudaBackend::new().is_err());
    }
}
