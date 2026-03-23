// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Tensor-parallel executor: shards tensors across GPUs with all-reduce.

use std::time::{Duration, Instant};

use super::multi_gpu::{GpuPartition, MultiGpuError};
use super::multi_gpu_exec::{ExecutionResult, PartitionExecutor, TensorData};
use super::multi_gpu_partition::CrossGpuCommunication;

/// Shards each tensor row across GPUs, runs local matmul, then all-reduces.
pub struct TensorParallelExecutor {
    /// Per-shard compute latency in microseconds.
    shard_compute_us: u64,
    /// Whether P2P transfers are available.
    p2p_enabled: bool,
}

impl TensorParallelExecutor {
    pub fn new(shard_compute_us: u64, p2p_enabled: bool) -> Self {
        Self { shard_compute_us, p2p_enabled }
    }

    /// Simulate an all-reduce across `n` GPUs, returning aggregated data.
    fn all_reduce(
        &self,
        shards: &[Vec<f32>],
    ) -> Result<Vec<f32>, MultiGpuError> {
        if shards.is_empty() {
            return Ok(Vec::new());
        }
        let len = shards[0].len();
        let mut result = vec![0.0f32; len];
        for shard in shards {
            for (r, &s) in result.iter_mut().zip(shard.iter()) {
                *r += s;
            }
        }
        Ok(result)
    }

    /// Simulate a cross-GPU transfer (P2P or host-staged).
    fn simulate_transfer(&self, src: usize, dst: usize) {
        let comm = CrossGpuCommunication::new(src, dst, self.p2p_enabled);
        let latency_us: u64 = if comm.can_direct_transfer() { 5 } else { 20 };
        std::thread::sleep(Duration::from_micros(latency_us));
    }
}

impl PartitionExecutor for TensorParallelExecutor {
    fn execute(
        &self,
        partitions: &[GpuPartition],
        input: &TensorData,
    ) -> Result<ExecutionResult, MultiGpuError> {
        let start = Instant::now();
        let gpu_count = partitions.len().max(1);
        let numel = input.numel();
        let shard_size = numel / gpu_count;

        // Phase 1: shard input and compute locally on each GPU
        let mut shards: Vec<Vec<f32>> = Vec::with_capacity(gpu_count);
        for (i, partition) in partitions.iter().enumerate() {
            let begin = i * shard_size;
            let end = if i == gpu_count - 1 { numel } else { begin + shard_size };
            let shard: Vec<f32> = input.data[begin..end]
                .iter()
                .map(|&v| v * partition.parameter_fraction)
                .collect();
            shards.push(shard);
            simulate_compute(self.shard_compute_us);
        }

        // Phase 2: all-reduce (simulate transfers between GPUs)
        for i in 1..gpu_count {
            self.simulate_transfer(partitions[i].gpu_index, partitions[0].gpu_index);
        }
        let reduced = self.all_reduce(&shards)?;

        // Pad or truncate to match original length
        let mut data = vec![0.0f32; numel];
        let copy_len = reduced.len().min(numel);
        data[..copy_len].copy_from_slice(&reduced[..copy_len]);

        Ok(ExecutionResult {
            output: TensorData::new(data, input.shape.clone()),
            elapsed: start.elapsed(),
            gpus_used: gpu_count,
        })
    }
}

fn simulate_compute(microseconds: u64) {
    std::thread::sleep(Duration::from_micros(microseconds));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::multi_gpu::GpuPartition;

    fn make_tensor_partitions(n: usize) -> Vec<GpuPartition> {
        (0..n)
            .map(|i| GpuPartition {
                gpu_index: i,
                layers: 0..usize::MAX,
                memory_budget: 1_000_000,
                parameter_fraction: 1.0 / n as f32,
            })
            .collect()
    }

    #[test]
    fn tensor_parallel_p2p_vs_host_staging() {
        let input = TensorData::new(vec![1.0; 64], vec![8, 8]);
        let parts = make_tensor_partitions(2);

        let p2p = TensorParallelExecutor::new(10, true);
        let r1 = p2p.execute(&parts, &input).unwrap();

        let staged = TensorParallelExecutor::new(10, false);
        let r2 = staged.execute(&parts, &input).unwrap();

        // Both must produce valid output
        assert_eq!(r1.output.numel(), 64);
        assert_eq!(r2.output.numel(), 64);
        assert_eq!(r1.gpus_used, 2);
    }
}
