// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Metal Backend Implementation (macOS only)
//!
//! Provides GPU acceleration using Apple Metal on macOS. This module implements
//! device detection, memory management, and kernel execution for Apple Silicon GPUs.

use std::sync::Arc;
use thiserror::Error;

use super::{GpuBackend, GpuDevice, GpuError};

/// Metal-specific error types
#[derive(Debug, Error)]
pub enum MetalError {
    #[error("Metal framework not available (requires macOS)")]
    NotAvailable,

    #[error("No Metal-capable devices found")]
    NoDevicesFound,

    #[error("Metal device {index} not found: {reason}")]
    DeviceNotFound { index: usize, reason: String },

    #[error("Metal buffer allocation failed: {0}")]
    AllocationFailed(String),

    #[error("Metal buffer copy failed: {0}")]
    CopyFailed(String),

    #[error("Metal kernel compilation failed: {0}")]
    KernelCompilationFailed(String),

    #[error("Metal command buffer submission failed: {0}")]
    CommandSubmissionFailed(String),

    #[error("Metal error: {0}")]
    Other(String),
}

/// Metal Device Information (extended from base GpuDevice)
#[derive(Debug)]
pub struct MetalDeviceInfo {
    /// Base GPU device info
    pub device: GpuDevice,
    /// Whether this is Apple Silicon (unified memory)
    pub is_apple_silicon: bool,
    /// Maximum threadgroup memory size
    pub max_threadgroup_memory_length: u64,
    /// Maximum threads per threadgroup
    pub max_threads_per_threadgroup: u32,
    /// Recommended max working set size
    pub recommended_max_working_set_size: u64,
    /// Current allocated size
    pub current_allocated_size: u64,
    /// Whether the device supports ray tracing
    pub supports_ray_tracing: bool,
    /// Whether the device supports mesh shaders
    pub supports_mesh_shaders: bool,
    /// GPU family type
    pub gpu_family: MetalGpuFamily,
    /// Architecture name
    pub architecture: String,
}

/// Metal GPU Family classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetalGpuFamily {
    /// Apple-designed GPU (M1, M2, M3 series)
    Apple,
    /// Mac GPU (AMD/NVIDIA on Intel Macs)
    Mac,
    /// Common (supports both)
    Common,
    /// Metal 3 capable
    Metal3,
    /// Unknown
    Unknown,
}

impl MetalDeviceInfo {
    /// Check if device supports unified memory (zero-copy between CPU and GPU)
    pub fn supports_unified_memory(&self) -> bool {
        self.is_apple_silicon
    }

    /// Check if device supports Metal 3 features
    pub fn supports_metal3(&self) -> bool {
        self.gpu_family == MetalGpuFamily::Metal3
    }

    /// Get optimal buffer alignment for this device
    pub fn buffer_alignment(&self) -> usize {
        // Metal typically uses 256-byte alignment
        256
    }
}

// Metal bindings using the metal crate
#[cfg(all(feature = "metal", target_os = "macos"))]
mod metal_impl {
    use super::*;
    use metal::*;

    /// Metal Backend - manages Metal devices and memory
    pub struct MetalBackend {
        /// Available Metal devices
        devices: Vec<MetalDeviceInfo>,
        /// Active Metal device
        active_device: Option<Device>,
        /// Active device index
        active_index: Option<usize>,
        /// Command queue for the active device
        command_queue: Option<CommandQueue>,
    }

    impl MetalBackend {
        /// Create a new Metal backend
        pub fn new() -> Result<Self, MetalError> {
            let mut backend = Self {
                devices: Vec::new(),
                active_device: None,
                active_index: None,
                command_queue: None,
            };

            backend.detect_devices()?;
            Ok(backend)
        }

        /// Detect all available Metal devices
        pub fn detect_devices(&mut self) -> Result<(), MetalError> {
            self.devices.clear();

            // Get all Metal devices
            let devices = Device::all();

            if devices.is_empty() {
                return Err(MetalError::NoDevicesFound);
            }

            for (index, device) in devices.iter().enumerate() {
                let info = self.query_device_info(index, device);
                self.devices.push(info);
            }

            Ok(())
        }

