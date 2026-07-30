//! Security-pipeline overhead benchmarks.
//!
//! Quantifies the per-call cost that `SecurityPipeline` adds to every
//! `Runtime::infer` (B-33 made the pipeline mandatory): ingress
//! `scan_prompt` (prompt-injection regex scan) and egress `sanitize_output`
//! (PII-redaction regex scan). The pipeline is constructed from
//! `SecurityConfig::default()` — both stages enabled in blocking mode, the
//! default product configuration — with no model, so this joins the CI-safe
//! bench set. The `sanitize_output` size curve is the load-bearing output:
//! it decides whether B-36 (incremental streaming sanitize) is warranted.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gg_core::security::{SecurityConfig, SecurityPipeline};

/// The default product pipeline: injection + PII stages both enabled, blocking.
fn blocking_pipeline() -> SecurityPipeline {
    SecurityPipeline::from_config(&SecurityConfig::default())
}

/// A clean prompt/output of `length` bytes (no injection or PII tokens), so the
/// measurement isolates the scan cost of the common admit/pass-through path.
fn clean_text(length: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .chars()
        .cycle()
        .take(length)
        .collect()
}

/// A ~`length`-byte output densely seeded with PII-shaped tokens (email +
/// phone), so the measurement captures the redaction-active cost rather than
/// the clean pass-through.
fn pii_heavy_text(length: usize) -> String {
    "contact john.doe@example.com or 555-123-4567. "
        .chars()
        .cycle()
        .take(length)
        .collect()
}

fn bench_scan_prompt(c: &mut Criterion) {
    let pipeline = blocking_pipeline();
    let mut group = c.benchmark_group("security_scan_prompt");

    for (name, length) in [
        ("256_chars", 256),
        ("2048_chars", 2048),
        ("16384_chars", 16384),
    ] {
        let prompt = clean_text(length);
        group.throughput(Throughput::Bytes(length as u64));
        group.bench_with_input(BenchmarkId::new("clean", name), &prompt, |b, p| {
            b.iter(|| pipeline.scan_prompt(p))
        });
    }

    group.finish();
}

fn bench_sanitize_output(c: &mut Criterion) {
    let pipeline = blocking_pipeline();
    let mut group = c.benchmark_group("security_sanitize_output");

    for (name, length) in [
        ("256_chars", 256),
        ("2048_chars", 2048),
        ("16384_chars", 16384),
    ] {
        let output = clean_text(length);
        group.throughput(Throughput::Bytes(length as u64));
        group.bench_with_input(BenchmarkId::new("clean", name), &output, |b, o| {
            b.iter(|| pipeline.sanitize_output(o))
        });
    }

    // Redaction-active case: measures the cost when PII is actually rewritten.
    let pii = pii_heavy_text(2048);
    group.throughput(Throughput::Bytes(pii.len() as u64));
    group.bench_with_input(BenchmarkId::new("pii_heavy", "2048_chars"), &pii, |b, o| {
        b.iter(|| pipeline.sanitize_output(o))
    });

    group.finish();
}

criterion_group!(benches, bench_scan_prompt, bench_sanitize_output);
criterion_main!(benches);
