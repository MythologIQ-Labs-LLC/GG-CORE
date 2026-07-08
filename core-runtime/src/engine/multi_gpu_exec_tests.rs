// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Throughput scaling tests for multi-GPU execution (P2.2.3).

use std::sync::Arc;

use crate::engine::gpu::{GpuBackend, GpuDevice};
use crate::engine::multi_gpu::GpuPartition;
use crate::engine::multi_gpu_exec::{
    LayerParallelExecutor, MockPartitionExecutor, PartitionExecutor, TensorData,
};
use crate::engine::multi_gpu_partition::{
    cuda_can_access_peer, CrossGpuCommunication, TransferMethod,
};
use crate::engine::multi_gpu_pipeline::PipelineParallelExecutor;
use crate::engine::multi_gpu_tensor::TensorParallelExecutor;

fn make_gpu(index: usize) -> Arc<GpuDevice> {
    Arc::new(GpuDevice {
        backend: GpuBackend::Cuda,
        index,
        name: format!("MockGPU-{index}"),
        total_memory: 24_000_000_000,
        available_memory: 20_000_000_000,
        compute_capability: Some((8, 6)),
    })
}

fn make_devices(n: usize) -> Vec<Arc<GpuDevice>> {
    (0..n).map(make_gpu).collect()
}

fn partition_layers(n: usize, layers: usize) -> Vec<GpuPartition> {
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

fn input_tensor() -> TensorData {
    TensorData::new(vec![1.0f32; 256], vec![16, 16])
}

// -- P2.2.3: Throughput scaling -------------------------------------------

#[test]
fn mock_2gpu_throughput_above_1_5x() {
    // Use result.elapsed (mock-internal timing) instead of external
    // wall-clock to avoid Windows sleep granularity (~15ms) jitter.
    let base_us: u64 = 50_000;
    let exec = MockPartitionExecutor::new(base_us);
    let input = input_tensor();

    let p1 = partition_layers(1, 32);
    let p2 = partition_layers(2, 32);

    let r1 = exec.execute(&p1, &input).unwrap();
    let r2 = exec.execute(&p2, &input).unwrap();

    let t1 = r1.elapsed.as_secs_f64();
    let t2 = r2.elapsed.as_secs_f64();
    let speedup = t1 / t2;
    assert!(speedup > 1.5, "2-GPU speedup {speedup:.2}x < 1.5x");
}

#[test]
fn mock_4gpu_throughput_above_2_5x() {
    // Use result.elapsed (mock-internal timing) instead of external
    // wall-clock to avoid Windows sleep granularity (~15ms) jitter.
    let base_us: u64 = 50_000;
    let exec = MockPartitionExecutor::new(base_us);
    let input = input_tensor();

    let p1 = partition_layers(1, 48);
    let p4 = partition_layers(4, 48);

    let r1 = exec.execute(&p1, &input).unwrap();
    let r4 = exec.execute(&p4, &input).unwrap();

    let t1 = r1.elapsed.as_secs_f64();
    let t4 = r4.elapsed.as_secs_f64();
    let speedup = t1 / t4;
    assert!(speedup > 2.5, "4-GPU speedup {speedup:.2}x < 2.5x");
}

#[test]
fn all_strategies_identical_output_for_same_input() {
    let input = input_tensor();
    let parts = partition_layers(2, 32);

    let mock = MockPartitionExecutor::new(100);
    let layer = LayerParallelExecutor::new(make_devices(2), 10);
    let pipeline = PipelineParallelExecutor::new(10);

    let r_mock = mock.execute(&parts, &input).unwrap();
    let r_layer = layer.execute(&parts, &input).unwrap();
    let r_pipe = pipeline.execute(&parts, &input).unwrap();

    // All produce output with same length
    assert_eq!(r_mock.output.numel(), input.numel());
    assert_eq!(r_layer.output.numel(), input.numel());
    assert_eq!(r_pipe.output.numel(), input.numel());

    // Mock and layer-parallel use same deterministic scaling
    // (both scale by 1 + total_layers * 1e-6)
    let mock_sum: f32 = r_mock.output.data.iter().sum();
    let layer_sum: f32 = r_layer.output.data.iter().sum();
    assert!(
        (mock_sum - layer_sum).abs() < 0.01,
        "mock={mock_sum} vs layer={layer_sum}"
    );
}

// -- P2.2.2: P2P and host-staging fallback --------------------------------

#[test]
fn p2p_transfer_returns_correct_data() {
    let comm = CrossGpuCommunication::new(0, 1, true);
    let data = vec![1.0, 2.0, 3.0];
    let result = comm.transfer(&data);
    assert_eq!(result.method, TransferMethod::P2pDirect);
    assert_eq!(result.data, data);
    assert_eq!(result.source, 0);
    assert_eq!(result.destination, 1);
}

#[test]
fn host_staged_fallback_preserves_data() {
    let comm = CrossGpuCommunication::new(0, 1, false);
    let data = vec![4.0, 5.0, 6.0, 7.0];
    let result = comm.transfer(&data);
    assert_eq!(result.method, TransferMethod::HostStaged);
    assert_eq!(result.data, data);
}

#[test]
fn tensor_parallel_host_staging_correctness() {
    let input = TensorData::new(vec![1.0; 64], vec![8, 8]);
    let parts: Vec<GpuPartition> = (0..2)
        .map(|i| GpuPartition {
            gpu_index: i,
            layers: 0..usize::MAX,
            memory_budget: 1_000_000,
            parameter_fraction: 0.5,
        })
        .collect();

    // P2P enabled
    let p2p = TensorParallelExecutor::new(10, true);
    let r_p2p = p2p.execute(&parts, &input).unwrap();

    // Host staging fallback
    let staged = TensorParallelExecutor::new(10, false);
    let r_staged = staged.execute(&parts, &input).unwrap();

    // Both must produce identical numerical output
    assert_eq!(r_p2p.output.data, r_staged.output.data);
    assert_eq!(r_p2p.gpus_used, r_staged.gpus_used);
}

#[test]
fn cuda_p2p_check_without_feature() {
    // Without the cuda feature, always returns false
    assert!(!cuda_can_access_peer(0, 1));
}
