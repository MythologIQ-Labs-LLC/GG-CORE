//! Concurrent load benchmarks.
//!
//! Measures scheduler throughput with priority queue operations.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

use gg_core::engine::InferenceParams;
use gg_core::scheduler::{Priority, PriorityQueue, QueuedRequest};

fn create_request(id: u64, token_count: usize) -> QueuedRequest {
    // Generate a prompt string of approximately the specified token count
    let prompt = "test ".repeat(token_count);
    QueuedRequest::new(
        id,
        "test-model".to_string(),
        prompt,
        InferenceParams {
            max_tokens: 100,
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

    for (name, queue_size) in [("empty", 0), ("half_full", 50), ("near_full", 90)] {
        let mut queue: PriorityQueue<QueuedRequest> = PriorityQueue::new();

        // Pre-fill the queue
        for i in 0..queue_size {
            queue.push(create_request(i as u64, 100), Priority::Normal);
        }

        group.throughput(Throughput::Elements(1));
        group.bench_function(BenchmarkId::new("push", name), |b| {
            let mut id = queue_size as u64;
            b.iter(|| {
                queue.push(black_box(create_request(id, 100)), Priority::Normal);
                id += 1;
                // Pop to prevent overflow
                let _ = queue.pop();
            })
        });
    }

    group.finish();
}

fn bench_priority_queue_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_pop");

    for (name, batch_size) in [("single", 1), ("batch_4", 4), ("batch_8", 8)] {
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_function(BenchmarkId::new("batch", name), |b| {
            b.iter(|| {
                let mut queue: PriorityQueue<QueuedRequest> = PriorityQueue::new();
                // Fill with batch_size items
                for i in 0..batch_size {
                    queue.push(create_request(i as u64, 100), Priority::Normal);
                }
                // Pop all
                for _ in 0..batch_size {
                    let _ = black_box(queue.pop());
                }
            })
        });
    }

    group.finish();
}

fn bench_priority_ordering(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_ordering");

    // Mix of priorities
    group.throughput(Throughput::Elements(10));
    group.bench_function("mixed_priority_10", |b| {
        b.iter(|| {
            let mut queue: PriorityQueue<QueuedRequest> = PriorityQueue::new();

            // Enqueue mixed priorities
            for i in 0..10u64 {
                let req = create_request(i, 100);
                let priority = if i % 3 == 0 {
                    Priority::Critical
                } else if i % 3 == 1 {
                    Priority::High
                } else {
                    Priority::Normal
                };
                queue.push(req, priority);
            }

            // Pop all (should be priority-ordered)
            for _ in 0..10 {
                let _ = black_box(queue.pop());
            }
        })
    });

    group.finish();
}

fn bench_queue_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_throughput");

    for count in [100, 500, 1000] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_function(BenchmarkId::new("requests", count), |b| {
            b.iter(|| {
                let mut queue: PriorityQueue<QueuedRequest> = PriorityQueue::new();

                // Enqueue all
                for i in 0..count {
                    queue.push(create_request(i as u64, 50), Priority::Normal);
                }

                // Pop all
                while queue.pop().is_some() {}
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_priority_queue_push,
    bench_priority_queue_pop,
    bench_priority_ordering,
    bench_queue_throughput
);
criterion_main!(benches);
