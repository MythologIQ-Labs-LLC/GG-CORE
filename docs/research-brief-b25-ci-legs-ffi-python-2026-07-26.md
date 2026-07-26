# Research Brief

**Date**: 2026-07-26T11:40:12-04:00
**Analyst**: The Qor-logic Analyst
**Target**: BACKLOG B-25 — consumable FFI/Python surface: CI feature legs +
pre-existing defect remediation + inference reroute through the secure façade.
**Scope**: Discover exactly what breaks when the `gguf`/`onnx`/`ffi`/`python`
features are compiled (they are never built by the default-only CI), and map the
FFI/Python inference reroute surface.
Step 2.5: target is a backlog item, not a GH issue — pre-check skipped.

---

## Executive Summary

Every optional-feature surface carries pre-existing clippy debt that is invisible
because `.github/workflows/rust.yml` builds only default features (`default = []`).
Measured by actually compiling each feature: **ffi = 18 clippy errors** (17
`missing_safety_doc` + 1 raw-pointer-not-`unsafe`) plus `ffi/inference.rs` at 272
lines (Razor), **onnx = 2**, **python = 3** (builds clean), **gguf = 6**. None can
pass a `-D warnings` CI leg today. This confirms and quantifies Shadow Genome #7:
a CI-invisible surface accumulates compile/lint debt a change would inherit.
B-25 therefore splits cleanly into (1) a CI-foundation cycle — make all four
features clippy-clean, extract `ffi/inference.rs` under Razor, add the four CI
legs (mechanical, no behavior change, L2) — and (2) the FFI/Python inference
reroute through `Runtime::infer`/`infer_stream` (L3, security surface), which the
new legs then verify.

## Findings

### CI state (verified)
- `.github/workflows/rust.yml`: two jobs, both 3-OS (`ubuntu/macos/windows`),
  `dtolnay/rust-toolchain@stable`: `lint` (`cargo fmt --check`; `cargo clippy
  --all-targets -- -D warnings`) and `test` (`cargo test --workspace`). **No
  feature flags anywhere** — gguf/onnx/ffi/python never compiled in CI.
  `codeql.yml` is the only other workflow.
- `Cargo.toml [features]`: `onnx=[candle-core,candle-onnx]`,
  `gguf=[llama-cpp-2,encoding_rs]`, `ffi=[cbindgen]`,
  `python=[pyo3,pyo3-asyncio-0-21]`, `full=[onnx,gguf]` (Cargo.toml:124-135).
  Build cost: gguf compiles llama.cpp C++ (heavy; the local MSVC/cmake toolchain
  DID build it), onnx compiles candle (moderate), python needs Python dev
  headers (present locally — `cargo check --features python` succeeded), ffi runs
  cbindgen (build.rs:8-49, generates `include/gg_core.h`).

### Pre-existing feature-build defects (measured by compiling each)
- **ffi** — `cargo clippy --features ffi --all-targets -D warnings` → **18
  errors**: `missing_safety_doc` on the unsafe `extern "C"` fns in
  `ffi/auth.rs` (17,73,97,105), `ffi/health.rs` (15,58,69,88),
  `ffi/inference.rs` (133,169), `ffi/models.rs` (15,78,117,151,163,191),
  `ffi/streaming.rs` (162); and `clippy::not_unsafe_ptr_arg_deref` at
  `ffi/runtime.rs:31`. Plus **Razor**: `ffi/inference.rs` = 272 lines (>250);
  other ffi files ≤250 (`streaming.rs` 166, `runtime.rs` 136, `error.rs` 141).
- **onnx** — 2 errors: `engine/onnx/embedder.rs:53` (needless `return`), `:101`
  (redundant full-range slicing).
- **python** — compiles clean (`cargo check --features python` exit 0); clippy
  `-D warnings` → 3 errors (field-assignment-after-`Default::default`, 2
  redundant closures) in `python/`.
- **gguf** — 6 errors in the gguf backend: `pos`-used-as-loop-counter ×3,
  unnecessary `f32`→`f32` cast, clamp-like pattern, needless `return`.
