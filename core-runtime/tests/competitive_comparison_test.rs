//! Competitive comparison tests against external runtime benchmarks.
//!
//! Validates CORE Runtime targets against industry benchmarks from:
//! - llama.cpp (CPU generation)
//! - Ollama (CPU generation)
//! - ONNX Runtime (classification/embedding)

use std::fs;
use std::path::Path;

/// External reference metrics from industry benchmarks.
#[derive(Debug, serde::Deserialize)]
struct BaselineWithReferences {
    #[allow(dead_code)]
    version: String,
    metrics: Metrics,
    external_references: ExternalReferences,
    tier_targets: TierTargets,
}

#[derive(Debug, serde::Deserialize)]
struct Metrics {
    classification_p95_ms: u64,
    #[allow(dead_code)]
    embedding_p95_ms: u64,
    generation_tok_per_sec: f64,
    peak_rss_ratio: f64,
}

#[derive(Debug, serde::Deserialize)]
struct ExternalReferences {
    generation: GenerationRefs,
    classification: ClassificationRefs,
}

#[derive(Debug, serde::Deserialize)]
struct GenerationRefs {
    ollama_cpu_16core: RuntimeRef,
    #[allow(dead_code)]
    llama_cpp_m2_ultra: RuntimeRef,
    #[allow(dead_code)]
    vllm_cpu_16core: RuntimeRef,
}

#[derive(Debug, serde::Deserialize)]
struct ClassificationRefs {
    onnx_runtime_arm: ClassificationRef,
}

#[derive(Debug, serde::Deserialize)]
struct RuntimeRef {
    tok_per_sec: f64,
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    note: String,
}

#[derive(Debug, serde::Deserialize)]
struct ClassificationRef {
    latency_ms: f64,
    #[allow(dead_code)]
    throughput_inf_per_sec: f64,
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    note: String,
}

#[derive(Debug, serde::Deserialize)]
struct TierTargets {
    tier_1_minimum: TierTarget,
    tier_2_competitive: TierTarget,
    tier_3_optimized: TierTarget,
}

#[derive(Debug, serde::Deserialize)]
struct TierTarget {
    generation_tok_per_sec: f64,
    classification_p95_ms: u64,
    memory_ratio: f64,
    #[allow(dead_code)]
    description: String,
}

fn load_baseline() -> Result<BaselineWithReferences, String> {
    let path = Path::new("fixtures/baselines/baseline_metrics.json");
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to load baseline: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse baseline: {}", e))
}

#[test]
fn external_references_loaded() {
    let baseline = load_baseline().expect("Should load baseline");

    // Verify external references are present
    assert!(baseline.external_references.generation.ollama_cpu_16core.tok_per_sec > 0.0);
    assert!(baseline.external_references.generation.llama_cpp_m2_ultra.tok_per_sec > 0.0);
    assert!(baseline.external_references.classification.onnx_runtime_arm.latency_ms > 0.0);
}

#[test]
fn tier_targets_defined() {
    let baseline = load_baseline().expect("Should load baseline");

    // Tier 1 < Tier 2 < Tier 3 for throughput
    assert!(baseline.tier_targets.tier_1_minimum.generation_tok_per_sec
        < baseline.tier_targets.tier_2_competitive.generation_tok_per_sec);
    assert!(baseline.tier_targets.tier_2_competitive.generation_tok_per_sec
        < baseline.tier_targets.tier_3_optimized.generation_tok_per_sec);

    // Tier 1 > Tier 2 > Tier 3 for latency (lower is better)
    assert!(baseline.tier_targets.tier_1_minimum.classification_p95_ms
        > baseline.tier_targets.tier_2_competitive.classification_p95_ms);
    assert!(baseline.tier_targets.tier_2_competitive.classification_p95_ms
        > baseline.tier_targets.tier_3_optimized.classification_p95_ms);
}

