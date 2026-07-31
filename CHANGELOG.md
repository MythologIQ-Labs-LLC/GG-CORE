# Changelog

All notable changes to GG-CORE (Greatest Good - Contained Offline Restricted Execution) are documented in this file.

## [Unreleased]

### Documentation
- Adopted the canonical `docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md` onto `main` (B-21a) — the design of record that five FEATURE_INDEX rows (F-48/49/50/51/53) cited but which existed only on the unmerged PR #59 branch. Reconciled to the built reality: status Proposed → Accepted-implemented-(dormant), with an Implementation Status & Consolidation section recording that the ADR-007 stack (#61–#68, sealed #87–#94) is built + sealed but not yet wired into `Runtime::infer`, and the confirmed v1/v2 → single-`adaptive_speculative` retirement sequence.
- Added `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` (B-13) — the code-grounded technical spec that `CLAUDE.md` and `docs/TANDEM_EXPERIMENTS_PROPOSAL.md` cited but which did not exist (C.O.R.E. principles, security boundaries, module map, the secure `Runtime::infer` path, GGUF/ONNX dispatch, scheduler/memory, consumable-dependency shape).
- Corrected two mis-stated `docs/FEATURE_INDEX.md` rows (B-14): F-45 (Veritas shim) cited `n/a` despite 14 inline tests — now cited; F-38 (sandbox) marked verified with a unix-gated/CI-verified note. `feature_index_verify` reports 60/60 verified.

