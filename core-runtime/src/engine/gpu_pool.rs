// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! GPU Memory Pool for efficient allocation.
//!
//! Extracted from `gpu.rs` for Section 4 compliance (files <= 250 lines).

use std::sync::Arc;

use super::gpu::{GpuDevice, GpuError, GpuMemory};
use super::gpu_allocator::GpuAllocator;

/// GPU Memory Pool for efficient allocation via a `GpuAllocator`.
pub struct GpuMemoryPool {
    device: Arc<GpuDevice>,
    allocator: Arc<dyn GpuAllocator>,
    total_allocated: u64,
    max_size: u64,
}

impl GpuMemoryPool {
    /// Create a new memory pool backed by an allocator.
    pub fn new(
        device: Arc<GpuDevice>,
        max_size: u64,
        allocator: Arc<dyn GpuAllocator>,
    ) -> Self {
        Self {
            device,
            allocator,
            total_allocated: 0,
            max_size,
        }
    }

    /// Allocate from pool via the underlying allocator.
    pub fn allocate(&mut self, size: u64) -> Result<GpuMemory, GpuError> {
        if self.total_allocated + size > self.max_size {
            return Err(GpuError::OutOfMemory {
                required: size,
                available: self.max_size - self.total_allocated,
            });
        }

        let allocation = self.allocator.allocate(size as usize)?;
        self.total_allocated += size;

        Ok(GpuMemory::new_allocated(
            self.device.clone(),
            allocation,
            self.allocator.clone(),
        ))
    }

    /// Get pool utilization
    pub fn utilization(&self) -> f32 {
        if self.max_size == 0 {
            return 0.0;
        }
        self.total_allocated as f32 / self.max_size as f32
    }

    /// Total bytes allocated through this pool.
    pub fn total_allocated(&self) -> u64 {
        self.total_allocated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::gpu_allocator::MockGpuAllocator;

    #[test]
    fn test_gpu_memory_pool() {
        let device = Arc::new(GpuDevice::cpu());
        let allocator = Arc::new(MockGpuAllocator::new(4096, 0));
        let mut pool = GpuMemoryPool::new(device, 1024, allocator);

        let mem = pool.allocate(512).unwrap();
        assert_eq!(mem.size, 512);
        assert_eq!(pool.utilization(), 0.5);
    }

    #[test]
    fn test_gpu_memory_pool_out_of_memory() {
        let device = Arc::new(GpuDevice::cpu());
        let allocator = Arc::new(MockGpuAllocator::new(4096, 0));
        let mut pool = GpuMemoryPool::new(device, 1024, allocator);

        let _mem = pool.allocate(512).unwrap();
        let result = pool.allocate(1024);

        assert!(matches!(result, Err(GpuError::OutOfMemory { .. })));
    }
}
