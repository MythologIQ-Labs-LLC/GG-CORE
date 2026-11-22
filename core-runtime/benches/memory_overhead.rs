//! Memory overhead benchmarks.
//!
//! Measures memory pool and resource limit operations.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use gg_core::memory::{MemoryPool, MemoryPoolConfig, ResourceLimits, ResourceLimitsConfig};

fn bench_memory_pool_acquire(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pool_acquire");

    let config = MemoryPoolConfig::default();
    let pool = MemoryPool::new(config);

    // Benchmark synchronous buffer acquisition (no async overhead)
    group.throughput(Throughput::Elements(1));
    group.bench_function("acquire", |b| {
        b.iter(|| {
            let buffer = pool.acquire();
            black_box(buffer.len())
        })
    });

    group.finish();
}

fn bench_resource_limits_acquire(c: &mut Criterion) {
    let mut group = c.benchmark_group("resource_limits_acquire");

    for (name, memory_size) in [
        ("1kb", 1024usize),
        ("1mb", 1024 * 1024),
        ("10mb", 10 * 1024 * 1024),
    ] {
        let config = ResourceLimitsConfig {
            max_memory_per_call: 100 * 1024 * 1024,
            max_total_memory: 1024 * 1024 * 1024,
            max_concurrent: 100,
        };
        let limits = ResourceLimits::new(config);

        group.throughput(Throughput::Bytes(memory_size as u64));
        group.bench_function(BenchmarkId::new("acquire_release", name), |b| {
            b.iter(|| {
                let guard = limits.try_acquire(black_box(memory_size)).unwrap();
                drop(black_box(guard))
            })
        });
    }

    group.finish();
}

fn bench_resource_limits_tracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("resource_limits_tracking");

    let config = ResourceLimitsConfig {
        max_memory_per_call: 100 * 1024 * 1024,
        max_total_memory: 10 * 1024 * 1024 * 1024,
        max_concurrent: 1000,
    };
    let limits = ResourceLimits::new(config);

    // Pre-acquire some resources to test tracking under load
    let _guards: Vec<_> = (0..10)
        .map(|_| limits.try_acquire(1024 * 1024).unwrap())
        .collect();

    group.bench_function("current_memory", |b| {
        b.iter(|| black_box(limits.current_memory()))
    });

    group.bench_function("current_concurrent", |b| {
        b.iter(|| black_box(limits.current_concurrent()))
    });

    group.finish();
}

fn bench_concurrent_acquire_release(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_resource_ops");

    for count in [5usize, 10, 20] {
        let config = ResourceLimitsConfig {
            max_memory_per_call: 10 * 1024 * 1024,
            max_total_memory: 100 * 1024 * 1024,
            max_concurrent: count + 5,
        };
        let limits = ResourceLimits::new(config);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new("sequential", count), |b| {
            b.iter(|| {
                let guards: Vec<_> = (0..count)
                    .map(|_| limits.try_acquire(black_box(1024)).unwrap())
                    .collect();
                drop(black_box(guards))
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_memory_pool_acquire,
    bench_resource_limits_acquire,
    bench_resource_limits_tracking,
    bench_concurrent_acquire_release
);
criterion_main!(benches);
