//! Multi-GPU scaling benchmarks.
//!
//! Measures partition executor overhead for LayerParallel, TensorParallel,
//! and Pipeline strategies with varying GPU counts, plus transfer overhead.

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use gg_core::engine::gpu::{GpuBackend, GpuDevice};
use gg_core::engine::multi_gpu::GpuPartition;
use gg_core::engine::multi_gpu_exec::{
    LayerParallelExecutor, MockPartitionExecutor, PartitionExecutor, TensorData,
};
use gg_core::engine::multi_gpu_partition::CrossGpuCommunication;
use gg_core::engine::multi_gpu_pipeline::PipelineParallelExecutor;
use gg_core::engine::multi_gpu_tensor::TensorParallelExecutor;

fn make_devices(n: usize) -> Vec<Arc<GpuDevice>> {
    (0..n)
        .map(|i| {
            Arc::new(GpuDevice {
                backend: GpuBackend::Cpu,
                index: i,
                name: format!("mock-gpu-{}", i),
                total_memory: 8 << 30,
                available_memory: 7 << 30,
                compute_capability: None,
            })
        })
        .collect()
}

fn make_partitions(n: usize, layers: usize) -> Vec<GpuPartition> {
    let per = layers / n;
    let extra = layers % n;
    let mut cur = 0;
    (0..n)
        .map(|i| {
            let cnt = per + if i < extra { 1 } else { 0 };
            let p = GpuPartition {
                gpu_index: i,
                layers: cur..cur + cnt,
                memory_budget: 1_000_000,
                parameter_fraction: cnt as f32 / layers as f32,
            };
            cur += cnt;
            p
        })
        .collect()
}

fn bench_mock_executor_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("mock_executor_scaling");
    group.sample_size(20);
    let input = TensorData::new(vec![1.0; 256], vec![16, 16]);

    // Use 1ms base so per-iteration time is measurable but fast
    let exec = MockPartitionExecutor::new(1000);

    for &gpus in &[1, 2, 4, 8] {
        let parts = make_partitions(gpus, 32);
        group.bench_with_input(
            BenchmarkId::new("mock_gpus", gpus),
            &parts,
            |b, p| b.iter(|| exec.execute(p, &input).unwrap()),
        );
    }
    group.finish();
}

fn bench_layer_parallel_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("layer_parallel_scaling");
    group.sample_size(20);
    let input = TensorData::new(vec![1.0; 256], vec![16, 16]);

    for &gpus in &[1, 2, 4, 8] {
        let devices = make_devices(gpus);
        let exec = LayerParallelExecutor::new(devices, 50);
        let parts = make_partitions(gpus, 32);

        group.bench_with_input(
            BenchmarkId::new("layer_gpus", gpus),
            &parts,
            |b, p| b.iter(|| exec.execute(p, &input).unwrap()),
        );
    }
    group.finish();
}

fn bench_tensor_parallel_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("tensor_parallel_scaling");
    group.sample_size(20);

    for &gpus in &[1, 2, 4, 8] {
        let input = TensorData::new(vec![1.0; 256], vec![16, 16]);
        let parts = make_partitions(gpus, 32);
        let exec = TensorParallelExecutor::new(50, true);

        group.bench_with_input(
            BenchmarkId::new("tensor_gpus", gpus),
            &parts,
            |b, p| b.iter(|| exec.execute(p, &input).unwrap()),
        );
    }
    group.finish();
}

fn bench_pipeline_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_scaling");
    group.sample_size(20);
    let input = TensorData::new(vec![1.0; 256], vec![16, 16]);

    for &gpus in &[1, 2, 4] {
        let exec = PipelineParallelExecutor::new(50);
        let parts = make_partitions(gpus, 32);

        group.bench_with_input(
            BenchmarkId::new("pipeline_gpus", gpus),
            &parts,
            |b, p| b.iter(|| exec.execute(p, &input).unwrap()),
        );
    }
    group.finish();
}

fn bench_transfer_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("transfer_overhead");
    let data: Vec<f32> = vec![1.0; 4096];

    group.bench_function("p2p_direct", |b| {
        let comm = CrossGpuCommunication::new(0, 1, true);
        b.iter(|| comm.transfer(&data));
    });

    group.bench_function("host_staged", |b| {
        let comm = CrossGpuCommunication::new(0, 1, false);
        b.iter(|| comm.transfer(&data));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_mock_executor_scaling,
    bench_layer_parallel_scaling,
    bench_tensor_parallel_scaling,
    bench_pipeline_scaling,
    bench_transfer_overhead,
);
criterion_main!(benches);
