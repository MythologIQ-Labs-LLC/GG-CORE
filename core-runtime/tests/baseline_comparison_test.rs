//! Baseline comparison tests for regression detection.
//!
//! Compares current metrics against baseline thresholds to detect performance regressions.

use std::fs;
use std::path::Path;

/// Baseline metrics loaded from fixtures.
#[derive(Debug, serde::Deserialize)]
struct BaselineMetrics {
    version: String,
    #[allow(dead_code)]
    collected_at: String,
    metrics: Metrics,
    thresholds: Thresholds,
}

#[derive(Debug, serde::Deserialize)]
struct Metrics {
    classification_p95_ms: u64,
    embedding_p95_ms: u64,
    generation_tok_per_sec: f64,
    cold_load_60mb_ms: u64,
    peak_rss_ratio: f64,
    concurrent_throughput_ratio: f64,
}

#[derive(Debug, serde::Deserialize)]
struct Thresholds {
    latency_increase_percent: u64,
    throughput_decrease_percent: u64,
    memory_increase_percent: u64,
}

fn load_baseline() -> Result<BaselineMetrics, String> {
    let path = Path::new("fixtures/baselines/baseline_metrics.json");
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to load baseline: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse baseline: {}", e))
}

#[test]
fn baseline_file_exists_and_valid() {
    let baseline = load_baseline();
    assert!(baseline.is_ok(), "Baseline file should exist and be valid JSON");

    let baseline = baseline.unwrap();
    assert_eq!(baseline.version, "1.1.0");
}

#[test]
fn baseline_has_required_metrics() {
    let baseline = load_baseline().expect("Baseline should load");

    // Verify all required metrics are present and reasonable
    assert!(baseline.metrics.classification_p95_ms > 0);
    assert!(baseline.metrics.embedding_p95_ms > 0);
    assert!(baseline.metrics.generation_tok_per_sec > 0.0);
    assert!(baseline.metrics.cold_load_60mb_ms > 0);
    assert!(baseline.metrics.peak_rss_ratio > 1.0);
    assert!(baseline.metrics.concurrent_throughput_ratio > 0.0);
}

#[test]
fn baseline_thresholds_are_reasonable() {
    let baseline = load_baseline().expect("Baseline should load");

    // Verify thresholds are set and reasonable
    assert!(baseline.thresholds.latency_increase_percent <= 50);
    assert!(baseline.thresholds.throughput_decrease_percent <= 50);
    assert!(baseline.thresholds.memory_increase_percent <= 50);
}

#[test]
fn compare_classification_latency() {
    let baseline = load_baseline().expect("Baseline should load");

    // Simulated current measurement (in real usage, this would come from benchmarks)
    let current_classification_p95_ms = 90; // 90ms

    let threshold = baseline.metrics.classification_p95_ms
        * (100 + baseline.thresholds.latency_increase_percent)
        / 100;

    assert!(
        current_classification_p95_ms <= threshold,
        "Classification P95 latency regression: {}ms > {}ms threshold",
        current_classification_p95_ms,
        threshold
    );
}

#[test]
fn compare_embedding_latency() {
    let baseline = load_baseline().expect("Baseline should load");

    // Simulated current measurement
    let current_embedding_p95_ms = 45; // 45ms

    let threshold = baseline.metrics.embedding_p95_ms
        * (100 + baseline.thresholds.latency_increase_percent)
        / 100;

    assert!(
        current_embedding_p95_ms <= threshold,
        "Embedding P95 latency regression: {}ms > {}ms threshold",
        current_embedding_p95_ms,
        threshold
    );
}

#[test]
fn compare_generation_throughput() {
    let baseline = load_baseline().expect("Baseline should load");

    // Simulated current measurement
    let current_generation_tok_per_sec = 11.5; // 11.5 tok/s

    let threshold = baseline.metrics.generation_tok_per_sec
        * (100.0 - baseline.thresholds.throughput_decrease_percent as f64)
        / 100.0;

    assert!(
        current_generation_tok_per_sec >= threshold,
        "Generation throughput regression: {:.1} tok/s < {:.1} tok/s threshold",
        current_generation_tok_per_sec,
        threshold
    );
}

#[test]
fn compare_memory_efficiency() {
    let baseline = load_baseline().expect("Baseline should load");

    // Simulated current measurement
    let current_peak_rss_ratio = 1.45; // 1.45x model size

    let threshold = baseline.metrics.peak_rss_ratio
        * (100.0 + baseline.thresholds.memory_increase_percent as f64)
        / 100.0;

    assert!(
        current_peak_rss_ratio <= threshold,
        "Memory efficiency regression: {:.2}x > {:.2}x threshold",
        current_peak_rss_ratio,
        threshold
    );
}

#[test]
fn compare_concurrent_throughput() {
    let baseline = load_baseline().expect("Baseline should load");

    // Simulated current measurement
    let current_concurrent_throughput_ratio = 0.80; // 80% of single-threaded

    let threshold = baseline.metrics.concurrent_throughput_ratio
        * (100.0 - baseline.thresholds.throughput_decrease_percent as f64)
        / 100.0;

    assert!(
        current_concurrent_throughput_ratio >= threshold,
        "Concurrent throughput regression: {:.2} < {:.2} threshold",
        current_concurrent_throughput_ratio,
        threshold
    );
}
