# Plan: B-33 — `Runtime` as the Sole External Inference Entry Point

**change_class**: breaking

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - `InferenceEngine` the type and `InferenceEngine::new` stay `pub` (white-box tests and
    `Runtime` construction need them); only the *inference* methods are demoted.
- non_goals:
  - No behavior change to inference, security scanning, or the scheduler. This is a
    visibility change + a test relocation.
  - No change to `Runtime::infer`/`infer_stream` (the surviving secure path).
- exclusions:
  - The COREFORGE-side migration (call `runtime.infer()` instead of the raw engine) is
    tracked in COREFORGE #538, not here — this cycle compiler-enforces it.

## Open Questions

None. Consumer-contract fork resolved at cycle start: **hard demotion** — `Runtime::infer`
becomes the only external inference path.

## Design Rationale (Simple Made Easy)

A security façade is only *enforced* if the wrapped primitive's dangerous surface is not
also public. `Runtime::infer` scans+sanitizes, but `InferenceEngine::run*` is `pub`, so a
consumer can skip it. Demoting the four run methods to `pub(crate)` removes the bypass
without touching any logic: in-crate callers (the façade, the scheduler workers) keep
working; external callers cannot reach the raw engine and must use `Runtime`.

## Phase 1: Demote the raw inference methods to `pub(crate)`

### Affected Files

- `core-runtime/src/engine/inference.rs` — `pub async fn run` → `pub(crate) async fn run`;
  `pub async fn run_cancellable` → `pub(crate)`; `pub async fn
  run_cancellable_with_memory_limit` → `pub(crate)`. (`new`, `has_model`,
  `max_context_length`, `model_memory_usage`, `register_model`, `unregister_model` stay
  `pub` — they are non-inference or harmless.)
- `core-runtime/src/engine/inference_streaming.rs` — `pub fn run_stream_sync` →
  `pub(crate) fn run_stream_sync`.

### Changes

Visibility keyword only. In-crate callers unaffected: `runtime_facade.rs:76/116`,
`scheduler/worker.rs:195/205`, `scheduler/worker_streaming.rs:101`.

### Unit Tests

- No new unit test for the visibility change itself; it is a compile-level guarantee. The
  relocated engine tests (Phase 2) exercise the methods through the in-crate module, and
  `tests/security_pipeline_wiring_test.rs` proves the surviving secure path
  (clean prompt reaches the engine; injection ⇒ rejected) still works.

## Phase 2: Relocate the three engine-direct tests in-crate

- `core-runtime/src/engine/inference_chaos_tests.rs` (NEW, ~45 lines) — the three tests
  moved verbatim from the external chaos file:
  `chaos_inference_engine_context_exceeded` (huge prompt ⇒ error),
  `chaos_inference_engine_invalid_params` (bad params ⇒ error),
  `chaos_inference_engine_concurrent_requests` (concurrent `run` on an
  `Arc<InferenceEngine>` ⇒ all complete). Header: `use crate::engine::{InferenceEngine,
  InferenceParams}; use std::sync::Arc;`. A dedicated file (not the pre-existing 366-line
  `inference_tests.rs`) keeps every file ≤250 — `inference_tests.rs` is a pre-existing
  Razor state, out of scope to refactor here.
- `core-runtime/src/engine/inference.rs` — add `#[cfg(test)] #[path =
  "inference_chaos_tests.rs"] mod chaos_tests;` alongside the existing `mod tests;`.
- `core-runtime/tests/chaos_scheduler_shutdown_test.rs` — remove the three
  `chaos_inference_engine_*` tests; change the import
  `use gg_core::engine::{InferenceEngine, InferenceParams};` →
  `use gg_core::engine::InferenceParams;` (the file's scheduler/queue tests keep
  `InferenceParams`; `InferenceEngine` is no longer referenced there).

### Changes

Verbatim test relocation; the assertions are unchanged. The three tests now compile against
the `pub(crate)` `run` because they live in the crate.

### Unit Tests

Covered by the relocated tests themselves (they assert `run`'s returned `Result` — the
same behavior as before, now in-crate).

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| F-39 | MODIFIED | core-runtime/tests/security_pipeline_wiring_test.rs | External inference is reachable only via `Runtime::infer` (raw `InferenceEngine::run*` is `pub(crate)`); a clean prompt reaches the engine and an injection prompt is rejected — proving the sole path is security-enforced |

## Definition of Done

### Deliverable: `Runtime::infer` is the sole external inference path

- **D1**: A consumer of the crate cannot call inference without passing through the
  `SecurityPipeline`; the raw engine run surface is not part of the public API.
- **D2**: `InferenceEngine::{run,run_cancellable,run_cancellable_with_memory_limit,
  run_stream_sync}` are `pub(crate)` in `inference.rs`/`inference_streaming.rs`;
  `InferenceEngine`/`new` remain `pub`.
- **D3**: META_LEDGER entry (canonical markup) records the demotion; BACKLOG gains B-33
  (done); COREFORGE #538 updated to reflect the now-compiler-enforced migration; CHANGELOG
  notes the breaking internal-API change.
- **D4**: `chaos_inference_engine_context_exceeded` (relocated) passes in-crate, asserting
  `run` returns `ContextExceeded`; `cargo test` (external crate) confirms
  `chaos_scheduler_shutdown_test` still compiles/passes without the raw engine.

## CI Commands

```bash
cargo build -p gg-core --all-features                                   # full-feature compile
cargo clippy -p gg-core --all-targets --all-features -- -D warnings     # lint clean, warnings-as-errors
cargo test -p gg-core                                                   # default: relocated engine tests + external chaos/scheduler + wiring
cargo test -p gg-core --features gguf                                   # streaming (run_stream_sync) path compiles under pub(crate)
cargo fmt --check                                                       # formatting
```
