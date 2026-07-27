//! Generation throughput benchmarks.
//!
//! Measures token generation infrastructure and output processing.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gg_core::engine::{FinishReason, GenerationResult, StreamItem, StreamTerminal};

fn create_generation_result(token_count: usize) -> GenerationResult {
    let text = "generated ".repeat(token_count);

    GenerationResult {
        text,
        tokens_generated: token_count as u32,
        finish_reason: FinishReason::MaxTokens,
    }
}

fn bench_result_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("generation_result_creation");

    for (name, count) in [("50_tokens", 50), ("200_tokens", 200), ("500_tokens", 500)] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new("tokens", name), |b| {
            b.iter(|| create_generation_result(black_box(count)))
        });
    }

    group.finish();
}

fn bench_streaming_output_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_output_creation");

    for (name, count) in [("single", 1), ("batch_10", 10), ("batch_50", 50)] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new("tokens", name), |b| {
            b.iter(|| {
                for i in 0..count {
                    let frame = if i == count - 1 {
                        StreamItem::End(StreamTerminal::Complete)
                    } else {
                        StreamItem::Token((i % 50000) as u32)
                    };
                    let _ = black_box(frame);
                }
            })
        });
    }

    group.finish();
}

fn bench_finish_reason_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("finish_reason_matching");

    let reasons = vec![
        FinishReason::Stop,
        FinishReason::MaxTokens,
        FinishReason::Timeout,
        FinishReason::ContentFiltered,
        FinishReason::Cancelled,
    ];

    group.bench_function("pattern_match", |b| {
        b.iter(|| {
            for reason in &reasons {
                let _ = black_box(match reason {
                    FinishReason::Stop => 0,
                    FinishReason::MaxTokens => 1,
                    FinishReason::Timeout => 2,
                    FinishReason::ContentFiltered => 3,
                    FinishReason::Cancelled => 4,
                });
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_result_creation,
    bench_streaming_output_creation,
    bench_finish_reason_matching
);
criterion_main!(benches);
