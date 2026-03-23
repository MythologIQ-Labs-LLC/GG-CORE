//! Inference latency benchmarks.
//!
//! Measures input validation and processing pipeline latency.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gg_core::engine::{ChatMessage, ChatRole, InferenceInput, InferenceParams};

fn create_text_input(length: usize) -> InferenceInput {
    let text = "a".repeat(length);
    InferenceInput::Text(text)
}

fn create_chat_input(message_count: usize, length_per_message: usize) -> InferenceInput {
    let messages: Vec<ChatMessage> = (0..message_count)
        .map(|i| ChatMessage {
            role: if i % 2 == 0 { ChatRole::User } else { ChatRole::Assistant },
            content: "x".repeat(length_per_message),
        })
        .collect();
    InferenceInput::ChatMessages(messages)
}

fn bench_input_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("input_validation_latency");

    for (name, length) in [("256_chars", 256), ("2048_chars", 2048), ("16384_chars", 16384)] {
        let input = create_text_input(length);

        group.throughput(Throughput::Bytes(length as u64));
        group.bench_with_input(BenchmarkId::new("text", name), &input, |b, inp| {
            b.iter(|| inp.validate())
        });
    }

    group.finish();
}

fn bench_chat_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("chat_validation_latency");

    for (name, count) in [("2_messages", 2), ("10_messages", 10), ("32_messages", 32)] {
        let input = create_chat_input(count, 500);
        let total_bytes = count * 500;

        group.throughput(Throughput::Bytes(total_bytes as u64));
        group.bench_with_input(BenchmarkId::new("chat", name), &input, |b, inp| {
            b.iter(|| inp.validate())
        });
    }

    group.finish();
}

fn bench_params_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("params_creation");

    group.bench_function("inference_params", |b| {
        b.iter(|| {
            InferenceParams {
                max_tokens: black_box(100),
                temperature: black_box(0.7),
                top_p: black_box(1.0),
                top_k: black_box(50),
                stream: false,
                timeout_ms: None,
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_input_validation,
    bench_chat_validation,
    bench_params_creation
);
criterion_main!(benches);
