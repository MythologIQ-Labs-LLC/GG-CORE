# System State Snapshot

**Generated**: 2026-07-08 (ADR-007 epic + /qor-refactor cycle)
**Phase**: SUBSTANTIATE (Hardening Cycle 2 + Refactor)
**Status**: SEALED (push authorized — user approved 2026-07-08)
**Session ID**: 2026-07-08T2300-b36127 (Entry #95)

## Delta since prior seal (p8t9h0b1, 2026-02-14)

Branch topology after cycle 1:

- `main` — rebased onto origin/main `575d703`; carries `b048869` (Candle ONNX
  embedder) + `354d41d` (governance reconstruction: BACKLOG, FEATURE_INDEX,
  ARCHITECTURE_PLAN, GOVERNANCE_INDEX, META_LEDGER #80, cycle-1 gates).
- `style/cargo-fmt-sweep` — `774df09`: isolated 191-file cargo fmt sweep.
- `chore/hardening-ci-sandbox-lints` — `7a00233`: issue #54 lint-only fixes in
  `src/sandbox/unix.rs`; NEW `.github/workflows/rust.yml` (fmt/clippy/test CI,
  3-OS matrix); NEW `tests/python_binding_test.rs` (F-40 binding); FEATURE_INDEX
  F-38/F-40 updates. This seal's branch.
- `integration/preview-cycle1` — local verification preview (PR #47 tip
  `b661403` + cherry-picked `7a00233`); evidence base for the seal, disposable.

Verification evidence (preview base): 69 suites ok / 1073 tests passed;
sandbox_test 5/5, security_sandbox_escape 8/8, security_input_validation 11/11,
filter_test 10/10. 4 pre-existing failures reproduced on bare `b661403` and
filed as issues #55 (gguf validate_path ×2 + kv_cache addendum), #56 (13
residual clippy errors), #57 (security: NUL byte not rejected by validate_path).
Green-CI dependency chain: PR #47 → #56 → #55/#57 → this branch + fmt sweep.

## Prior snapshot (2026-02-14, session p8t9h0b1)

## Physical Tree

```
core-runtime/
├── Cargo.toml
├── src/
│   ├── main.rs                    [Graceful Shutdown - drain loop]
│   ├── lib.rs                     [Runtime Enhancements - OutputCache, ConnectionPool]
│   ├── shutdown.rs                [Graceful Shutdown - coordinator]
│   ├── health.rs                  [Health Check - HealthChecker]
│   ├── ipc/
│   │   ├── mod.rs                   [Runtime Enhancements - exports]
│   │   ├── auth.rs                  [Runtime Enhancements - connection tracking] **MODIFIED**
│   │   ├── encoding.rs              [Tier 2 - V2Encoder]
│   │   ├── handler.rs               [Runtime Enhancements - cancel/warmup handlers]
│   │   ├── health_handler.rs        [Runtime Enhancements - extracted health] **NEW**
│   │   ├── connections.rs           [Runtime Enhancements - ConnectionPool] **NEW**
│   │   └── protocol.rs              [Runtime Enhancements - Cancel/Warmup messages]
│   ├── scheduler/
│   │   ├── mod.rs                   [Runtime Enhancements - dedup export]
│   │   ├── pool.rs                  [Tier 2 - ThreadPoolConfig]
│   │   ├── continuous.rs            [Tier 4 - ContinuousBatcher]
│   │   ├── priority.rs              [Runtime Enhancements - iter() method]
│   │   ├── queue.rs                 [Runtime Enhancements - deadline/cancel]
│   │   ├── dedup.rs                 [Runtime Enhancements - OutputCache] **NEW**
│   │   └── batch.rs
│   ├── engine/
│   │   ├── mod.rs                   [Tier 3/4/5/6 - exports]
│   │   ├── config.rs
│   │   ├── error.rs
│   │   ├── input.rs
│   │   ├── output.rs
│   │   ├── filter.rs               [Pre-Testing - NFC normalization] **MODIFIED**
│   │   ├── inference.rs             [B-29b-1 - registry holds Arc<dyn Model>]
│   │   ├── inference_streaming.rs   [B-29b-1 - extracted GGUF streaming (Razor)]
│   │   ├── inference_degraded.rs    [B-07 - apply_degraded_context (Razor)]
│   │   ├── degraded_mode.rs         [B-07 - degraded-mode policy + evaluate]
│   │   ├── model.rs                 [B-29b-1 - unified backend-neutral Model trait]
│   │   ├── tokenizer.rs
│   │   ├── streaming.rs
│   │   ├── simd_tokenizer.rs        [Tier 3]
│   │   ├── simd_matmul.rs           [Tier 5 - AVX2 SIMD kernels]
│   │   ├── simd_neon.rs             [Tier 6 - NEON SIMD kernels]
│   │   ├── flash_attn.rs            [Tier 6 - Flash Attention CPU]
│   │   ├── speculative.rs           [Tier 3]
│   │   ├── quantize.rs              [Tier 4/6 - SIMD integration]
│   │   ├── prefill.rs               [Tier 4 - PrefillExecutor]
│   │   ├── decode.rs                [Tier 4 - DecodeExecutor]
│   │   ├── onnx/
│   │   │   ├── mod.rs
│   │   │   ├── classifier.rs
│   │   │   ├── embedder.rs
│   │   │   ├── tokenizer.rs            [B-28 - offline WordPiece]
│   │   │   ├── dispatch.rs             [B-29a - manifest→loader dispatch]
│   │   │   └── dispatch_tests.rs       [B-29a - 10 unit tests]
│   │   └── gguf/
│   │       ├── mod.rs
│   │       └── generator.rs
│   ├── models/
│   │   ├── mod.rs                   [Hot-Swap + ADR-007 exports]
│   │   ├── loader.rs                [Tier 2 - MappedModel]
│   │   ├── manifest.rs
│   │   ├── backend_dispatch.rs      [B-29b-2 - manifest→GGUF/ONNX load dispatch]
│   │   ├── registry.rs              [Hot-Swap - ModelHandle::new()]
│   │   ├── router.rs                [Hot-Swap - atomic routing]
│   │   ├── drain.rs                 [Hot-Swap - flight tracking]
│   │   ├── preload.rs               [Hot-Swap - preload validation]
│   │   ├── swap.rs                  [Hot-Swap - orchestration]
│   │   ├── speculative_config.rs    [ADR-007/#61 — AdaptiveSpeculativeConfig] **advanced**
│   │   ├── tier_synergy/            [ADR-007 — refactored from 397-line flat file] **advanced**
│   │   │   ├── mod.rs               (230 lines — TierSynergy orchestration)
│   │   │   ├── mode.rs              (29 lines — SynergyMode + SynergyResult)
│   │   │   ├── status.rs            (16 lines — SynergyStatus)
│   │   │   └── tests.rs             (112 lines — unit tests, cfg(test))
│   │   └── tier_synergy_speculative.rs [ADR-007/#64 — TierSpeculativePlan] **advanced**
│   ├── memory/
│   │   ├── mod.rs                   [Tier 3/4/5 - Arena, Paged, Q8KvStore, PromptCache exports]
│   │   ├── arena.rs                 [Tier 3]
│   │   ├── paged.rs                 [Tier 4 - PageTable, 16 tokens/page]
│   │   ├── kv_quant.rs              [Tier 5 - Q8 KV storage]
│   │   ├── prompt_cache.rs          [Tier 5 - LRU prompt cache]
│   │   ├── pool.rs
│   │   ├── gpu.rs
│   │   ├── cache.rs                 [Tier 2 - KvCache]
│   │   └── limits.rs
│   ├── sandbox/
│   │   ├── mod.rs
│   │   ├── windows.rs
│   │   ├── unix.rs                  [B-16 - struct + Sandbox impl (535→147, Razor)]
│   │   ├── unix_seccomp.rs          [B-16 - seccomp-bpf filter]
│   │   ├── unix_seccomp_defs.rs     [B-16 - BPF opcode tables]
│   │   ├── unix_cgroup.rs           [B-16 - cgroup v2 limits]
│   │   └── unix_tests.rs            [B-16 - sandbox unit tests]
│   └── telemetry/                   [Observability Stack]
│       ├── mod.rs                   [Metrics Export - store exports]
│       ├── logging.rs
│       ├── spans.rs
│       ├── metrics.rs
│       └── store.rs                 [Metrics Export - MetricsStore]
├── tests/
│   ├── auth_test.rs
│   ├── backend_test.rs
│   ├── baseline_comparison_test.rs
│   ├── bench_fixtures_test.rs
│   ├── competitive_comparison_test.rs
│   ├── connections_test.rs          [Runtime Enhancements - 6 tests] **NEW**
│   ├── dedup_test.rs                [Runtime Enhancements - 8 tests] **NEW**
│   ├── drain_test.rs                [Hot-Swap - drain tests]
│   ├── encoding_roundtrip_test.rs
│   ├── filter_test.rs
│   ├── flash_attn_test.rs           [Tier 6 - Flash Attention tests]
│   ├── health_test.rs               [Health Check - 11 tests]
│   ├── inference_types_test.rs
│   ├── integration_end_to_end_test.rs
│   ├── integration_gguf_test.rs
│   ├── integration_onnx_test.rs
│   ├── kv_quant_test.rs             [Tier 5 - Q8 KV tests]
│   ├── limits_test.rs
│   ├── memory_test.rs               [Tier 3 - arena tests]
│   ├── metrics_export_test.rs       [Metrics Export - 11 tests]
│   ├── model_router_test.rs         [Hot-Swap - router tests]
│   ├── preload_test.rs              [Hot-Swap - preload tests]
│   ├── prompt_cache_test.rs         [Tier 5 - Cache tests]
│   ├── protocol_test.rs
│   ├── protocol_version_test.rs
│   ├── runtime_enhancements_integration_test.rs [Runtime Enhancements - 5 tests] **NEW**
│   ├── sandbox_test.rs
│   ├── scheduler_test.rs
│   ├── security_filter_adversarial_test.rs  [Pre-Testing - NFC tests] **MODIFIED**
│   ├── security_hash_verification_test.rs
│   ├── security_input_validation_test.rs
│   ├── security_path_traversal_test.rs
│   ├── security_sandbox_escape_test.rs
│   ├── shutdown_test.rs             [Graceful Shutdown - 10 tests]
│   ├── simd_matmul_test.rs          [Tier 5 - SIMD tests]
│   ├── simd_neon_test.rs            [Tier 6 - NEON tests]
│   ├── speculative_test.rs          [Tier 3]
│   ├── streaming_test.rs            [Streaming Response - 10 tests]
│   ├── swap_integration_test.rs     [Hot-Swap - integration tests]
│   ├── telemetry_test.rs            [Observability Stack]
│   ├── tier4_paged_continuous_test.rs   [Tier 4 - Phase 1-2 tests]
│   ├── tier4_quantize_decode_test.rs    [Tier 4 - Phase 3-4 tests]
│   ├── timeout_cancel_test.rs       [Runtime Enhancements - 10 tests] **NEW**
│   ├── tokenizer_test.rs            [Tier 3]
│   └── warmup_test.rs               [Runtime Enhancements - 6 tests] **NEW**
├── benches/
│   ├── concurrent_load.rs
│   ├── generation_throughput.rs
│   ├── inference_latency.rs
│   ├── ipc_throughput.rs
│   ├── memory_overhead.rs
│   └── scheduler_throughput.rs
└── fixtures/
    ├── baselines/
    │   ├── baseline_metrics.json
    │   └── external_references.json
    └── prompts/
        ├── small.json
        ├── medium.json
        └── large.json
```

## Runtime Enhancements Bundle Compliance

| Phase | Promised (Blueprint) | Delivered | Lines | Tests | Status |
|-------|---------------------|-----------|-------|-------|--------|
| 0 | Handler Split | `health_handler.rs` | 84 | — | PASS |
| 1 | Timeout/Cancel | `queue.rs`, `protocol.rs` | 182, 238 | 10 | PASS |
| 2 | Model Warm-up | `handler.rs`, `protocol.rs` | 245, 238 | 6 | PASS |
| 3 | Request Dedup | `dedup.rs` | 122 | 8 | PASS |
| 4 | Connection Mgmt | `connections.rs`, `auth.rs` | 77, 148 | 6 | PASS |
| 5 | Integration | `lib.rs`, `ipc/mod.rs` | 145, 23 | 5 | PASS |

**Blueprint Match**: 8/8 components (100%)

**Architecture**:
```
Request Timeout & Cancellation:
  ├── InferenceParams.timeout_ms → deadline computed on enqueue
  ├── QueuedRequest.is_cancelled() / is_expired()
  ├── RequestQueue.cancel(id) → marks request cancelled
  └── dequeue() skips cancelled/expired requests

Model Warm-up via IPC:
  └── IpcMessage::WarmupRequest { model_id, tokens }
      └── NO AUTH REQUIRED (orchestrator pattern)
          └── Enqueue minimal inference, return elapsed_ms

Request Deduplication:
  └── OutputCache with SHA256(tokens + params) key
      ├── TTL-based expiration (default 30s)
      └── LRU eviction at max_entries

Connection Management:
  └── ConnectionPool with global max_connections
      ├── try_acquire() → ConnectionGuard (RAII)
      └── SessionAuth.track_connection() per session
```

**Components**:
- `HealthHandler` - Extracted health check handling (orchestrator pattern)
- `ConnectionPool` - Global connection limiting with RAII guards
- `ConnectionGuard` - RAII connection release
- `OutputCache` - SHA256-keyed response cache with TTL
- `CachedOutput` - Output tokens + timestamp
- `DedupResult` - Cached/Queued enum
- `InferenceParams.timeout_ms` - Optional request deadline
- `QueuedRequest.deadline` - Computed from timeout_ms
- `QueuedRequest.cancelled` - AtomicBool for cancellation
- `WarmupRequest/WarmupResponse` - IPC types for model priming
- `CancelRequest/CancelResponse` - IPC types for request cancellation

---

## Pre-Testing Hardening Bundle Compliance

| Phase | Promised (Blueprint) | Delivered | Lines | Tests | Status |
|-------|---------------------|-----------|-------|-------|--------|
| 1 | Unicode NFC Normalization | `filter.rs` | 127 (+22) | 4 | PASS |
| 2 | V2 Encoding Tests | Already exists | — | 8 (existing) | SKIPPED |
| 3 | DashMap Sessions | Deferred | — | — | DEFERRED |

**Blueprint Match**: 1/1 required phases (100%)

**Changes Applied**:
- `Cargo.toml` - Added `unicode-normalization = "0.1"`
- `src/engine/filter.rs` - NFC normalization, pre-computed blocklist (127 lines)
- `tests/security_filter_adversarial_test.rs` - 4 new Unicode tests (208 lines)

**Security Finding Addressed**:
- Z.ai Report: "lack of Unicode normalization before filtering could allow bypass"
- Solution: NFC normalization applied to both blocklist (at construction) and input (at comparison)

---

## File Inventory (Updated)

### Source Files (66 files)

| Path | Lines | Purpose |
|------|-------|---------|
| `src/main.rs` | 64 | Entry point + Graceful shutdown |
| `src/lib.rs` | 145 | Public API + OutputCache + ConnectionPool |
| `src/shutdown.rs` | 137 | Coordinator |
| `src/health.rs` | 122 | HealthChecker |
| `src/ipc/mod.rs` | 23 | IPC root + connection exports |
| `src/ipc/auth.rs` | 148 | Session auth + connection tracking |
| `src/ipc/handler.rs` | 245 | Request handling + cancel/warmup |
| `src/ipc/health_handler.rs` | 84 | **[NEW]** Extracted health handling |
| `src/ipc/connections.rs` | 77 | **[NEW]** ConnectionPool + Guard |
| `src/ipc/protocol.rs` | 238 | Wire format + Cancel/Warmup messages |
| `src/ipc/encoding.rs` | 111 | Token encoding (V1 + V2) |
| `src/scheduler/mod.rs` | 15 | Scheduler root + dedup export |
| `src/scheduler/pool.rs` | 44 | ThreadPoolConfig |
| `src/scheduler/continuous.rs` | 166 | ContinuousBatcher |
| `src/scheduler/priority.rs` | 106 | Priority queue + iter() |
| `src/scheduler/queue.rs` | 182 | Request queue + deadline/cancel |
| `src/scheduler/dedup.rs` | 122 | **[NEW]** OutputCache |
| `src/scheduler/batch.rs` | 98 | Batching logic |
| `src/engine/mod.rs` | 56 | Engine root |
| `src/engine/inference.rs` | 108 | Core inference + timeout_ms |
| _(remaining 46 files unchanged)_ | | |

### Test Files (45 files)

| Path | Lines | Tests | Purpose |
|------|-------|-------|---------|
| `tests/timeout_cancel_test.rs` | 183 | 10 | **[NEW]** Timeout/cancellation |
| `tests/warmup_test.rs` | 98 | 6 | **[NEW]** Model warm-up |
| `tests/dedup_test.rs` | 156 | 8 | **[NEW]** Deduplication |
| `tests/connections_test.rs` | 95 | 6 | **[NEW]** Connection pool |
| `tests/runtime_enhancements_integration_test.rs` | 124 | 5 | **[NEW]** Integration |
| _(remaining 40 files unchanged)_ | | |

## Test Summary

**Total Source Files**: 66 (+3 from Runtime Enhancements)
**Total Test Files**: 45 (+5 from Runtime Enhancements)
**Total Benchmark Files**: 6
**Tests Passing**: 440 (+4 from Pre-Testing Hardening)

### Test Breakdown

| Category | Count | Status |
|----------|-------|--------|
| Security Validation | 54 | PASS |
| Baseline/Competitive | 15 | PASS |
| Integration Tests | 37 | PASS |
| Tier 2 Tests | 22 | PASS |
| Tier 3 Tests | 31 | PASS |
| Observability Tests | 22 | PASS |
| Tier 4 Tests | 22 | PASS |
| Tier 5 Tests | 26 | PASS |
| Tier 6 Tests | 15 | PASS |
| Hot-Swap Tests | 25 | PASS |
| Graceful Shutdown Tests | 10 | PASS |
| Health Check Tests | 11 | PASS |
| Metrics Export Tests | 11 | PASS |
| Streaming Response Tests | 10 | PASS |
| Runtime Enhancements Tests | 35 | PASS |
| Existing Tests | 94 | PASS |
| **Total** | **440** | **ALL PASS** |

### Runtime Enhancements Test Summary

| Category | Tests | Status |
|----------|-------|--------|
| Timeout/Cancel | 10 | PASS |
| Warmup | 6 | PASS |
| Dedup | 8 | PASS |
| Connections | 6 | PASS |
| Integration | 5 | PASS |
| **Total** | **35** | **PASS** |

## Section 4 Razor Compliance

| Check | Limit | Actual | Status |
|-------|-------|--------|--------|
| Max function lines | 40 | ~25 | PASS |
| Max file lines | 250 | 245 (handler.rs) | PASS |
| Max nesting depth | 3 | 2 | PASS |
| Nested ternaries | 0 | 0 | PASS |

### Runtime Enhancements File Compliance

| File | Lines | Limit | Status |
|------|-------|-------|--------|
| handler.rs | 245 | 250 | PASS |
| health_handler.rs | 84 | 250 | PASS |
| connections.rs | 77 | 250 | PASS |
| dedup.rs | 122 | 250 | PASS |
| queue.rs | 182 | 250 | PASS |
| protocol.rs | 238 | 250 | PASS |
| auth.rs | 148 | 250 | PASS |
| lib.rs | 145 | 250 | PASS |

## Dependencies Status

### Current Dependencies

| Package | Version | Purpose | Status |
|---------|---------|---------|--------|
| tokio | 1.35 | Async runtime | APPROVED |
| serde | 1.0 | Serialization | APPROVED |
| serde_json | 1.0 | JSON parsing | APPROVED |
| interprocess | 2.0 | IPC | APPROVED |
| thiserror | 1.0 | Error handling | APPROVED |
| async-trait | 0.1 | Async traits | APPROVED |
| sha2 | 0.10 | Cryptographic hashing | APPROVED |
| hex | 0.4 | Hex encoding | APPROVED |
| regex | 1.10 | Output filtering | APPROVED |
| unicode-normalization | 0.1 | NFC normalization (security) | APPROVED |
| toml | 0.8 | Config parsing | APPROVED |
| memmap2 | 0.9 | Zero-copy model loading | APPROVED |
| tracing | 0.1 | Structured diagnostics | APPROVED |
| tracing-subscriber | 0.3 | Log formatting | APPROVED |
| metrics | 0.22 | Metrics facade | APPROVED |

### Forbidden Dependencies

| Package | Status |
|---------|--------|
| reqwest | NOT PRESENT |
| hyper | NOT PRESENT |
| tungstenite | NOT PRESENT |
| tokio-tungstenite | NOT PRESENT |
| walkdir | NOT PRESENT |

## Forbidden Modules Status

| Module | Status |
|--------|--------|
| `auth/` | NOT PRESENT |
| `vault/` | NOT PRESENT |
| `synapse/` | NOT PRESENT |
| `plugins/` | NOT PRESENT |
| `network/` | NOT PRESENT |

## Debug Artifacts

| Pattern | Count |
|---------|-------|
| `println!` | 0 |
| `dbg!` | 0 |
| `eprintln!` | 3 (shutdown messages only) |
| `console.log` | 0 |
| `TODO` | 0 |
| `FIXME` | 0 |
| `HACK` | 0 |

---