        /// Query detailed device information
        fn query_device_info(&self, index: usize, device: &Device) -> MetalDeviceInfo {
            let name = device.name().to_string();

            // Determine if this is Apple Silicon
            let is_apple_silicon = name.contains("Apple") || device.has_unified_memory();

            // Get memory information
            let recommended_max = device.recommended_max_working_set_size();
            let current_allocated = device.current_allocated_size();

            // Get feature support
            let supports_ray_tracing = device.supports_ray_tracing();
            let supports_mesh_shaders = device.supports_mesh_shaders();

            // Determine GPU family
            let gpu_family = if device.supports_family(MTLGPUFamilyAppleMetal3) {
                MetalGpuFamily::Metal3
            } else if device.supports_family(MTLGPUFamilyApple1)
                || device.supports_family(MTLGPUFamilyApple2)
                || device.supports_family(MTLGPUFamilyApple3)
                || device.supports_family(MTLGPUFamilyApple4)
                || device.supports_family(MTLGPUFamilyApple5)
                || device.supports_family(MTLGPUFamilyApple6)
                || device.supports_family(MTLGPUFamilyApple7)
                || device.supports_family(MTLGPUFamilyApple8)
                || device.supports_family(MTLGPUFamilyApple9)
            {
                MetalGpuFamily::Apple
            } else if device.supports_family(MTLGPUFamilyMac2) {
                MetalGpuFamily::Mac
            } else {
                MetalGpuFamily::Unknown
            };

            // Get architecture name
            let architecture = if is_apple_silicon {
                if name.contains("M3") {
                    "Apple M3".to_string()
                } else if name.contains("M2") {
                    "Apple M2".to_string()
                } else if name.contains("M1") {
                    "Apple M1".to_string()
                } else {
                    "Apple Silicon".to_string()
                }
            } else {
                name.clone()
            };

            MetalDeviceInfo {
                device: GpuDevice {
                    backend: GpuBackend::Metal,
                    index,
                    name,
                    total_memory: recommended_max,
                    available_memory: recommended_max.saturating_sub(current_allocated),
                    compute_capability: None, // Metal doesn't use compute capability
                },
                is_apple_silicon,
                max_threadgroup_memory_length: device.max_threadgroup_memory_length() as u64,
                max_threads_per_threadgroup: device.max_threads_per_threadgroup(),
                recommended_max_working_set_size: recommended_max,
                current_allocated_size: current_allocated,
                supports_ray_tracing,
                supports_mesh_shaders,
                gpu_family,
                architecture,
            }
        }

        /// Select and initialize a Metal device
        pub fn select_device(&mut self, index: usize) -> Result<&Device, MetalError> {
            if index >= self.devices.len() {
                return Err(MetalError::DeviceNotFound {
                    index,
                    reason: "Device index out of range".to_string(),
                });
            }

            let devices = Device::all();
            let device =
                devices
                    .into_iter()
                    .nth(index)
                    .ok_or_else(|| MetalError::DeviceNotFound {
                        index,
                        reason: "Failed to get device".to_string(),
                    })?;

            // Create command queue
            let queue = device.new_command_queue();

            self.active_device = Some(device);
            self.active_index = Some(index);
            self.command_queue = Some(queue);

            Ok(self.active_device.as_ref().unwrap())
        }

        /// Get the active device
        pub fn active_device(&self) -> Option<&Device> {
            self.active_device.as_ref()
        }

        /// Get the command queue
        pub fn command_queue(&self) -> Option<&CommandQueue> {
            self.command_queue.as_ref()
        }

        /// Get all detected devices
        pub fn devices(&self) -> &[MetalDeviceInfo] {
            &self.devices
        }

        /// Get number of devices
        pub fn device_count(&self) -> usize {
            self.devices.len()
        }

        /// Check if Metal is available
        pub fn is_available() -> bool {
            !Device::all().is_empty()
        }
    }

    /// Metal Buffer - manages GPU memory allocation
    pub struct MetalBuffer {
        /// The Metal buffer
        buffer: Buffer,
        /// Size in bytes
        size: usize,
        /// Owning device
        device: Device,
    }