### Added
- ONNX inference servable end-to-end: `core_model_load` selects the GGUF or ONNX backend from a sibling `manifest.json` (`load_model_dispatch`); ONNX embed/classify reachable through FFI/Python (closes #72 scope-3).
- Degraded-mode policy: intentional, explained degradation under resource pressure — over-budget prompts are context-reduced instead of hard-failing (closes #53).

### Changed
- Unified GGUF/ONNX behind a single `engine::Model` trait; the registry now holds `Arc<dyn Model>`.
- `sandbox/unix.rs` split for Section 4 Razor (behavior unchanged).

### CI / Tooling
- Added a `bench` CI job (B-34) that runs the CI-safe default-feature benches on every PR to `main`, failing on compile error / bench panic and uploading the criterion baseline — preventing benchmark rot. (It immediately caught a rotted `ipc_throughput` bench → B-39.)
- Added a `security_overhead` bench (B-35) quantifying the per-call `SecurityPipeline` tax every `Runtime::infer` pays: `scan_prompt` ~8.7 ns/byte, `sanitize_output` ~53 ns/byte (the dominant stage), both linear per call. Joins the CI `bench` job. The linear-per-call sanitize result confirms the streaming egress re-sanitize is O(n²) over pushes → B-36 armed.
- Repaired the rotted `ipc_throughput` bench (B-39): `fixture_to_request` now derives the prompt from the fixtures' `prompt_tokens` size ladder instead of a missing top-level `prompt` string, and the bench is re-added to the CI `bench` job (the gate that caught the rot in B-34).
- Added a `scheduler_queue_overhead` bench (B-37) measuring the async `RequestQueue` enqueue/dequeue tax (tokio `Mutex` + `Notify`) over the bare `BinaryHeap`. Result (measurement only, no code change): ~550–620 ns per roundtrip, depth-insensitive, ~250 ns/op amortized under batch drain — <0.1% of per-request inference latency, so the scheduler is confirmed not a hotspot and needs no optimization.

### CI / Tooling (cont.)
- Added a run-over-run perf-regression gate (B-34b): the CI `bench` job caches the criterion baseline from `main` and, on each PR, restores it and fails if any tracked bench's median regresses beyond 2.0× (a deliberately generous gross-regression threshold, since the trimmed CI bench run is noisy). No committed absolute baseline (hardware-relative baselines are unsound); the comparison is same-runner-class run-over-run via `core-runtime/scripts/perf_gate.py`. This closes the optimization initiative's measurement + gating work.

### Fixed
- `PromptCache::find_prefix` was O(n²) — it re-hashed every prefix `tokens[..len]` from scratch for each length. It now does a single forward SHA256 pass (cloning the running hasher per prefix), making longest-prefix lookup O(n) with identical results (B-38). Confirmed by the new `prompt_cache_overhead` bench (flat throughput across 64/512/2048 tokens). The prompt cache is dormant (not yet wired into inference), so this removes a latent trap before it ships.

### Performance
- Streaming egress PII sanitizer is now O(n) per stream instead of O(n²) (B-36). Previously every generated token re-sanitized the entire accumulated buffer; it now caches the sanitized stable prefix and re-sanitizes only a bounded tail, rebasing the prefix at boundaries proven to split no PII match. Output is byte-identical to the previous whole-buffer sanitize (verified by a differential test against a whole-buffer reference + a one-shot oracle), and the release decision stays on sanitized text so internal-separator PII (e.g. credit-card numbers) is never split and leaked.

### Security / **BREAKING**
- **`Runtime::infer`/`infer_stream` is now the sole external inference entry point.** `InferenceEngine::{run, run_cancellable, run_cancellable_with_memory_limit, run_stream_sync}` are `pub(crate)` — a consumer can no longer bypass the `SecurityPipeline` (ingress scan + egress PII sanitize). Embedded consumers that called the raw engine must switch to `runtime.infer()` (see COREFORGE #538). `InferenceEngine`/`new` remain public.

## [0.8.2] - 2026-07-27

### Security & Dependency Hardening

Consolidates the security-chain wiring, the unified secure inference façade, the
CI feature matrix, and the dependency-advisory cleanup accumulated since 0.8.1
(PRs #71–#79). No breaking public API change.

#### Security

- **Security pipeline wired into production** (`src/security/pipeline.rs`, PR #73):
  ingress prompt-injection scan + egress PII sanitization now run on the live
  inference path instead of existing only in tests.
- **Unified secure inference façade** (`src/runtime_facade.rs`, PR #75):
  `Runtime::infer` / `infer_stream` route both the embedded (COREFORGE) and the
  consumable (FFI/Python) surfaces through the same scan → engine → sanitize path;
  fixed an FFI/Python enqueue-with-no-worker deadlock.

#### Dependencies (advisory cleanup)

- **pyo3 0.21 → 0.29** (PR #78): clears RUSTSEC-2026-0176 (high), RUSTSEC-2026-0177
  (medium), RUSTSEC-2025-0020 (low); `pyo3-asyncio-0-21` → `pyo3-async-runtimes`.
- **rand 0.8 → 0.9** (PR #79): final Dependabot item; `OsRng` migrated to the
  `TryRngCore::unwrap_err()` adapter, preserving CSPRNG + panic-on-entropy-failure
  semantics on the key/nonce generation path.
- **Dropped `atty`** (PR #76): `cbindgen` 0.26 → 0.28 removes the unmaintained
  `atty` build-time dependency (RUSTSEC-2024-0375/0378).

#### Added

- **Real ONNX classifier** (`src/engine/onnx/classifier.rs`, PR #77): candle-onnx
  `simple_eval` classifier with a pure `logits_to_classification` (softmax+argmax)
  helper and deterministic output selection.
- **CI feature matrix** (`.github/workflows/rust.yml`, PR #71): dedicated
  `features` legs building `gguf` / `onnx` / `ffi` / `python`, flushing out latent
  per-feature clippy/compile debt on the CI-invisible surfaces.

#### Verified

- ✅ Full matrix green under `-D warnings`: fmt + clippy + test across 3 OS, plus
  the gguf/onnx/ffi/python feature legs, CodeQL, and Analyze.
- ✅ All Dependabot advisories cleared as of this release.

---

## [0.8.1] - 2026-02-20

### E2E Model Inference Verified

This release fixes critical bugs in the GGUF backend and adds verified E2E testing with real models.

#### Fixed

- **GGUF Batch Logits** (`src/engine/gguf/backend.rs`): Fixed `add_seq()` to compute logits only for the last token in the prompt batch, required for sampling
- **Sampler Index** (`src/engine/gguf/backend.rs`): Fixed `sampler.sample()` to use `-1` (last output) instead of sequence position, matching llama-cpp-2 API expectations

#### Added

- **Speculative Decoding for GGUF** (`src/engine/gguf/speculative.rs`): 2-3x CPU speedup via draft-verify loop
  - `GgufDraftModel`: Wrapper implementing `DraftModel` trait
  - `GgufTargetModel`: Wrapper implementing `TargetModel` trait
  - Backend methods: `generate_from_tokens()`, `verify_tokens()`, `eos_token()`
- **E2E Model Test** (`tests/e2e_model_test.rs`): Real model inference tests with Qwen 2.5 0.5B
  - `e2e_load_and_generate`: Batch generation test
  - `e2e_streaming_generation`: Token-by-token streaming test
  - `e2e_chat_messages`: Chat message formatting with system/user roles
  - `e2e_speculative_decoding`: Speculative decoding integration test
  - `e2e_performance_benchmark`: Throughput measurement (tok/s)
- **Test Scripts**: PowerShell build script for VS2022 + LLVM environment setup

#### Verified

- ✅ GGUF model loading (Qwen 2.5 0.5B, 463 MiB, Q4_K)
- ✅ Batch generation (~40 tok/s on CPU release, ~21 tok/s debug)
- ✅ Streaming generation (20 tokens via async channel)
- ✅ Chat messages with role formatting
- ✅ Flash Attention enabled automatically
- ✅ Memory usage: 435 MiB model + 299 MiB compute + 6 MiB KV cache

#### Benchmark Hardware

- CPU: Intel Core i7-7700K (4c/8t @ 4.2 GHz)
- RAM: 32 GB DDR4-2400
- OS: Windows 10 x64
- Build: Release with `lto = "thin"`, `codegen-units = 1`

---

## [0.8.0] - 2026-02-19

### GG-CORE Rebrand & Extension Point Architecture

This release rebrands from "Veritas SPARK" to "GG-CORE" (Greatest Good - Contained Offline Restricted Execution) and introduces the extension point architecture for commercial multi-tenant features.

#### Added

- **Request Shim Interface** (`src/shim/mod.rs`): Extension point for commercial features
  - `RequestInterceptor` trait for rate limiting, priority tagging, tenant context
  - `PassthroughInterceptor` default no-op implementation
  - `InterceptResult` and `InterceptError` types for interception results
- **Open Core Architecture**: Clear separation between OSS runtime and commercial extensions
  - GG-CORE OSS: Apache 2.0 licensed core runtime
  - GG-CORE Nexus: Commercial extension point (separate repo)

#### Changed

- **Complete Rebrand**: All references updated from Veritas SPARK to GG-CORE
  - `veritas-spark` → `gg-core` (crate name, CLI, socket paths)
  - `VERITAS_SPARK_*` → `GG_CORE_*` (environment variables)
  - Updated all documentation, comments, and branding

#### Philosophy

GG-CORE adopts triage principles ("Greatest Good for the Greatest Number"):
- **C.O.R.E.**: Contained, Offline, Restricted, Execution
- Resource-aware, multi-tenant AI that prioritizes system stability
- Extension points for commercial tiered service models

---

## [0.7.0] - 2026-02-19

### Streaming Inference

This release introduces real token-by-token streaming inference via IPC.

#### Added

- **Streaming Inference**: Token-by-token streaming via IPC with `stream: true` parameter
- **Mid-Stream Cancellation**: Cancel active streaming requests with `CancelRequest` message
- **CLI `infer` Command**: New CLI command for direct inference
  - `gg-core infer --model <MODEL> --prompt <PROMPT>` - Single response
  - `gg-core infer --model <MODEL> --prompt <PROMPT> --stream` - Streaming output
- **IpcStreamBridge**: New adapter for sending streaming chunks to IPC clients
- **StreamChunk.text Field**: Optional decoded text field for client display

#### Changed

- **E2E Test Scripts**: Updated to include streaming verification (steps 5-7)

#### Wire Protocol

New streaming protocol (backward compatible):

```json
// Request with stream: true
{ "type": "inference_request", "request_id": 1, "model_id": "...", "prompt": "...", "parameters": { "stream": true } }

// Multiple response chunks
{ "type": "stream_chunk", "request_id": 1, "token": 15496, "text": "Hello", "is_final": false }
{ "type": "stream_chunk", "request_id": 1, "token": 198, "text": "!", "is_final": true }

// Cancel request
{ "type": "cancel_request", "request_id": 1 }
```

#### Internal

- `process_streaming()` in handler.rs for streaming inference coordination
- `run_stream_sync()` for blocking task integration
- Split read/write connection handling in server.rs
- CancellationToken integration for mid-stream abort

---

## [0.6.7] - 2026-02-19

### Production Safety Fixes

This release focuses on production safety and fail-fast behavior for the COREFORGE integration.

#### Fixed

- **Flash Attention Placeholder**: CUDA and Metal implementations now return explicit errors instead of zero vectors when kernel not implemented
- **Tokenizer Stub Behavior**: `encode()` and `decode()` now return `TokenizerError::NotLoaded` instead of silently returning empty results
- **Handler Metrics**: Fixed hardcoded `ModelHandle::new(0)` - now uses proper model lookup for metrics attribution
- **Telemetry Integration**: Handler now calls `telemetry::record_request_success()` and `record_request_failure()` for Prometheus-compatible metrics
- **FFI Streaming**: Updated to use model_id lookup; token-based API now fails fast with deprecation message
- **Benchmark Protocol**: Updated IPC throughput and scheduler benchmarks to use v0.6.5 text-based protocol

#### Added

- `InferenceEngine::get_handle()` method for model_id to ModelHandle resolution
- 8 new tests for InferenceEngine and InferenceParams validation
- Explicit version roadmap comments for unimplemented status --json fields (v0.7.0+)

#### Changed

- Tokenizer tests updated to expect `NotLoaded` errors instead of empty results
- Prompt fixtures updated to use text-based `prompt` field instead of `prompt_tokens`

### Breaking Changes

- FFI streaming with token arrays now returns `InvalidParams` error
- Stub tokenizer operations now fail instead of returning empty values

---

## [0.6.5] - 2026-02-18

### Text-Based IPC Protocol

- Eliminated mock data paths
- Changed IPC protocol from tokenized to text-based prompts
- Added chaos testing infrastructure

---

## [0.6.0] - 2026-02-17

### Functional GGUF Backend

- Functional GGUF inference via llama-cpp-2
- IPC server implementation
- Chaos testing framework

---

Copyright 2024-2026 GG-CORE Contributors
