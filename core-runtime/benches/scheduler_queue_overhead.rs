//! Scheduler `RequestQueue` overhead benchmarks.
//!
//! The bare `PriorityQueue` (a `BinaryHeap`) is already benched by
//! `scheduler_throughput`. This quantifies the ACTUAL scheduler op —
//! `RequestQueue::enqueue`/`dequeue`, which wrap the heap in an async
//! `tokio::Mutex` + `Notify` — so the per-request concurrency tax over the bare
//! heap is measured (the scheduler analogue of `security_overhead`, B-37). Async
//! is driven by a single current-thread tokio runtime; no model, no GPU, default
//! feature → CI-safe.

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};

use gg_core::engine::InferenceParams;
use gg_core::scheduler::{Priority, RequestQueue, RequestQueueConfig};
use tokio::runtime::Runtime;

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("current-thread runtime")
}

fn queue() -> RequestQueue {
    // Large max_pending so batch sizes never hit the cap; a short prompt keeps the
    // `prompt_bytes/4 > max_context` heuristic from ever rejecting.
    RequestQueue::new(RequestQueueConfig {
        max_pending: 100_000,
        max_context_tokens: 4096,
    })
}

fn params() -> InferenceParams {
    InferenceParams {
        max_tokens: 64,
        temperature: 0.7,
        top_p: 1.0,
        top_k: 50,
        stream: false,
        timeout_ms: None,
    }
}

async fn enqueue_one(q: &RequestQueue) {
    q.enqueue(
        "bench-model".to_string(),
        "hello".to_string(),
        params(),
        Priority::Normal,
    )
    .await
    .expect("enqueue");
}

async fn fill(depth: usize) -> RequestQueue {
    let q = queue();
    for _ in 0..depth {
        enqueue_one(&q).await;
    }
    q
}

/// One balanced enqueue+dequeue per iteration at a fixed queue depth: isolates the
/// async Mutex + Notify tax (plus the O(log depth) heap op) over the bare heap.
fn bench_enqueue_dequeue_roundtrip(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("scheduler_queue_roundtrip");

    for depth in [0usize, 64, 256] {
        let q = rt.block_on(fill(depth));
        group.bench_with_input(BenchmarkId::new("depth", depth), &q, |b, q| {
            b.iter(|| {
                rt.block_on(async {
                    enqueue_one(q).await;
                    black_box(q.dequeue().await);
                })
            })
        });
    }

    group.finish();
}

/// Amortized per-op cost of enqueuing a batch then draining it all — the datum a
/// batch-drain follow-up would need.
fn bench_batch_drain(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("scheduler_queue_batch_drain");

    for batch in [16usize, 128, 512] {
        group.throughput(Throughput::Elements(batch as u64));
        group.bench_function(BenchmarkId::new("batch", batch), |b| {
            b.iter_batched(
                || rt.block_on(fill(batch)),
                |q| {
                    rt.block_on(async {
                        while let Some(r) = q.dequeue().await {
                            black_box(r);
                        }
                    })
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_enqueue_dequeue_roundtrip, bench_batch_drain);
criterion_main!(benches);
