# Performance Baselines

> **Historical/conditional:** This early canary/rollout proposal is not an installed GG-CORE v1 benchmark or an automatic deployment capability. GG-CORE is an upstream standalone runtime; CoreForge may separately consume qualified versions. See [the standalone workflow map](../architecture/WORKFLOW_AND_RELEASE_BOUNDARIES.md). for Canary Analysis

**Version:** 0.8.1
**Last Updated:** 2026-02-20
**Status:** Active

## Overview

This document defines performance baselines and thresholds for automated canary
deployment decisions in GG-CORE standalone runtime. These metrics ensure safe
progressive rollouts with automatic rollback on performance degradation.

---

## Current Performance Baselines

### Infrastructure Overhead (Tier 1 Benchmarks)

Based on benchmarks from `benches/*.rs`:

| Component           | Baseline      | Unit          | Source Benchmark        |
|---------------------|---------------|---------------|-------------------------|
| IPC encode          | 104-135       | Melem/s       | ipc_throughput.rs       |
| IPC decode          | 23.6          | Melem/s       | ipc_throughput.rs       |
| Input validation    | 2.9-4.3       | ns            | inference_latency.rs    |
| Memory acquire      | 1.05          | us            | memory_overhead.rs      |
| Result creation     | 85-113        | ns            | generation_throughput.rs|
| Scheduler push      | 2-5           | Melem/s       | scheduler_throughput.rs |
| Queue throughput    | 1000          | req/window    | concurrent_load.rs      |

### Total Infrastructure Overhead

| Operation         | Latency        | % of 100ms Budget |
|-------------------|----------------|-------------------|
| Input validation  | 4.3 ns         | 0.00004%          |
| Memory acquire    | 1,050 ns       | 0.00105%          |
| Result creation   | 113 ns         | 0.00011%          |
| Scheduler ops     | ~200 ns        | 0.00020%          |
| IPC encode/decode | ~18,000 ns     | 0.01800%          |
| **Total**         | **~19,367 ns** | **0.02%**         |

**Conclusion:** Infrastructure overhead is <20 microseconds (<0.02% of target
latency), leaving >99.98% of budget for model inference.

---

## Canary Analysis Thresholds

### Default Thresholds

```rust
AnalysisThresholds {
    max_error_rate_increase: 0.01,   // 1% increase max
    max_latency_p99_increase: 1.2,   // 20% increase max
    min_throughput_ratio: 0.9,       // 10% decrease max
    min_sample_size: 1000,           // requests
    confidence_level: 0.95,          // 95%
    analysis_window: 300s,           // 5 minutes
}
```

### Strict Thresholds (Critical Workloads)

```rust
AnalysisThresholds::strict() {
    max_error_rate_increase: 0.005,  // 0.5% increase max
    max_latency_p99_increase: 1.1,   // 10% increase max
    min_throughput_ratio: 0.95,      // 5% decrease max
    min_sample_size: 2000,           // requests
    confidence_level: 0.99,          // 99%
    analysis_window: 600s,           // 10 minutes
}
```

### Relaxed Thresholds (Non-Critical Workloads)

```rust
AnalysisThresholds::relaxed() {
    max_error_rate_increase: 0.02,   // 2% increase max
    max_latency_p99_increase: 1.5,   // 50% increase max
    min_throughput_ratio: 0.8,       // 20% decrease max
    min_sample_size: 500,            // requests
    confidence_level: 0.90,          // 90%
    analysis_window: 180s,           // 3 minutes
}
```

---

## Alert Thresholds

### Alert Levels

| Level        | Trigger                  | Action Required |
|--------------|--------------------------|-----------------|
| None         | Metric within normal     | No action       |
| Warning      | 50% of threshold limit   | Monitor closely |
| Critical     | 80% of threshold limit   | Investigate     |
| AutoRollback | 100% of threshold limit  | Automatic       |

### Error Rate Alerts

