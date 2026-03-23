//! GPU allocation benchmarks.
//!
//! Measures MockGpuAllocator throughput, GpuMemoryPool contention,
//! and RAII GpuMemory drop overhead.

use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use gg_core::engine::gpu::{GpuDevice, GpuMemory};
use gg_core::engine::gpu_allocator::{GpuAllocator, MockGpuAllocator};
use gg_core::engine::gpu_pool::GpuMemoryPool;

fn bench_alloc_dealloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_alloc_dealloc");
    let allocator = MockGpuAllocator::new(1 << 30, 0); // 1 GB

    for &size in &[1024, 4096, 1 << 20] {
        group.bench_with_input(
            BenchmarkId::new("allocate_deallocate", size),
            &size,
            |b, &sz| {
                b.iter(|| {
                    let alloc = allocator.allocate(black_box(sz)).unwrap();
                    allocator.deallocate(&alloc).unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_rapid_alloc_burst(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_alloc_burst");

    for &count in &[10, 100, 500] {
        group.bench_with_input(
            BenchmarkId::new("burst", count),
            &count,
            |b, &n| {
                let allocator = MockGpuAllocator::new(n * 4096 + 4096, 0);
                b.iter(|| {
                    let allocs: Vec<_> = (0..n)
                        .map(|_| allocator.allocate(4096).unwrap())
                        .collect();
                    for a in &allocs {
                        allocator.deallocate(a).unwrap();
                    }
                    black_box(allocs.len())
                });
            },
        );
    }

    group.finish();
}

fn bench_pool_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_pool_allocation");

    for &size in &[1024_u64, 4096, 65536] {
        group.bench_with_input(
            BenchmarkId::new("pool_alloc", size),
            &size,
            |b, &sz| {
                b.iter_with_setup(
                    || {
                        let device = Arc::new(GpuDevice::cpu());
                        let allocator: Arc<dyn GpuAllocator> =
                            Arc::new(MockGpuAllocator::new(1 << 30, 0));
                        GpuMemoryPool::new(device, 1 << 28, allocator)
                    },
                    |mut pool| {
                        let _mem = pool.allocate(black_box(sz)).unwrap();
                    },
                );
            },
        );
    }

    group.finish();
}

fn bench_pool_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_pool_contention");
    group.sample_size(30);

    for &threads in &[2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("threads", threads),
            &threads,
            |b, &n| {
                b.iter(|| {
                    let allocator: Arc<dyn GpuAllocator> =
                        Arc::new(MockGpuAllocator::new(1 << 30, 0));
                    let alloc_clone = allocator.clone();
                    let handles: Vec<_> = (0..n)
                        .map(|_| {
                            let a = alloc_clone.clone();
                            std::thread::spawn(move || {
                                for _ in 0..100 {
                                    let al = a.allocate(1024).unwrap();
                                    a.deallocate(&al).unwrap();
                                }
                            })
                        })
                        .collect();
                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_raii_drop(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_raii_drop");

    for &size in &[1024_usize, 1 << 20] {
        group.bench_with_input(
            BenchmarkId::new("drop_cost", size),
            &size,
            |b, &sz| {
                let device = Arc::new(GpuDevice::cpu());
                let allocator: Arc<dyn GpuAllocator> =
                    Arc::new(MockGpuAllocator::new(1 << 30, 0));
                b.iter(|| {
                    let alloc = allocator.allocate(sz).unwrap();
                    let mem = GpuMemory::new_allocated(
                        device.clone(),
                        alloc,
                        allocator.clone(),
                    );
                    drop(black_box(mem));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_alloc_dealloc,
    bench_rapid_alloc_burst,
    bench_pool_allocation,
    bench_pool_contention,
    bench_raii_drop,
);
criterion_main!(benches);