#[test]
fn core_targets_meet_tier_1() {
    let baseline = load_baseline().expect("Should load baseline");

    // Our baseline metrics should meet Tier 1 minimum
    assert!(
        baseline.metrics.generation_tok_per_sec >= baseline.tier_targets.tier_1_minimum.generation_tok_per_sec,
        "Generation throughput {:.1} tok/s should meet Tier 1 minimum {:.1} tok/s",
        baseline.metrics.generation_tok_per_sec,
        baseline.tier_targets.tier_1_minimum.generation_tok_per_sec
    );

    assert!(
        baseline.metrics.classification_p95_ms <= baseline.tier_targets.tier_1_minimum.classification_p95_ms,
        "Classification P95 {}ms should meet Tier 1 maximum {}ms",
        baseline.metrics.classification_p95_ms,
        baseline.tier_targets.tier_1_minimum.classification_p95_ms
    );

    assert!(
        baseline.metrics.peak_rss_ratio <= baseline.tier_targets.tier_1_minimum.memory_ratio,
        "Memory ratio {:.2}x should meet Tier 1 maximum {:.2}x",
        baseline.metrics.peak_rss_ratio,
        baseline.tier_targets.tier_1_minimum.memory_ratio
    );
}

#[test]
fn generation_competitive_ratio() {
    let baseline = load_baseline().expect("Should load baseline");

    let ollama_cpu = baseline.external_references.generation.ollama_cpu_16core.tok_per_sec;
    let our_target = baseline.metrics.generation_tok_per_sec;
    let ratio = our_target / ollama_cpu;

    // We should achieve at least 10% of CPU Ollama performance
    // (accounting for sandboxing overhead)
    assert!(
        ratio >= 0.10,
        "CORE should achieve at least 10% of Ollama CPU ({:.1}%)",
        ratio * 100.0
    );

    // Document the gap for analysis
    println!("CORE vs Ollama CPU: {:.1}% ({:.1} vs {:.1} tok/s)",
        ratio * 100.0, our_target, ollama_cpu);
}

#[test]
fn classification_competitive_ratio() {
    let baseline = load_baseline().expect("Should load baseline");

    let onnx_bare = baseline.external_references.classification.onnx_runtime_arm.latency_ms;
    let our_target = baseline.metrics.classification_p95_ms as f64;
    let overhead_factor = our_target / onnx_bare;

    // Sandboxed execution should be within 100x of bare ONNX Runtime
    // (security overhead is expected but should be bounded)
    assert!(
        overhead_factor <= 100.0,
        "CORE classification overhead should be <100x bare ONNX ({:.1}x)",
        overhead_factor
    );

    // Document the gap for analysis
    println!("CORE vs bare ONNX: {:.1}x overhead ({:.1}ms vs {:.2}ms)",
        overhead_factor, our_target, onnx_bare);
}

#[test]
fn memory_efficiency_competitive() {
    let baseline = load_baseline().expect("Should load baseline");

    // Memory efficiency should be within industry norms (1.3-1.5x)
    assert!(
        baseline.metrics.peak_rss_ratio <= 1.5,
        "Peak RSS ratio {:.2}x should be <= 1.5x (industry standard)",
        baseline.metrics.peak_rss_ratio
    );

    assert!(
        baseline.metrics.peak_rss_ratio >= 1.0,
        "Peak RSS ratio {:.2}x must be >= 1.0x (at least model size)",
        baseline.metrics.peak_rss_ratio
    );
}

#[test]
fn tier_progression_reasonable() {
    let baseline = load_baseline().expect("Should load baseline");

    // Tier 2 should be 2-3x better than Tier 1 for throughput
    let gen_improvement = baseline.tier_targets.tier_2_competitive.generation_tok_per_sec
        / baseline.tier_targets.tier_1_minimum.generation_tok_per_sec;
    assert!(gen_improvement >= 2.0 && gen_improvement <= 5.0,
        "Tier 2 generation should be 2-5x Tier 1 (got {:.1}x)", gen_improvement);

    // Tier 3 should approach but not exceed external benchmarks
    let ollama_cpu = baseline.external_references.generation.ollama_cpu_16core.tok_per_sec;
    assert!(
        baseline.tier_targets.tier_3_optimized.generation_tok_per_sec <= ollama_cpu,
        "Tier 3 target should not exceed unsandboxed Ollama"
    );
}
