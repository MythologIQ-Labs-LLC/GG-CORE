# Plan: B-37 — Profile the Scheduler Queue Hot Path (async RequestQueue overhead)

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Ships a **measurement** deliverable only: a `scheduler_queue_overhead` bench quantifying the
    async `RequestQueue::enqueue`/`dequeue` tax (tokio `Mutex` + `Notify`) over the bare
    `BinaryHeap`, plus a hotspot ranking. No scheduler logic change; the `PriorityQueue` (already
    optimal) is untouched.
- non_goals:
  - No optimization (a follow-up acts only if the measured tax is material — the B-35→B-36 pattern).
  - No change to `PriorityQueue`, `RequestQueue`, or any scheduler source.
- exclusions:
  - No change to `test`/`clippy`/`fmt`/`features` jobs beyond adding the new bench to the `bench` job.

## Open Questions

None. The construction path is verified: `RequestQueue::new(RequestQueueConfig { max_pending,
max_context_tokens })`; `enqueue(model_id: String, prompt: String, params: InferenceParams,
priority: Priority) -> Result<(u64, usize), QueueError>`; `dequeue() -> Option<QueuedRequest>`;
public via `gg_core::scheduler::{Priority, RequestQueue, RequestQueueConfig}`. Async is driven by a
`tokio` current-thread runtime + `block_on` (tokio is a full dep; no new criterion feature).

## Design Rationale (Simple Made Easy)

Research showed the `PriorityQueue`/`BinaryHeap` is optimal and already benched, but the actual
scheduler op — the async `RequestQueue` with its `Mutex` + `Notify` — is unmeasured. The bench is a
pure value harness: build a `RequestQueue`, drive `enqueue`/`dequeue` through a tokio runtime, and
report the per-op cost at several queue depths and a batch drain. Comparing against
`scheduler_throughput`'s bare-heap numbers isolates the concurrency tax, which is the number that
decides whether any scheduler optimization is warranted.

## Phase 1: Add the `scheduler_queue_overhead` bench

### Affected Files

- `core-runtime/benches/scheduler_queue_overhead.rs` (NEW) — criterion bench, `harness = false`,
  driving the async `RequestQueue` via a single `tokio::runtime::Builder::new_current_thread()`
  runtime:
  - Helper `queue()` = `RequestQueue::new(RequestQueueConfig { max_pending: 100_000,
    max_context_tokens: 4096 })` (large `max_pending` so batch sizes don't hit the cap; short prompt
    so the `prompt_bytes/4 > max_context` heuristic never rejects). `params()` = a fixed
    `InferenceParams`.
  - `bench_enqueue_dequeue_roundtrip`: group `scheduler_queue_roundtrip`. For depth ∈ {0, 64, 256},
    pre-fill a persistent queue to `depth` (outside timing, via `rt.block_on`), then
    `b.iter(|| rt.block_on(async { let (id,_) = q.enqueue(..).await.unwrap(); let r =
    q.dequeue().await; black_box((id, r)); }))` — one balanced enqueue+dequeue per iter at that
    depth. Measures the async Mutex+Notify tax + heap-depth effect.
  - `bench_batch_drain`: group `scheduler_queue_batch_drain`. For batch ∈ {16, 128, 512},
    `b.iter_batched(|| rt.block_on(fill(batch)), |q| rt.block_on(drain_all(&q)),
    BatchSize::SmallInput)` — amortized per-op cost of enqueuing then draining a batch under the
    lock (the datum the "batch-drain" follow-up question needs). `Throughput::Elements(batch)`.
  - `criterion_group!` + `criterion_main!` per convention.
- `core-runtime/Cargo.toml` — add `[[bench]]\nname = "scheduler_queue_overhead"\nharness = false`.
- `.github/workflows/rust.yml` — append `--bench scheduler_queue_overhead` to the `bench` job's
  `cargo bench` list (CI-safe set → 9). Model-free / GPU-free / default-feature.

### Changes

New bench file + one `[[bench]]` stanza + one `--bench` flag. No scheduler source change.

### Unit Tests

No unit test (a benchmark is the executable verification). The bench constructs the real
`RequestQueue` and drives `enqueue`/`dequeue`; a signature or panic regression fails compilation or
the run (the B-34 gate). Local verification: `cargo bench --bench scheduler_queue_overhead --
--warm-up-time 1 --measurement-time 2 --sample-size 10` runs both groups to completion, printing
per-depth and per-batch ns + throughput.

## Feature Inventory Touches

Empty — justified. Measurement-infrastructure change (a benchmark); no user-touchable runtime
feature is introduced or modified (the scheduler surface is unchanged).

## Definition of Done

### Deliverable: `scheduler_queue_overhead` bench quantifying the async scheduler tax

- **D1**: The async `RequestQueue::enqueue`/`dequeue` per-request cost (Mutex + Notify over the
  bare heap) is measured at several queue depths and a batch drain, and ranked against the
  bare-heap `scheduler_throughput` numbers — producing the datum that decides any scheduler
  follow-up.
- **D2**: `core-runtime/benches/scheduler_queue_overhead.rs` (NEW) with the two groups driving a
  real `RequestQueue`; `[[bench]] name = "scheduler_queue_overhead" harness = false` in
  `Cargo.toml`; `--bench scheduler_queue_overhead` in the `rust.yml` bench job.
- **D3**: META_LEDGER entries (canonical markup) research #168, plan, audit, seal; BACKLOG B-37 →
  done; a hotspot-ranking paragraph in the seal.
- **D4**: `cargo bench --bench scheduler_queue_overhead -- --warm-up-time 1 --measurement-time 2
  --sample-size 10` runs to completion locally (both groups report ns + throughput); CI: the
  `bench` job (now incl. `--bench scheduler_queue_overhead`) concludes success on the PR-to-main run.

## CI Commands

- `cargo bench --bench scheduler_queue_overhead -- --warm-up-time 1 --measurement-time 2 --sample-size 10` — the new bench compiles + runs to completion
- `cargo fmt --check` — formatting
- `cargo clippy --bench scheduler_queue_overhead -- -D warnings` — lint the new bench
