// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Partition executor trait and layer-parallel / mock implementations.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::gpu::GpuDevice;
use super::multi_gpu::{GpuPartition, MultiGpuError};

/// Opaque tensor data exchanged between partition stages.
#[derive(Debug, Clone, PartialEq)]
pub struct TensorData {
    /// Flat f32 buffer representing the tensor.
    pub data: Vec<f32>,
    /// Shape dimensions.
    pub shape: Vec<usize>,
}

impl TensorData {
    pub fn new(data: Vec<f32>, shape: Vec<usize>) -> Self {
        Self { data, shape }
    }

    /// Number of elements.
    pub fn numel(&self) -> usize {
        self.data.len()
    }
}

/// Result of executing across partitions.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub output: TensorData,
    pub elapsed: Duration,
    pub gpus_used: usize,
}

/// Trait for executing a model partitioned across GPUs.
pub trait PartitionExecutor: Send + Sync {
    fn execute(
        &self,
        partitions: &[GpuPartition],
        input: &TensorData,
    ) -> Result<ExecutionResult, MultiGpuError>;
}

/// Executes layers sequentially, forwarding activations between GPUs.
pub struct LayerParallelExecutor {
    devices: Vec<Arc<GpuDevice>>,
    per_layer_us: u64,
}

impl LayerParallelExecutor {
    pub fn new(devices: Vec<Arc<GpuDevice>>, per_layer_us: u64) -> Self {
        Self { devices, per_layer_us }
    }
}

impl PartitionExecutor for LayerParallelExecutor {
    fn execute(
        &self,
        partitions: &[GpuPartition],
        input: &TensorData,
    ) -> Result<ExecutionResult, MultiGpuError> {
        let start = Instant::now();
        let mut activations = input.clone();

        for partition in partitions {
            let layer_count = partition.layers.len();
            if layer_count == 0 {
                continue;
            }
            // Simulate forward pass: scale each element by a tiny factor
            let scale = 1.0 + (layer_count as f32 * 1e-6);
            for v in &mut activations.data {
                *v *= scale;
            }
            // Simulate compute latency
            simulate_latency(self.per_layer_us * layer_count as u64);
        }

        Ok(ExecutionResult {
            output: activations,
            elapsed: start.elapsed(),
            gpus_used: partitions.len().min(self.devices.len()),
        })
    }
}

/// Mock executor whose latency decreases with more partitions.
pub struct MockPartitionExecutor {
    base_latency_us: u64,
}

impl MockPartitionExecutor {
    pub fn new(base_latency_us: u64) -> Self {
        Self { base_latency_us }
    }
}

impl PartitionExecutor for MockPartitionExecutor {
    fn execute(
        &self,
        partitions: &[GpuPartition],
        input: &TensorData,
    ) -> Result<ExecutionResult, MultiGpuError> {
        let start = Instant::now();
        let gpu_count = partitions.len().max(1);

        // Simulate speedup: latency inversely proportional to GPU count
        // with 85% parallel efficiency to be realistic
        let effective = self.base_latency_us as f64
            / (gpu_count as f64 * 0.85);
        simulate_latency(effective as u64);

        // Deterministic output: sum layer counts into a scaling factor
        let total_layers: usize = partitions.iter().map(|p| p.layers.len()).sum();
        let scale = 1.0 + (total_layers as f32 * 1e-6);
        let data: Vec<f32> = input.data.iter().map(|v| v * scale).collect();

        Ok(ExecutionResult {
            output: TensorData::new(data, input.shape.clone()),
            elapsed: start.elapsed(),
            gpus_used: gpu_count,
        })
    }
}

fn simulate_latency(microseconds: u64) {
    std::thread::sleep(Duration::from_micros(microseconds));
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn mock_executor_produces_deterministic_output() {
        let exec = MockPartitionExecutor::new(1000);
        let input = TensorData::new(vec![1.0; 16], vec![4, 4]);
        let p2 = make_partitions(2, 32);
        let p4 = make_partitions(4, 32);
        let r2 = exec.execute(&p2, &input).unwrap();
        let r4 = exec.execute(&p4, &input).unwrap();
        assert_eq!(r2.output, r4.output, "same layers -> same output");
    }
}

#[cfg(test)]
#[path = "multi_gpu_exec_tests.rs"]
mod scaling_tests;
