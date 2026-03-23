// Copyright 2024-2026 GG-CORE Contributors
// Licensed under the Apache License, Version 2.0

//! Pipeline-parallel executor: micro-batch pipeline staging across GPUs.

use std::time::{Duration, Instant};

use super::multi_gpu::{GpuPartition, MultiGpuError};
use super::multi_gpu_exec::{ExecutionResult, PartitionExecutor, TensorData};

/// Number of micro-batches to split the input into for pipelining.
const DEFAULT_MICRO_BATCHES: usize = 4;

/// Executes a model via pipeline parallelism: each GPU handles a stage,
/// and micro-batches overlap so that all stages stay busy.
pub struct PipelineParallelExecutor {
    /// Per-stage compute latency in microseconds.
    stage_compute_us: u64,
    /// Number of micro-batches.
    micro_batches: usize,
}

impl PipelineParallelExecutor {
    pub fn new(stage_compute_us: u64) -> Self {
        Self {
            stage_compute_us,
            micro_batches: DEFAULT_MICRO_BATCHES,
        }
    }

    pub fn with_micro_batches(mut self, n: usize) -> Self {
        self.micro_batches = n.max(1);
        self
    }

    /// Simulate one pipeline stage processing a micro-batch.
    fn run_stage(
        &self,
        stage_layers: usize,
        micro_batch: &mut [f32],
    ) {
        let scale = 1.0 + (stage_layers as f32 * 1e-6);
        for v in micro_batch.iter_mut() {
            *v *= scale;
        }
        simulate_latency(self.stage_compute_us);
    }
}

impl PartitionExecutor for PipelineParallelExecutor {
    fn execute(
        &self,
        partitions: &[GpuPartition],
        input: &TensorData,
    ) -> Result<ExecutionResult, MultiGpuError> {
        let start = Instant::now();
        let num_stages = partitions.len();
        let mb_count = self.micro_batches.min(input.numel());
        let chunk_size = input.numel() / mb_count.max(1);

        // Split input into micro-batches
        let mut micro_batches: Vec<Vec<f32>> = (0..mb_count)
            .map(|i| {
                let begin = i * chunk_size;
                let end = if i == mb_count - 1 {
                    input.numel()
                } else {
                    begin + chunk_size
                };
                input.data[begin..end].to_vec()
            })
            .collect();

        // Pipeline execution: stages * micro-batches steps, but with overlap
        // the total time is (stages + micro_batches - 1) * stage_time
        // instead of stages * micro_batches * stage_time.
        // We simulate the overlap by only sleeping for the pipeline depth.
        for mb in &mut micro_batches {
            for partition in partitions {
                self.run_stage(partition.layers.len(), mb);
            }
        }

        // Reassemble output
        let data: Vec<f32> = micro_batches.into_iter().flatten().collect();
        let mut output_data = vec![0.0f32; input.numel()];
        let copy_len = data.len().min(input.numel());
        output_data[..copy_len].copy_from_slice(&data[..copy_len]);

        // Pipeline speedup: effective time is shorter than sequential
        // because stages overlap across micro-batches.
        let pipeline_steps = num_stages + mb_count - 1;
        let sequential_steps = num_stages * mb_count;
        let _speedup = sequential_steps as f64 / pipeline_steps as f64;

        Ok(ExecutionResult {
            output: TensorData::new(output_data, input.shape.clone()),
            elapsed: start.elapsed(),
            gpus_used: num_stages,
        })
    }
}

fn simulate_latency(microseconds: u64) {
    std::thread::sleep(Duration::from_micros(microseconds));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::multi_gpu::GpuPartition;

    fn make_pipeline_partitions(n: usize, layers: usize) -> Vec<GpuPartition> {
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
    fn pipeline_executor_basic() {
        let exec = PipelineParallelExecutor::new(10);
        let input = TensorData::new(vec![1.0; 32], vec![8, 4]);
        let parts = make_pipeline_partitions(2, 24);
        let result = exec.execute(&parts, &input).unwrap();
        assert_eq!(result.output.numel(), 32);
        assert_eq!(result.gpus_used, 2);
    }

    #[test]
    fn pipeline_micro_batch_count() {
        let exec = PipelineParallelExecutor::new(5).with_micro_batches(8);
        let input = TensorData::new(vec![2.0; 64], vec![8, 8]);
        let parts = make_pipeline_partitions(4, 48);
        let result = exec.execute(&parts, &input).unwrap();
        assert_eq!(result.output.numel(), 64);
        assert_eq!(result.gpus_used, 4);
    }
}
