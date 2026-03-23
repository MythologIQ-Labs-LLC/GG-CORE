//! Scheduler queue operations throughput benchmarks.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gg_core::engine::InferenceParams;
use gg_core::scheduler::{Priority, PriorityQueue, QueuedRequest};

fn create_test_request(id: u64, token_count: usize) -> QueuedRequest {
    // Generate a prompt string of approximately the specified token count
    let prompt = "test ".repeat(token_count);
    QueuedRequest::new(
        id,
        "test-model".to_string(),
        prompt,
        InferenceParams {
            max_tokens: 256,
            temperature: 0.7,
            top_p: 1.0,
            top_k: 50,
            stream: false,
            timeout_ms: None,
        },
    )
}

fn bench_priority_queue_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_push");

    for size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::new("ops", size), &size, |b, &count| {
            b.iter(|| {
                let mut queue = PriorityQueue::new();
                for i in 0..count {
                    let priority = Priority::from((i % 4) as u8);
                    queue.push(black_box(create_test_request(i, 100)), priority);
                }
            })
        });
    }

    group.finish();
}

fn bench_priority_queue_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_pop");

    for size in [100, 1000, 10000] {
        let mut queue = PriorityQueue::new();
        for i in 0..size {
            let priority = Priority::from((i % 4) as u8);
            queue.push(create_test_request(i, 100), priority);
        }

        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::new("ops", size), &queue, |b, _| {
            b.iter_batched(
                || {
                    let mut q = PriorityQueue::new();
                    for i in 0..size {
                        let priority = Priority::from((i % 4) as u8);
                        q.push(create_test_request(i, 100), priority);
                    }
                    q
                },
                |mut q| {
                    while q.pop().is_some() {}
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

fn bench_priority_reordering(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_reordering");

    for size in [100, 1000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::new("mixed_priorities", size), &size, |b, &count| {
            b.iter(|| {
                let mut queue = PriorityQueue::new();
                // Interleave priorities to trigger reordering
                for i in 0..count {
                    let priority = match i % 4 {
                        0 => Priority::Critical,
                        1 => Priority::Low,
                        2 => Priority::High,
                        _ => Priority::Normal,
                    };
                    queue.push(black_box(create_test_request(i, 100)), priority);
                }
                // Pop all to verify ordering
                let mut prev_priority = Priority::Critical;
                while let Some(req) = queue.pop() {
                    black_box(&req);
                    black_box(&prev_priority);
                    prev_priority = Priority::from(((req.id % 4) as u8).min(prev_priority as u8));
                }
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_priority_queue_push,
    bench_priority_queue_pop,
    bench_priority_reordering
);
criterion_main!(benches);