- **ffi/error.rs exhaustiveness**: already fixed this session — both
  `From<engine::InferenceError>` (11 variants, error.rs:77-111) and
  `From<engine::inference::InferenceError>` (6 variants incl. MemoryExceeded +
  SecurityRejected, error.rs:127-141) are exhaustive; `--features ffi` COMPILES
  (only clippy `-D warnings` fails, on the safety-doc lints).

### FFI/Python reroute surface (the deferred L3 half)
- Every inference entry point enqueues to `request_queue` then awaits `rx` —
  but no worker is spawned in FFI/Python init (only `runtime_init.rs:124`, the
  IPC-server path), so all **deadlock**: `core_infer` (`ffi/inference.rs:19-99`,
  body 71-87), `core_infer_bounded` (`ffi/inference.rs:169-259`; adds a
  caller-provided output buffer + `BufferTooSmall` check, else identical),
  `core_infer_streaming` (`ffi/streaming.rs:66-158`; NOT true streaming — one
  callback with full output), `Session::infer` (`python/session.rs:69-98`),
  `AsyncSession::infer` (`python/session.rs:187-223`).
- Reroute target: call `rt.inner.infer(model_id, prompt, &params).await`
  (`runtime_facade.rs:60-81`) — handles ModelNotLoaded and SecurityRejected;
  drop the (absent) pre-checks. `infer_stream` (`runtime_facade.rs:93-116`) is
  `#[cfg(feature="gguf")]`, so FFI/Python streaming reroute must be gguf-gated
  with a non-gguf fallback error.

## Blueprint Alignment

| Claim | Finding | Status |
|---|---|---|
| BACKLOG B-25 "add CI legs first" | Confirmed necessary — legs expose 29 clippy errors that must be fixed for green | MATCH |
| Shadow Genome #7 (CI-invisible debt) | Quantified: 18/2/3/6 errors across ffi/onnx/python/gguf | MATCH (evidence) |
| FEATURE_INDEX F-39 (FFI) "verified" | FFI has 18 clippy errors + Razor overage + deadlocking entry points — "verified" overstates it | DRIFT |
| FEATURE_INDEX F-40 (python) "unverified" | Accurate — python not built/tested in CI | MATCH |
| ARCHITECTURE_PLAN: FFI/Python are consumable surfaces | True, but currently deadlock on inference (no worker) | DRIFT (documented in B-25) |

## Recommendations (B-25 split)

1. **P1 — CI-foundation cycle FIRST (this auto-dev cycle; L2, mechanical, no
   behavior change).** (a) Make each feature clippy-clean by deriving the fix
   map from captured `cargo clippy --features <f> --all-targets` output (per
   Shadow Genome #3 — never guess a lint): fix the 18 ffi safety-doc + unsafe
   lints, 2 onnx, 3 python, 6 gguf. (b) Extract `ffi/inference.rs` (272→≤250)
   under Razor. (c) Add four CI legs to `rust.yml` (`--features gguf`,
   `--features onnx`, `--features ffi`, `--features python`; gguf/onnx on
   ubuntu; ffi cross-OS; python where dev headers exist). Result: the consumable
   surface becomes verified ground.
2. **P1 — Reroute cycle NEXT (L3).** Route the five FFI/Python entry points
   through `Runtime::infer`/`infer_stream` (deadlock fix + security enforcement),
   gguf-gate the streaming reroute, map SecurityRejected. Now verifiable by the
   legs from (1).
3. **P2 — Correct FEATURE_INDEX F-39** from "verified" once the ffi leg is green
   (it currently overstates coverage).

## Updated Knowledge

- Shadow Genome #7 gains concrete evidence: the exact per-feature clippy debt
  (ffi 18, onnx 2, python 3, gguf 6). Add to the countermeasure: the CI-leg
  cycle must budget for the debt the leg exposes, fixing it as deliverable #1.

---

_Research complete. Findings are advisory — implementation decisions remain
with the Governor._