    impl MetalBuffer {
        /// Allocate a new buffer on the device
        pub fn new(device: &Device, size: usize) -> Result<Self, MetalError> {
            let buffer = device.new_buffer(size, MTLResourceOptions::StorageModeShared);

            if buffer.length() != size {
                return Err(MetalError::AllocationFailed(format!(
                    "Requested {} bytes, got {} bytes",
                    size,
                    buffer.length()
                )));
            }

            Ok(Self {
                buffer,
                size,
                device: device.clone(),
            })
        }

        /// Allocate a buffer with unsafe storage mode (for Apple Silicon unified memory)
        pub fn new_unified(device: &Device, size: usize) -> Result<Self, MetalError> {
            // StorageModeMemoryless is for tile memory, use Shared for unified
            let buffer = device.new_buffer(size, MTLResourceOptions::StorageModeShared);

            Ok(Self {
                buffer,
                size,
                device: device.clone(),
            })
        }

        /// Get the buffer contents as a slice
        pub fn contents(&self) -> &[u8] {
            // Safety: Metal guarantees the buffer contents are valid for the lifetime of the buffer
            unsafe { std::slice::from_raw_parts(self.buffer.contents() as *const u8, self.size) }
        }

        /// Get the buffer contents as a mutable slice
        pub fn contents_mut(&mut self) -> &mut [u8] {
            // Safety: Metal guarantees the buffer contents are valid for the lifetime of the buffer
            unsafe { std::slice::from_raw_parts_mut(self.buffer.contents() as *mut u8, self.size) }
        }

        /// Get the Metal buffer reference
        pub fn as_buffer(&self) -> &Buffer {
            &self.buffer
        }

        /// Get the size in bytes
        pub fn size(&self) -> usize {
            self.size
        }

        /// Copy data into the buffer
        pub fn copy_from(&mut self, data: &[u8]) -> Result<(), MetalError> {
            if data.len() != self.size {
                return Err(MetalError::CopyFailed(format!(
                    "Size mismatch: source {} bytes, buffer {} bytes",
                    data.len(),
                    self.size
                )));
            }

            self.contents_mut().copy_from_slice(data);
            Ok(())
        }

        /// Copy data out of the buffer
        pub fn copy_to(&self, dest: &mut [u8]) -> Result<(), MetalError> {
            if dest.len() != self.size {
                return Err(MetalError::CopyFailed(format!(
                    "Size mismatch: buffer {} bytes, dest {} bytes",
                    self.size,
                    dest.len()
                )));
            }

            dest.copy_from_slice(self.contents());
            Ok(())
        }
    }

    /// Metal Compute Pipeline for kernel execution
    pub struct MetalComputePipeline {
        /// The compute pipeline state
        pipeline_state: ComputePipelineState,
        /// Owning device
        device: Device,
    }

    impl MetalComputePipeline {
        /// Create a compute pipeline from a Metal Shading Language (MSL) source
        pub fn from_source(
            device: &Device,
            source: &str,
            function_name: &str,
        ) -> Result<Self, MetalError> {
            let library = device
                .new_library_with_source(source, &CompileOptions::new())
                .map_err(|e| {
                    MetalError::KernelCompilationFailed(format!(
                        "Library compilation failed: {:?}",
                        e
                    ))
                })?;

            let function = library.get_function(function_name, None).map_err(|e| {
                MetalError::KernelCompilationFailed(format!(
                    "Function '{}' not found: {:?}",
                    function_name, e
                ))
            })?;

            let pipeline_state = device
                .new_compute_pipeline_state_with_function(&function)
                .map_err(|e| {
                    MetalError::KernelCompilationFailed(format!(
                        "Pipeline creation failed: {:?}",
                        e
                    ))
                })?;

            Ok(Self {
                pipeline_state,
                device: device.clone(),
            })
        }

        /// Get the pipeline state
        pub fn pipeline_state(&self) -> &ComputePipelineState {
            &self.pipeline_state
        }
    }

