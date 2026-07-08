// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//\! Metrics collection and statistical analysis for canary deployments.
//\!
//\! Collects performance metrics from both canary and stable versions,
//\! performs statistical comparisons, and detects anomalies.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Core deployment metrics for comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentMetrics {
    /// Error rate as a fraction (0.0 - 1.0).
    pub error_rate: f64,
    /// 50th percentile latency.
    pub latency_p50: Duration,
    /// 95th percentile latency.
    pub latency_p95: Duration,
    /// 99th percentile latency.
    pub latency_p99: Duration,
    /// Requests per second throughput.
    pub throughput: f64,
    /// Resource saturation (0.0 - 1.0).
    pub saturation: f64,
}

impl Default for DeploymentMetrics {
    fn default() -> Self {
        Self {
            error_rate: 0.0,
            latency_p50: Duration::ZERO,
            latency_p95: Duration::ZERO,
            latency_p99: Duration::ZERO,
            throughput: 0.0,
            saturation: 0.0,
        }
    }
}

/// A snapshot of metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// When the snapshot was taken.
    pub timestamp: Instant,
    /// Number of samples in this snapshot.
    pub sample_count: usize,
    /// Aggregated metrics.
    pub metrics: DeploymentMetrics,
}

impl MetricsSnapshot {
    /// Create a new metrics snapshot.
    pub fn new(sample_count: usize, metrics: DeploymentMetrics) -> Self {
        Self {
            timestamp: Instant::now(),
            sample_count,
            metrics,
        }
    }

    /// Check if snapshot has minimum required samples.
    pub fn has_sufficient_samples(&self, min_samples: usize) -> bool {
        self.sample_count >= min_samples
    }
}

/// Result of statistical comparison between canary and stable.
#[derive(Debug, Clone)]
pub struct StatisticalComparison {
    /// Error rate difference (canary - stable).
    pub error_rate_diff: f64,
    /// P99 latency ratio (canary / stable).
    pub latency_p99_ratio: f64,
    /// Throughput ratio (canary / stable).
    pub throughput_ratio: f64,
    /// Statistical confidence (0.0 - 1.0).
    pub confidence: f64,
    /// Whether the comparison is statistically significant.
    pub is_significant: bool,
}

impl StatisticalComparison {
    /// Check if canary performs worse than thresholds allow.
    pub fn is_degraded(&self, max_error_increase: f64, max_latency_ratio: f64) -> bool {
        self.error_rate_diff > max_error_increase || self.latency_p99_ratio > max_latency_ratio
    }
}

/// Collector for deployment metrics with rolling windows.
pub struct MetricsCollector {
    latencies: VecDeque<u64>,
    success_count: u64,
    error_count: u64,
    window_start: Instant,
    max_samples: usize,
}

impl MetricsCollector {
    /// Create a new metrics collector with specified window size.
    pub fn new(max_samples: usize) -> Self {
        Self {
            latencies: VecDeque::with_capacity(max_samples),
            success_count: 0,
            error_count: 0,
            window_start: Instant::now(),
            max_samples,
        }
    }

    /// Record a successful request with its latency.
    pub fn record_success(&mut self, latency: Duration) {
        self.success_count += 1;
        self.push_latency(latency);
    }

    /// Record a failed request.
    pub fn record_error(&mut self, latency: Duration) {
        self.error_count += 1;
        self.push_latency(latency);
    }

    fn push_latency(&mut self, latency: Duration) {
        if self.latencies.len() >= self.max_samples {
            self.latencies.pop_front();
        }
        self.latencies.push_back(latency.as_micros() as u64);
    }

    /// Get current sample count.
    pub fn sample_count(&self) -> usize {
        self.latencies.len()
    }

    /// Take a snapshot of current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let metrics = self.compute_metrics();
        MetricsSnapshot::new(self.sample_count(), metrics)
    }

    fn compute_metrics(&self) -> DeploymentMetrics {
        let total = self.success_count + self.error_count;
        let error_rate = if total > 0 {
            self.error_count as f64 / total as f64
        } else {
            0.0
        };

        let (p50, p95, p99) = self.compute_percentiles();
        let elapsed = self.window_start.elapsed().as_secs_f64().max(0.001);
        let throughput = total as f64 / elapsed;

        DeploymentMetrics {
            error_rate,
            latency_p50: Duration::from_micros(p50),
            latency_p95: Duration::from_micros(p95),
            latency_p99: Duration::from_micros(p99),
            throughput,
            saturation: 0.0,
        }
    }

    fn compute_percentiles(&self) -> (u64, u64, u64) {
        if self.latencies.is_empty() {
            return (0, 0, 0);
        }

        let mut sorted: Vec<u64> = self.latencies.iter().copied().collect();
        sorted.sort_unstable();

        let p50 = percentile(&sorted, 0.50);
        let p95 = percentile(&sorted, 0.95);
        let p99 = percentile(&sorted, 0.99);

        (p50, p95, p99)
    }

    /// Reset the collector for a new window.
    pub fn reset(&mut self) {
        self.latencies.clear();
        self.success_count = 0;
        self.error_count = 0;
        self.window_start = Instant::now();
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Compare two metric snapshots statistically.
pub fn compare_metrics(
    canary: &MetricsSnapshot,
    stable: &MetricsSnapshot,
    confidence_level: f64,
) -> StatisticalComparison {
    let error_rate_diff = canary.metrics.error_rate - stable.metrics.error_rate;

    let latency_p99_ratio = if stable.metrics.latency_p99.as_micros() > 0 {
        canary.metrics.latency_p99.as_micros() as f64
            / stable.metrics.latency_p99.as_micros() as f64
    } else {
        1.0
    };

    let throughput_ratio = if stable.metrics.throughput > 0.0 {
        canary.metrics.throughput / stable.metrics.throughput
    } else {
        1.0
    };

    let min_samples = canary.sample_count.min(stable.sample_count);
    let confidence = (min_samples as f64 / 1000.0).min(1.0);
    let is_significant = confidence >= confidence_level;

    StatisticalComparison {
        error_rate_diff,
        latency_p99_ratio,
        throughput_ratio,
        confidence,
        is_significant,
    }
}
