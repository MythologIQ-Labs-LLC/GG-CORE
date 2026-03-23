//! Tests for GPU allocator trait and MockGpuAllocator.

use crate::engine::gpu::{GpuDevice, GpuError, GpuMemory};
use crate::engine::gpu_allocator::{GpuAllocator, MockGpuAllocator};
use std::sync::Arc;

#[test]
fn allocate_and_deallocate_all_returns_zero() {
    let alloc = MockGpuAllocator::new(4096, 0);
    let mut handles = Vec::new();
    for _ in 0..8 {
        handles.push(alloc.allocate(512).unwrap());
    }
    assert_eq!(alloc.allocated_bytes(), 4096);
    for h in &handles {
        alloc.deallocate(h).unwrap();
    }
    assert_eq!(alloc.allocated_bytes(), 0);
    assert_eq!(alloc.leak_count(), 0);
}

#[test]
fn raii_drop_deallocates_via_gpu_memory() {
    let alloc = Arc::new(MockGpuAllocator::new(4096, 0));
    let device = Arc::new(GpuDevice::cpu());
    let allocation = alloc.allocate(1024).unwrap();
    let mem = GpuMemory::new_allocated(device, allocation, alloc.clone());
    assert_eq!(alloc.allocated_bytes(), 1024);
    drop(mem);
    assert_eq!(alloc.allocated_bytes(), 0);
}

#[test]
fn beyond_capacity_returns_out_of_memory() {
    let alloc = MockGpuAllocator::new(1024, 0);
    let _a = alloc.allocate(512).unwrap();
    let result = alloc.allocate(1024);
    assert!(matches!(result, Err(GpuError::OutOfMemory { .. })));
}

#[test]
fn double_free_detection() {
    let alloc = MockGpuAllocator::new(4096, 0);
    let handle = alloc.allocate(256).unwrap();
    alloc.deallocate(&handle).unwrap();
    let result = alloc.deallocate(&handle);
    assert!(matches!(result, Err(GpuError::AllocationFailed(_))));
}

#[test]
fn stress_test_1000_cycles_zero_drift() {
    let alloc = MockGpuAllocator::new(1024 * 1024, 0);
    for _ in 0..1000 {
        let a = alloc.allocate(1024).unwrap();
        alloc.deallocate(&a).unwrap();
    }
    assert_eq!(alloc.allocated_bytes(), 0);
    assert_eq!(alloc.leak_count(), 0);
}

#[test]
fn leak_detection_reports_live_allocations() {
    let alloc = MockGpuAllocator::new(4096, 0);
    let _a = alloc.allocate(128).unwrap();
    let _b = alloc.allocate(256).unwrap();
    assert_eq!(alloc.leak_count(), 2);
    assert_eq!(alloc.allocated_bytes(), 384);
}

#[test]
fn allocation_ids_are_unique() {
    let alloc = MockGpuAllocator::new(4096, 0);
    let a = alloc.allocate(64).unwrap();
    let b = alloc.allocate(64).unwrap();
    assert_ne!(a.id, b.id);
}

#[test]
fn device_index_is_preserved() {
    let alloc = MockGpuAllocator::new(4096, 7);
    let a = alloc.allocate(64).unwrap();
    assert_eq!(a.device_index, 7);
}

#[test]
fn exact_capacity_allocation_succeeds() {
    let alloc = MockGpuAllocator::new(512, 0);
    let a = alloc.allocate(512).unwrap();
    assert_eq!(alloc.allocated_bytes(), 512);
    let over = alloc.allocate(1);
    assert!(matches!(over, Err(GpuError::OutOfMemory { .. })));
    alloc.deallocate(&a).unwrap();
    assert_eq!(alloc.allocated_bytes(), 0);
}
