// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! GPU Allocator trait and implementations (mock, CUDA stub, Metal stub).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::gpu::GpuError;

/// A handle representing a GPU memory allocation.
#[derive(Debug, Clone)]
pub struct GpuAllocation {
    pub id: u64,
    pub size: usize,
    pub device_index: usize,
}

/// Trait abstracting GPU memory allocation.
pub trait GpuAllocator: Send + Sync {
    fn allocate(&self, size: usize) -> Result<GpuAllocation, GpuError>;
    fn deallocate(&self, allocation: &GpuAllocation) -> Result<(), GpuError>;
    fn allocated_bytes(&self) -> usize;
}

// -- Mock allocator (testing + CPU fallback) ----------------------------------

struct MockState {
    allocations: HashMap<u64, usize>,
    total: usize,
}

/// Mock GPU allocator backed by a HashMap for testing.
pub struct MockGpuAllocator {
    capacity: usize,
    device_index: usize,
    next_id: AtomicU64,
    state: Mutex<MockState>,
}

impl MockGpuAllocator {
    pub fn new(capacity: usize, device_index: usize) -> Self {
        Self {
            capacity,
            device_index,
            next_id: AtomicU64::new(1),
            state: Mutex::new(MockState { allocations: HashMap::new(), total: 0 }),
        }
    }

    /// Count of live (un-freed) allocations — useful for leak detection.
    pub fn leak_count(&self) -> usize {
        self.state.lock().unwrap().allocations.len()
    }
}

impl GpuAllocator for MockGpuAllocator {
    fn allocate(&self, size: usize) -> Result<GpuAllocation, GpuError> {
        let mut s = self.state.lock().unwrap();
        if s.total + size > self.capacity {
            return Err(GpuError::OutOfMemory {
                required: size as u64,
                available: (self.capacity - s.total) as u64,
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        s.allocations.insert(id, size);
        s.total += size;
        Ok(GpuAllocation { id, size, device_index: self.device_index })
    }

    fn deallocate(&self, allocation: &GpuAllocation) -> Result<(), GpuError> {
        let mut s = self.state.lock().unwrap();
        match s.allocations.remove(&allocation.id) {
            Some(size) => { s.total -= size; Ok(()) }
            None => Err(GpuError::AllocationFailed(
                format!("double-free or unknown allocation id={}", allocation.id),
            )),
        }
    }

    fn allocated_bytes(&self) -> usize {
        self.state.lock().unwrap().total
    }
}

// -- CUDA allocator stub ------------------------------------------------------

#[cfg(feature = "cuda")]
pub struct CudaGpuAllocator {
    device_index: usize,
    next_id: AtomicU64,
    state: Mutex<MockState>,
    capacity: usize,
}

#[cfg(feature = "cuda")]
impl CudaGpuAllocator {
    pub fn new(device_index: usize, capacity: usize) -> Self {
        Self {
            device_index,
            next_id: AtomicU64::new(1),
            state: Mutex::new(MockState { allocations: HashMap::new(), total: 0 }),
            capacity,
        }
    }
}

#[cfg(feature = "cuda")]
impl GpuAllocator for CudaGpuAllocator {
    fn allocate(&self, size: usize) -> Result<GpuAllocation, GpuError> {
        // TODO: Replace with cudarc::driver::CudaDevice::alloc
        let mut s = self.state.lock().unwrap();
        if s.total + size > self.capacity {
            return Err(GpuError::OutOfMemory {
                required: size as u64,
                available: (self.capacity - s.total) as u64,
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        s.allocations.insert(id, size);
        s.total += size;
        Ok(GpuAllocation { id, size, device_index: self.device_index })
    }

    fn deallocate(&self, allocation: &GpuAllocation) -> Result<(), GpuError> {
        let mut s = self.state.lock().unwrap();
        match s.allocations.remove(&allocation.id) {
            Some(size) => { s.total -= size; Ok(()) }
            None => Err(GpuError::AllocationFailed(
                format!("double-free id={}", allocation.id),
            )),
        }
    }

    fn allocated_bytes(&self) -> usize {
        self.state.lock().unwrap().total
    }
}

// -- Metal allocator stub -----------------------------------------------------

#[cfg(feature = "metal")]
pub struct MetalGpuAllocator {
    device_index: usize,
    next_id: AtomicU64,
    state: Mutex<MockState>,
    capacity: usize,
}

#[cfg(feature = "metal")]
impl MetalGpuAllocator {
    pub fn new(device_index: usize, capacity: usize) -> Self {
        Self {
            device_index,
            next_id: AtomicU64::new(1),
            state: Mutex::new(MockState { allocations: HashMap::new(), total: 0 }),
            capacity,
        }
    }
}

#[cfg(feature = "metal")]
impl GpuAllocator for MetalGpuAllocator {
    fn allocate(&self, size: usize) -> Result<GpuAllocation, GpuError> {
        // TODO: Replace with metal::Device::new_buffer
        let mut s = self.state.lock().unwrap();
        if s.total + size > self.capacity {
            return Err(GpuError::OutOfMemory {
                required: size as u64,
                available: (self.capacity - s.total) as u64,
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        s.allocations.insert(id, size);
        s.total += size;
        Ok(GpuAllocation { id, size, device_index: self.device_index })
    }

    fn deallocate(&self, allocation: &GpuAllocation) -> Result<(), GpuError> {
        let mut s = self.state.lock().unwrap();
        match s.allocations.remove(&allocation.id) {
            Some(size) => { s.total -= size; Ok(()) }
            None => Err(GpuError::AllocationFailed(
                format!("double-free id={}", allocation.id),
            )),
        }
    }

    fn allocated_bytes(&self) -> usize {
        self.state.lock().unwrap().total
    }
}

#[cfg(test)]
#[path = "gpu_allocator_tests.rs"]
mod tests;
