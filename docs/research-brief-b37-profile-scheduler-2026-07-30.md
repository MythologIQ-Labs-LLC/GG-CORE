# Research Brief — B-37: Profile the Scheduler Queue Hot Path

**Date**: 2026-07-30
**Analyst**: The Qor-logic Analyst
**Target**: B-37 — profile scheduler/batching throughput under concurrency and rank hotspots,
guided by the `scheduler_throughput` + `concurrent_load` baselines (B-34). Fourth cycle of the
optimization pass; measurement-first (like B-35).
**Scope**: `scheduler/priority.rs` (`PriorityQueue`), `scheduler/queue.rs` (`RequestQueue`), and
the two existing scheduler benches.

---

## Executive Summary

The scheduler's core data structure is already optimal: `PriorityQueue` is a
`BinaryHeap<PrioritizedItem<T>>` (O(log n) push/pop) with a cheap allocation-free `Ord` (compare
`priority` u8, then `sequence` u64 for FIFO-stable ordering). The two existing benches measure
exactly this bare structure — so there is **no algorithmic win at the data-structure layer**. What
they do NOT measure is the actual production scheduler operation: `RequestQueue::enqueue`/`dequeue`,
which wrap the heap in an async `tokio::Mutex` plus a `Notify` wakeup. That concurrency layer is
the only place per-request scheduler overhead can hide, and it is unmeasured. Recommendation
(measurement-first, mirroring B-35): add a `scheduler_queue_overhead` bench that quantifies the
async `enqueue`/`dequeue` round-trip cost (the Mutex + Notify tax over the bare heap), joining the
CI bench set. The result decides whether a follow-up scheduler optimization (lock sharding /
lock-free / batch-drain) is warranted — or whether, like a clean profiling result, the scheduler
needs no change.

## Findings (verified)

### F1 — `PriorityQueue` is optimal; the data-structure benches confirm it
- `priority.rs:59-84`: `BinaryHeap<PrioritizedItem<T>>`; `push`/`pop` are O(log n). `Ord`
  (`:49-56`) compares `priority as u8` then `sequence` (u64) — no allocation, no string/hash work,
  FIFO-stable within a priority via a monotonic `next_sequence`. This is textbook-optimal for a
  priority queue; there is no faster general structure.
- `benches/scheduler_throughput.rs` + `benches/concurrent_load.rs` both bench the BARE
  `PriorityQueue` (`push`/`pop`/reordering) single-threaded — they measure F1's structure, not the
  scheduler's real op. So a "make the scheduler faster" effort has nothing to optimize here.

### F2 — the real scheduler op is async and UNMEASURED
- `queue.rs:41-56`: `RequestQueue { queue: Arc<Mutex<PriorityQueue<QueuedRequest>>>, streaming:
  Arc<Mutex<VecDeque<…>>>, notify: Arc<Notify>, … }`. `enqueue` (`:61`) locks the async Mutex,
  pushes, and notifies; `dequeue` (`:128`) locks, pops, and skips cancelled/expired entries. Every
  scheduled request pays this Mutex-acquire + Notify cost on top of the O(log n) heap op — and no
  bench covers it. This is the scheduler analogue of B-35's per-call `SecurityPipeline` tax.

### F3 — async benching is feasible without new deps
- `tokio` is a full dependency with `rt-multi-thread` (`Cargo.toml:17`). No existing bench is async
  (`memory_overhead`'s "concurrent" bench is sync `try_acquire`), so this is a new pattern, but a
  `tokio::runtime::Runtime` + `block_on` inside `b.iter` benches the async `enqueue`/`dequeue`
  without adding criterion's `async_tokio` feature. `RequestQueue`/`RequestQueueConfig` are public
  (used by `tests/security_pipeline_wiring_test.rs`), and `enqueue(model_id, prompt, params,
  priority) -> Result<(u64, usize)>` / `dequeue() -> Option<QueuedRequest>` are the surfaces to bench.

### F4 — the load-bearing comparison
- Benching the async `enqueue`/`dequeue` round-trip against `scheduler_throughput`'s bare-heap
  push/pop numbers isolates the **concurrency tax** (Mutex + Notify + async machinery) as a
  multiple of the raw heap op. If the tax is small, the scheduler needs no optimization (a clean
  profiling result — report and stop). If it is large under batch drain, it warrants a follow-up
  (lock sharding / batch-drain that pops N under one lock). B-37 produces the number that decides.

## Blueprint Alignment

| Optimization-brief expectation | Finding | Status |
|---|---|---|
| Scheduler/batching throughput under concurrency | Data structure optimal (F1); concurrency layer unmeasured (F2) | MATCH — measure the layer, not the heap |
| Guided by concurrent_load + scheduler_throughput | Those bench the bare heap only (F1) | DRIFT → add the async RequestQueue bench |

## Recommendations

1. **B-37 deliverable**: add `benches/scheduler_queue_overhead.rs` — a tokio-runtime bench measuring
   `RequestQueue::enqueue`, `dequeue`, and an enqueue→dequeue round-trip across batch sizes,
   quantifying the async Mutex + Notify tax over the bare heap. Add it to the CI `bench` job. Report
   the tax-vs-bare-heap ratio and rank the scheduler hot path.
2. **No optimization this cycle** (B-37 measures; a follow-up acts only if the tax is material) —
   the same measure-then-decide discipline that took B-35 → B-36.
3. **Do NOT touch `PriorityQueue`** — it is optimal; changing it is unwarranted (YAGNI).

## Updated Knowledge (Shadow Genome)

**Profile the layer that actually runs, not the structure underneath it.** The scheduler benches
measured a bare `BinaryHeap` and implied "scheduler is benched" — but production always goes through
the async `RequestQueue` (Mutex + Notify), which nothing measured. When a component wraps a data
structure in a concurrency layer, the layer is the hot path to profile; benching the bare structure
gives a falsely-complete picture.

---

_Research complete. B-37 = a `scheduler_queue_overhead` bench quantifying the async RequestQueue
tax over the (already-optimal) heap; the tax-vs-bare-heap ratio decides any follow-up. Measurement
only._
