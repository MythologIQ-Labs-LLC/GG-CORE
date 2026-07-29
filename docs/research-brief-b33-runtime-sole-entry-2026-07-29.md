# Research Brief — B-33: `Runtime` as the Sole External Inference Entry Point

**Date**: 2026-07-29
**Analyst**: The Qor-logic Analyst
**Target**: B-33 — remove the public raw-inference footgun so a consumer cannot bypass
the `SecurityPipeline`. Make `Runtime::infer`/`infer_stream` the only inference path
reachable from outside the crate.
**Scope**: the public visibility of `InferenceEngine`'s run methods and the callers that
depend on them.
**Motivation**: consumers (COREFORGE and others) must get security by default — no extra
work, no way to accidentally skip ingress scan / egress PII sanitize.

---

## Executive Summary

`gg_core::engine::InferenceEngine::{run, run_cancellable, run_cancellable_with_memory_limit,
run_stream_sync}` are all `pub` (re-exported at `engine/mod.rs:77`). Any embedded consumer
can construct/reach an `InferenceEngine` and call `run()` directly, **bypassing the
`SecurityPipeline`** — the exact gap that forces COREFORGE #538. The secure façade
`Runtime::infer` (ingress scan → engine → egress sanitize) is the documented path
(USAGE_GUIDE), and the ONLY in-crate caller of the raw engine is the façade itself
(`runtime_facade.rs:76`) plus the scheduler workers. Demoting the four run methods to
`pub(crate)` makes `Runtime::infer` the sole external inference path — secure by default —
and breaks exactly **one** external test file's three engine-direct tests, which relocate
in-crate.

## Findings (verified)

### F1 — the raw run surface is public (the footgun)
- `engine/mod.rs:77` `pub use inference::{InferenceEngine, InferenceParams, InferenceResult};`
  and `lib.rs:26 pub mod engine;` → `gg_core::engine::InferenceEngine` is consumer-reachable.
- `inference.rs`: `pub async fn run` (:59), `run_cancellable` (:75),
  `run_cancellable_with_memory_limit` (:103); `inference_streaming.rs`: `pub fn
  run_stream_sync`. All bypass security (they call the model engine directly).

### F2 — the secure façade is the intended path
- `Runtime::infer` (`runtime_facade.rs:60`): `scan_prompt` → block ⇒ `SecurityRejected` →
  `inference_engine.run()` → `sanitize_output`. USAGE_GUIDE (`:104/:127`) documents
  `Runtime::new(config)` + `runtime.infer(...)`. FFI (`core_infer*`) and PyO3
  (`Session::infer`) already route through it (B-25b).

### F3 — in-crate callers survive `pub(crate)`
- `run`: only `runtime_facade.rs:76` (the façade). `run_cancellable*`: only
  `scheduler/worker.rs:195/205`. `run_stream_sync`: `runtime_facade.rs:116`,
  `worker_streaming.rs:101`. All in-crate → `pub(crate)` keeps them reachable.

### F4 — exactly one external test file breaks; three tests relocate
- `tests/chaos_scheduler_shutdown_test.rs` calls raw `engine.run()` in THREE tests only:
  `chaos_inference_engine_context_exceeded` (:162), `chaos_inference_engine_invalid_params`
  (:174), `chaos_inference_engine_concurrent_requests` (:186). These test the engine's own
  error behavior (context/params/concurrency) and belong in the in-crate engine test module
  `engine/inference_tests.rs`. The file's other tests exercise the *scheduler* (queue) and
  keep `InferenceParams` (used throughout) but drop the now-unused `InferenceEngine` import.
- `tests/security_pipeline_wiring_test.rs` only **constructs** `InferenceEngine::new`
  (stays `pub`) and drives the secure façade path — it does NOT call raw `run()`, so it is
  unaffected. `InferenceEngine` the type + `new()` stay public (needed to build a `Runtime`
  in white-box tests); only the *run* methods are demoted.

### F5 — no CLI / bin / example depends on raw run
- No `core-runtime/src/cli/` or `bin/` caller of `engine.run()`; the CLI is a health probe.

## Blueprint Alignment

| Claim | Actual finding | Status |
|---|---|---|
| `Runtime::infer` is the consumer inference API | True (USAGE_GUIDE, FFI/PyO3 route through it) | MATCH |
| Security is enforced for all inference | FALSE for the raw `InferenceEngine::run*` public path | GAP → B-33 |
| Consumers can't bypass the SecurityPipeline | They can, via the pub raw engine | GAP → B-33 |

## Recommendations (operator decision resolved at cycle start)

**Hard demotion** (operator-selected): change the four run methods to `pub(crate)`;
relocate the three `chaos_inference_engine_*` tests into `engine/inference_tests.rs`; drop
the unused `InferenceEngine` import from the chaos test file. `InferenceEngine` type +
`new()` stay `pub` (white-box construction of a `Runtime`). Result: `Runtime::infer`/
`infer_stream` is the **only** external inference path — secure by default, zero extra
consumer work for the correct path. Breaking for embedded consumers that call the raw
engine (COREFORGE): compiler-enforces the #538 migration; must land with the submodule bump.

## Updated Knowledge (Shadow Genome)

New pattern: **a "secure façade" is only secure if the insecure primitive is not also
public.** Wrapping `InferenceEngine` in a security-enforcing `Runtime` does not protect a
consumer who can still reach `InferenceEngine::run` directly. When adding a security
wrapper, demote the wrapped primitive's dangerous surface to `pub(crate)` in the same
change, or the wrapper is advisory, not enforced.

---

_Research complete. Consumer-contract decision (hard demotion) resolved at cycle start;
implementation is a visibility change + a three-test relocation, verified by clippy +
the security-pipeline-wiring suite + CI._