- **Warning:** Error rate increase > 0.5% (50% of 1% limit)
- **Critical:** Error rate increase > 0.8% (80% of 1% limit)
- **AutoRollback:** Error rate increase >= 1.0% (100% of limit)

### P99 Latency Alerts

- **Warning:** P99 ratio > 1.10 (50% of 20% excess)
- **Critical:** P99 ratio > 1.16 (80% of 20% excess)
- **AutoRollback:** P99 ratio >= 1.20 (100% of limit)

### Throughput Alerts

- **Warning:** Throughput ratio < 0.95 (50% of 10% deficit)
- **Critical:** Throughput ratio < 0.92 (80% of 10% deficit)
- **AutoRollback:** Throughput ratio <= 0.90 (100% of limit)

---

## Canary Deployment Configuration

### Default Traffic Weight Progression

```
5% -> 10% -> 25% -> 50% -> 100%
```

### Timing

| Parameter        | Default | Description                    |
|------------------|---------|--------------------------------|
| step_duration    | 5 min   | Minimum time at each weight    |
| analysis_window  | 5 min   | Metrics collection window      |
| max_duration     | 60 min  | Maximum deployment time        |
| max_cycles       | 12      | Max analysis cycles            |

---

## Metrics Collection

### Deployment Metrics Structure

```rust
pub struct DeploymentMetrics {
    error_rate: f64,        // 0.0 - 1.0
    latency_p50: Duration,  // Median latency
    latency_p95: Duration,  // 95th percentile
    latency_p99: Duration,  // 99th percentile
    throughput: f64,        // Requests per second
    saturation: f64,        // Resource utilization 0.0 - 1.0
}
```

### Percentile Calculation

Percentiles are calculated from a rolling window of latency samples:
- Window size: 10,000 samples (configurable)
- Algorithm: Sort and index-based selection
- Update: On each request completion

### Statistical Comparison

Canary vs Stable comparison includes:
- **error_rate_diff:** canary_error - stable_error
- **latency_p99_ratio:** canary_p99 / stable_p99
- **throughput_ratio:** canary_throughput / stable_throughput
- **confidence:** Based on sample size (min_samples / 1000)

---

## Integration Points

### Telemetry Integration

The deployment metrics integrate with existing telemetry:

```rust
// From telemetry/metrics.rs
core_requests_total         // Total requests
core_requests_success       // Successful requests
core_requests_failed        // Failed requests
core_inference_latency_ms   // Latency histogram
```

### Health Check Integration

Canary health is reported via existing health endpoints:

```rust
// From health.rs
HealthStatus::Healthy    -> Canary performing within thresholds
HealthStatus::Degraded   -> Warning level alerts
HealthStatus::Unhealthy  -> Critical or AutoRollback
```

---

## Monitoring Recommendations

### Key Metrics to Monitor

1. **Error Rate Delta:** Track difference between canary and stable
2. **P99 Latency Ratio:** Watch for creeping latency increases
3. **Throughput Trend:** Ensure no capacity degradation
4. **Sample Count:** Verify sufficient data for decisions

### Dashboard Queries

```promql
# Error rate comparison
sum(rate(core_requests_failed{version="canary"}[5m]))
  / sum(rate(core_requests_total{version="canary"}[5m]))
-
sum(rate(core_requests_failed{version="stable"}[5m]))
  / sum(rate(core_requests_total{version="stable"}[5m]))

# P99 latency comparison
histogram_quantile(0.99, rate(core_inference_latency_ms{version="canary"}[5m]))
  / histogram_quantile(0.99, rate(core_inference_latency_ms{version="stable"}[5m]))
```

---

## References

- `core-runtime/src/deployment/metrics.rs` - Metrics collection
- `core-runtime/src/deployment/thresholds.rs` - Threshold configuration
- `core-runtime/src/deployment/canary.rs` - Canary controller
- `docs/analysis/BASELINE_METRICS.md` - Tier 1 benchmark results
- `core-runtime/benches/*.rs` - Performance benchmarks

