//! IPC encoding/decoding throughput benchmarks.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::fs;

use gg_core::engine::InferenceParams;
use gg_core::ipc::protocol::{
    decode_message, decode_message_binary, encode_message, encode_message_binary,
    InferenceRequest, IpcMessage, RequestId,
};

fn load_fixture(name: &str) -> serde_json::Value {
    let path = format!("fixtures/prompts/{}.json", name);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Failed to load fixture: {}", path));
    serde_json::from_str(&content).expect("Invalid JSON in fixture")
}

fn fixture_to_request(fixture: &serde_json::Value, request_id: u64) -> InferenceRequest {
    let prompt = fixture["prompt"]
        .as_str()
        .expect("prompt must be a string")
        .to_string();

    let params = &fixture["parameters"];
    InferenceRequest {
        request_id: RequestId(request_id),
        model_id: fixture["model_id"].as_str().unwrap().to_string(),
        prompt,
        parameters: InferenceParams {
            max_tokens: params["max_tokens"].as_u64().unwrap() as usize,
            temperature: params["temperature"].as_f64().unwrap() as f32,
            top_p: 1.0,
            top_k: 50,
            stream: false,
            timeout_ms: None,
        },
    }
}

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode_message");

    for (name, token_count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_fixture(name);
        let request = fixture_to_request(&fixture, 1);
        let message = IpcMessage::InferenceRequest(request);

        group.throughput(Throughput::Elements(token_count));
        group.bench_with_input(BenchmarkId::new("tokens", name), &message, |b, msg| {
            b.iter(|| encode_message(black_box(msg)))
        });
    }

    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_message");

    for (name, token_count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_fixture(name);
        let request = fixture_to_request(&fixture, 1);
        let message = IpcMessage::InferenceRequest(request);
        let encoded = encode_message(&message).expect("encode failed");

        group.throughput(Throughput::Elements(token_count));
        group.bench_with_input(BenchmarkId::new("tokens", name), &encoded, |b, bytes| {
            b.iter(|| decode_message(black_box(bytes)))
        });
    }

    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    for (name, token_count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_fixture(name);
        let request = fixture_to_request(&fixture, 1);
        let message = IpcMessage::InferenceRequest(request);

        group.throughput(Throughput::Elements(token_count));
        group.bench_with_input(BenchmarkId::new("tokens", name), &message, |b, msg| {
            b.iter(|| {
                let encoded = encode_message(black_box(msg)).unwrap();
                decode_message(black_box(&encoded))
            })
        });
    }

    group.finish();
}

fn bench_encode_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode_binary");

    for (name, token_count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_fixture(name);
        let request = fixture_to_request(&fixture, 1);
        let message = IpcMessage::InferenceRequest(request);

        group.throughput(Throughput::Elements(token_count));
        group.bench_with_input(BenchmarkId::new("tokens", name), &message, |b, msg| {
            b.iter(|| encode_message_binary(black_box(msg)))
        });
    }

    group.finish();
}

fn bench_decode_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_binary");

    for (name, token_count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_fixture(name);
        let request = fixture_to_request(&fixture, 1);
        let message = IpcMessage::InferenceRequest(request);
        let encoded = encode_message_binary(&message).expect("encode failed");

        group.throughput(Throughput::Elements(token_count));
        group.bench_with_input(BenchmarkId::new("tokens", name), &encoded, |b, bytes| {
            b.iter(|| decode_message_binary(black_box(bytes)))
        });
    }

    group.finish();
}

fn bench_roundtrip_binary(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip_binary");

    for (name, token_count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_fixture(name);
        let request = fixture_to_request(&fixture, 1);
        let message = IpcMessage::InferenceRequest(request);

        group.throughput(Throughput::Elements(token_count));
        group.bench_with_input(BenchmarkId::new("tokens", name), &message, |b, msg| {
            b.iter(|| {
                let encoded = encode_message_binary(black_box(msg)).unwrap();
                decode_message_binary(black_box(&encoded))
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_encode,
    bench_decode,
    bench_roundtrip,
    bench_encode_binary,
    bench_decode_binary,
    bench_roundtrip_binary
);
criterion_main!(benches);
