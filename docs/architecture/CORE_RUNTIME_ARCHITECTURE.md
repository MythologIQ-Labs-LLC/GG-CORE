# GG-CORE Runtime Architecture

**Status**: Living overview (reconciled against the code 2026-07-31, B-13).
**Companion docs**: `docs/CONCEPT.md` (the "why"), `docs/ARCHITECTURE_PLAN.md` (file-tree
contract + interface specs), the ADRs in `docs/architecture/`. Where this doc and the code
disagree, the code wins — fix this doc.

GG-CORE (**Greatest Good — Contained Offline Restricted Execution**) is a sandboxed, offline
inference engine that performs **model execution only**. It has no authority over data, tools, or
system actions. In the COREFORGE stack it is the compute tier:
Control (governance) → Vault (data) → Construct (persona) → Synapse (external) → **GG-CORE (compute)**.

## 1. C.O.R.E. design principles

| Principle | Meaning |
|-----------|---------|
| **Contained** | Sandbox with no ambient privileges (separate OS process, restricted user, seccomp/AppContainer). |
| **Offline** | Zero network access, inbound or outbound. |
| **Restricted** | IPC-only communication with authenticated callers — named pipes / Unix sockets; no HTTP/REST/WebSocket/localhost ports. |
| **Execution** | Pure compute — no business logic, no decision authority. |

Philosophy: resource-aware, multi-tenant triage ("Greatest Good for the Greatest Number") that
prioritizes system stability over any individual request.

## 2. Security boundaries

| Boundary | Rule |
|----------|------|
| Process | Separate OS process, restricted user, seccomp/AppContainer. |
| Filesystem | Read: `models/`, `tokenizers/`. Write: `temp/`, `cache/`. Deny all else. |
| Network | Deny all. |
| IPC | Named pipes / Unix sockets only. No HTTP/REST/WebSocket/localhost ports. |

**Forbidden modules** (their presence signals scope creep): `auth/`, `vault/`, `synapse/`,
`plugins/`, `network/`. **Forbidden dependencies**: `reqwest`, `hyper`, WebSocket libs, filesystem
-traversal libs.

## 3. Crate shape (consumable dependency)

`core-runtime/Cargo.toml` declares `crate-type = ["rlib", "cdylib"]`. GG-CORE is consumable four ways
from a single build:
- **Rust rlib** — embed `gg_core` directly (the COREFORGE path).
- **C ABI (cdylib)** — the C header `core-runtime/include/gg_core.h` (cbindgen) over
  `core-runtime/src/ffi/`.
- **Python (PyO3)** — `core-runtime/src/python/` (feature `python`).
- **CLI** — `core-runtime/src/cli/` (health probe).

## 4. Module map (`core-runtime/src/`)

| Module | Responsibility | Key files |
|--------|----------------|-----------|
| `ipc/` | IPC protocol + message types for authenticated callers | `ipc/protocol_types.rs` (`InferenceRequest`) |
| `scheduler/` | Priority queue, batching, worker pool | `scheduler/queue.rs` (`RequestQueue`), `scheduler/priority.rs` (`PriorityQueue` = `BinaryHeap`) |
| `engine/` | Inference, tokenizer, streaming, unified model trait, MoE | `engine/model.rs` (`Model` trait), `engine/inference.rs`, `engine/stream_*` |
| `models/` | Model load/registry/dispatch, hot-swap, versioning | `models/backend_dispatch.rs` (`load_model_dispatch`) |
| `memory/` | Buffer pool, GPU, KV cache, prompt cache, paged/arena | `memory/pool.rs` (`MemoryPool`), `memory/kv_cache.rs`, `memory/prompt_cache.rs` |
| `security/` | Ingress prompt-injection scan + egress PII sanitize | `security/pipeline.rs` (`SecurityPipeline`), `security/stream_sanitizer.rs` |
| `sandbox/` | Process sandboxing (seccomp/cgroup, unix-gated) | `sandbox/mod.rs`, `sandbox/unix*.rs` (`#[cfg(unix)]`) |
| `telemetry/` | Prometheus metrics, OpenTelemetry spans | `telemetry/` |
| `ab_testing/` | Traffic splitting + per-variant metrics | `ab_testing/` |
| `shim/` | Veritas shim: rate-limit + service-tier | `shim/rate_limiter.rs`, `shim/service_tier.rs` |
| `ffi/` | C FFI bindings (cdylib) | `ffi/mod.rs`, `ffi/models.rs` |
| `python/` | PyO3 bindings (feature `python`) | `python/mod.rs`, `python/session.rs` |
| `cli/` | Health-probe CLI | `cli/` |
| `deployment/`, `k8s/` | Deployment descriptors / K8s integration surfaces | `deployment/`, `k8s/` |