    /// Metal Command Buffer wrapper for kernel dispatch
    pub struct MetalCommandEncoder<'a> {
        /// The command buffer
        command_buffer: CommandBuffer,
        /// The compute command encoder
        encoder: ComputeCommandEncoder<'a>,
    }

    impl<'a> MetalCommandEncoder<'a> {
        /// Create a new command encoder
        pub fn new(queue: &CommandQueue) -> Result<Self, MetalError> {
            let command_buffer = queue.new_command_buffer();
            let encoder = command_buffer.new_compute_command_encoder();

            Ok(Self {
                command_buffer,
                encoder,
            })
        }

        /// Set the compute pipeline
        pub fn set_pipeline(&mut self, pipeline: &MetalComputePipeline) {
            self.encoder
                .set_compute_pipeline_state(pipeline.pipeline_state());
        }

        /// Set a buffer argument
        pub fn set_buffer(&mut self, index: usize, buffer: &MetalBuffer) {
            self.encoder.set_buffer(index, Some(buffer.as_buffer()), 0);
        }

        /// Set bytes directly (for small values)
        pub fn set_bytes<T>(&mut self, index: usize, value: &T) {
            self.encoder.set_bytes(
                index,
                std::mem::size_of::<T>() as u64,
                value as *const T as *const _,
            );
        }

        /// Dispatch threadgroups
        pub fn dispatch(&mut self, threadgroups: MTLSize, threads_per_threadgroup: MTLSize) {
            self.encoder
                .dispatch_thread_groups(threadgroups, threads_per_threadgroup);
        }

        /// End encoding and commit
        pub fn commit(self) -> Result<(), MetalError> {
            self.encoder.end_encoding();
            self.command_buffer.commit();
            self.command_buffer.wait_until_completed();
            Ok(())
        }
    }
}

// Re-export the implementation types when Metal is available
#[cfg(all(feature = "metal", target_os = "macos"))]
pub use metal_impl::*;

// Non-Metal fallback stubs
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub struct MetalBackend;

#[cfg(not(all(feature = "metal", target_os = "macos")))]
impl MetalBackend {
    pub fn new() -> Result<Self, MetalError> {
        Err(MetalError::NotAvailable)
    }

    pub fn is_available() -> bool {
        false
    }

    pub fn device_count(&self) -> usize {
        0
    }
}

#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub struct MetalBuffer;

#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub struct MetalComputePipeline;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metal_device_info_unified_memory() {
        let info = MetalDeviceInfo {
            device: GpuDevice::cpu(),
            is_apple_silicon: true,
            max_threadgroup_memory_length: 32768,
            max_threads_per_threadgroup: 1024,
            recommended_max_working_set_size: 17179869184, // 16 GB
            current_allocated_size: 0,
            supports_ray_tracing: true,
            supports_mesh_shaders: true,
            gpu_family: MetalGpuFamily::Metal3,
            architecture: "Apple M3".to_string(),
        };

        assert!(info.supports_unified_memory());
        assert!(info.supports_metal3());
        assert_eq!(info.buffer_alignment(), 256);
    }

    #[test]
    fn test_metal_device_info_intel_mac() {
        let info = MetalDeviceInfo {
            device: GpuDevice::cpu(),
            is_apple_silicon: false,
            max_threadgroup_memory_length: 32768,
            max_threads_per_threadgroup: 1024,
            recommended_max_working_set_size: 8589934592, // 8 GB
            current_allocated_size: 0,
            supports_ray_tracing: false,
            supports_mesh_shaders: false,
            gpu_family: MetalGpuFamily::Mac,
            architecture: "AMD Radeon Pro".to_string(),
        };

        assert!(!info.supports_unified_memory());
        assert!(!info.supports_metal3());
    }

    #[test]
    fn test_metal_gpu_family() {
        assert_eq!(MetalGpuFamily::Apple, MetalGpuFamily::Apple);
        assert_ne!(MetalGpuFamily::Apple, MetalGpuFamily::Mac);
        assert!(MetalGpuFamily::Metal3 == MetalGpuFamily::Metal3);
    }

    #[test]
    fn test_metal_error_display() {
        let err = MetalError::NotAvailable;
        assert!(format!("{}", err).contains("not available"));

        let err = MetalError::NoDevicesFound;
        assert!(format!("{}", err).contains("No Metal-capable"));

        let err = MetalError::DeviceNotFound {
            index: 0,
            reason: "test".to_string(),
        };
        assert!(format!("{}", err).contains("Device 0"));
    }

    #[test]
    #[cfg(not(all(feature = "metal", target_os = "macos")))]
    fn test_metal_backend_unavailable_without_feature() {
        assert!(!MetalBackend::is_available());
        assert!(MetalBackend::new().is_err());
    }
}