The public API is `core-runtime/src/lib.rs`; the secure product façade is
`core-runtime/src/runtime_facade.rs`.

## 5. The secure inference path (load-bearing invariant)

`Runtime::infer` / `Runtime::infer_stream` (`runtime_facade.rs`) is the **sole external inference
entry point**. Every call runs the security pipeline:

```
Runtime::infer(prompt)
  → SecurityPipeline::scan_prompt(prompt)        # ingress: prompt-injection scan (block on risk)
  → InferenceEngine::run*(...)                    # pub(crate) — not reachable externally
  → SecurityPipeline::sanitize_output(output)     # egress: PII redaction
      (streaming: security/stream_sanitizer.rs re-releases only settled, sanitized text)
```

`InferenceEngine::{run, run_cancellable, run_cancellable_with_memory_limit, run_stream_sync}` are
`pub(crate)` (B-33): a consumer **cannot** bypass the `SecurityPipeline`. This is why GG-CORE is
"secure by default" — installing the dependency and calling `runtime.infer()` is safe with no extra
wiring. See `docs/META_LEDGER.md` (B-33 seal) and COREFORGE #538 for the embedded-side migration.

## 6. Model dispatch (GGUF / ONNX)

`models/backend_dispatch.rs` `load_model_dispatch` selects the backend from a sibling
`manifest.json` next to the model file: ONNX when the manifest declares it, else the GGUF default.
Both backends are unified behind the `engine::Model` trait (`engine/model.rs`), so the engine
registry holds `Arc<dyn Model>` and streaming/non-streaming backends coexist (ONNX has no token
streaming). Production callers (FFI `ffi/models.rs`, Python `python/session.rs`) route through
`load_model_dispatch` — GGUF and ONNX are servable end-to-end (B-29).

## 7. Scheduler & memory

- **Scheduler** (`scheduler/`): `RequestQueue` wraps a `PriorityQueue` (a `BinaryHeap`, O(log n)
  push/pop, FIFO-stable within a priority) in an async `tokio::Mutex` + `Notify`. Priorities:
  Low/Normal/High/Critical. The queue op's cost is dominated by the async lock/notify, not the heap
  (B-37 measured ~0.55 µs/roundtrip, depth-insensitive).
- **Memory** (`memory/`): `MemoryPool` is a `parking_lot::Mutex<VecDeque<Vec<u8>>>` buffer pool
  (real reuse); `kv_cache` for paged KV; `prompt_cache` for exact + longest-prefix KV reuse
  (`find_prefix` is O(n), B-38). Resource limits gate concurrent memory.

## 8. Observability & degraded mode

- `telemetry/` exports Prometheus metrics + OpenTelemetry spans (no network egress — scraped).
- Under resource pressure the runtime degrades intentionally and explainably (context reduction
  before hard-fail) rather than failing opaquely (`engine/degraded_mode.rs`, B-07).

## 9. Governance

GG-CORE develops under the QoreLogic A.E.G.I.S. lifecycle: every change runs research → plan →
audit (PASS/VETO) → implement → substantiate, with a Merkle-chained decision log in
`docs/META_LEDGER.md`. Code-quality (Section 4 Razor): functions ≤ 40 lines, files ≤ 250 lines,
nesting ≤ 3. The current feature surface is enumerated in `docs/FEATURE_INDEX.md` (each row cites a
test that exercises it).

---

_This overview is maintained alongside the code. Update it in the same cycle as any architectural
change; `docs/ARCHITECTURE_PLAN.md` remains the binding file-tree + interface contract._
