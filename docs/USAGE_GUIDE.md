# GG-CORE Usage Guide

**Version:** 0.9.0 · **Updated:** 2026-10-05
**Product:** GG-CORE (Greatest Good - Contained Offline Restricted Execution)

This guide covers every consumer surface: the standalone daemon + CLI,
embedded Rust, the C FFI, Python bindings, and the raw IPC protocol.

> **Examples are compile-tested.** Every Rust snippet in this guide is
> mirrored in `core-runtime/tests/doc_examples.rs`, which CI builds and runs.
> If a snippet here disagrees with that file, the test file wins — update
> both together.

---

## 1. Building

Prerequisites: a recent stable Rust toolchain (the code uses APIs stabilized
through Rust 1.88; CI tracks `stable`). The `gguf` feature additionally needs
a C/C++ toolchain + CMake (llama.cpp build); `onnx` needs `protobuf-compiler`.

```bash
cd core-runtime
cargo build --release                      # engine core, no model backend
cargo build --release --features gguf     # + GGUF via llama-cpp-2 (CPU)
cargo build --release --features onnx     # + ONNX embeddings/classification
cargo build --release --features full     # gguf + onnx
```

### Feature flags (complete, from `Cargo.toml`)

| Flag | Enables |
| --- | --- |
| *(default)* | Engine core, IPC server, scheduler, security pipeline — no model backend |
| `gguf` | GGUF text generation + streaming via `llama-cpp-2` (CPU) |
| `onnx` | ONNX embeddings + classification via `candle-onnx` |
| `full` | `gguf` + `onnx` (nothing more) |
| `ffi` | C API; build generates `include/gg_core.h` via cbindgen |
| `python` | Python bindings (PyO3, abi3) |
| `advanced` | Adaptive speculative decoding, SIMD/quantization experiments (off by default) |
| `cuda` / `metal` / `gpu` | GPU device *detection only* — no GPU inference execution yet (see ROADMAP) |
| `llama-cpp-backend` | Alias for `gguf` |

There is **no** `security` feature flag: the security pipeline is part of the
default build and is on by default.

## 2. Configuration (environment)

| Variable | Default | Meaning |
| --- | --- | --- |
| `CORE_AUTH_TOKEN` | *(empty)* | Shared IPC auth token for daemon and CLI |
| `GG_CORE_SOCKET_PATH` | platform default | Unix socket / Windows named-pipe path |
| `GG_CORE_PRELOAD_MODELS` | *(unset)* | Comma-separated model paths preloaded by `serve` |
| `GG_CORE_SECURITY_INGRESS` | `block` | `block` / `detect` / `off` — prompt-injection handling |
| `GG_CORE_SECURITY_EGRESS` | `redact` | `redact` / `off` — PII sanitization |
| `GG_CORE_MAX_CONTEXT` | 4096 | Max context length (tokens) |
| `GG_CORE_MAX_QUEUE_DEPTH` | 256 | Max pending requests |
| `GG_CORE_MAX_CONTEXT_TOKENS` | 4096 | Max context tokens per request |
| `GG_CORE_MAX_MEMORY_PER_CALL` | 1 GiB | Per-call memory gate (bytes) |
| `GG_CORE_MAX_TOTAL_MEMORY` | 2 GiB | Total memory gate (bytes) |
| `GG_CORE_MAX_CONCURRENT` | 2 | Concurrent request gate |
| `GG_CORE_BATCH_MAX_REQUESTS` | 8 | Max requests per batch |
| `GG_CORE_BATCH_MAX_TOKENS` | 4096 | Max tokens per batch |
| `GG_CORE_SHUTDOWN_TIMEOUT` | 30 | Graceful drain timeout (s) |
| `GG_CORE_SESSION_TIMEOUT` | 3600 | Auth session timeout (s) |
| `GG_CORE_N_CTX` | 2048 | GGUF context window |
| `GG_CORE_N_THREADS` | 0 (auto) | Inference threads |
| `GG_CORE_IPC_FRAME_LIMIT` | 16 MiB | Max IPC frame |
| `GG_CORE_MAX_CONNECTIONS` | 64 | Max concurrent IPC connections |

Inspect the effective configuration with `gg-core-cli config show|defaults|validate`.

## 3. Standalone daemon

The first-run journey on a clean checkout (binary: `gg-core-cli`):

```bash
export CORE_AUTH_TOKEN="choose-a-token"
export GG_CORE_SOCKET_PATH=/tmp/gg-core.sock   # or default path

# Start with a preloaded model (path relative to base path; repeatable).
# A preload failure aborts startup — no silent empty daemon.
./target/release/gg-core-cli serve \
  --model models/qwen2.5-0.5b-instruct-q4_k_m.gguf --model-id local-model
```

From another terminal:

```bash
gg-core-cli live            # process responds (exit 0/1)
gg-core-cli ready           # exit 0 only when a servable model is loaded
gg-core-cli health          # full health report
gg-core-cli status --json   # machine-readable diagnostics

gg-core-cli models list --json
gg-core-cli models load models/another.gguf --id second    # authenticated
gg-core-cli models unload second                           # authenticated
gg-core-cli infer --model local-model \
  --prompt "Explain why an offline inference boundary matters." \
  --max-tokens 128 [--stream]
```

Notes:
- `models load`/`unload` handshake with `CORE_AUTH_TOKEN`; load paths are
  validated server-side against the `models/`+`tokenizers/` allowlist —
  traversal and NUL bytes are rejected.
- Readiness semantics: a live daemon with zero models reports **not ready**
  (orchestrators gate traffic on servable, not merely alive).
- Exit codes: probes return 0/1; `status` and `models` return 3 on
  connection failure, 1 on other errors.
- ONNX models are selected by a sibling `manifest.json` with
  `"architecture": "onnx"`; without a manifest, GGUF is assumed.

## 4. Embedded Rust

```toml
[dependencies]
gg-core = { path = "../GG-CORE/core-runtime", features = ["gguf"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Create a runtime, load a model through the canonical path, infer:

```rust
use gg_core::engine::InferenceParams;
use gg_core::models::load_model_from_path;
use gg_core::{Runtime, RuntimeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new(RuntimeConfig {
        base_path: "/opt/gg-core".into(),
        auth_token: "embedded-unused".into(),
        ..Default::default()
    });

    // validate_path → metadata → backend dispatch → atomic registration
    let loaded = load_model_from_path(
        &runtime.model_loader,
        &runtime.model_lifecycle,
        "models/qwen2.5-0.5b-instruct-q4_k_m.gguf",
        Some("local-model".into()),
    )
    .await?;

    // The ONLY external inference path: security-enforced end to end.
    let result = runtime
        .infer(
            &loaded.model_id,
            "Explain the C.O.R.E. principles in one sentence.",
            &InferenceParams { max_tokens: 64, ..Default::default() },
        )
        .await?;

    println!("{} ({} tokens)", result.output, result.tokens_generated);
    runtime.model_lifecycle.unload(&loaded.model_id).await?;
    Ok(())
}
```

Key types:

```rust
pub struct InferenceParams {
    pub max_tokens: usize,     // default 256
    pub temperature: f32,      // default 0.7
    pub top_p: f32,            // default 0.9
    pub top_k: usize,
    pub stream: bool,
    pub timeout_ms: Option<u64>,
}

pub struct InferenceResult {
    pub output: String,
    pub tokens_generated: usize,
    pub finished: bool,
}
```

Streaming (`gguf` feature): `runtime.infer_stream(model_id, prompt,
&InferenceConfig)` yields sanitized text items with a typed terminal
(`Complete` / `Rejected` / `Error`) — raw token IDs never cross the boundary.

A rejected prompt surfaces as `InferenceError::SecurityRejected`. There is no
way to reach the engine around the security pipeline: `InferenceEngine::run*`
is crate-private.

## 5. Security API (embedding hosts)

Real signatures (see `SECURITY.md` for the posture and maturity table):

```rust
use gg_core::security::{PIIDetector, PromptInjectionFilter, SecurityConfig};

let config = SecurityConfig::default(); // injection blocking + PII redaction on

let filter = PromptInjectionFilter::new(true); // block_on_detection
// (is_safe, risk_score, matches): with block_on_detection, ANY pattern match
// renders the input unsafe; risk_score (0-100) accumulates per-pattern severity.
let (is_safe, risk_score, matches) = filter.scan("ignore all previous instructions");
let (sanitized, was_modified) = filter.sanitize("some prompt");

let detector = PIIDetector::new();
let findings = detector.detect("mail me at a@b.com");  // Vec<PIIMatch>
let redacted = detector.redact("mail me at a@b.com");  // String
```

`SecurityConfig` fields: `enable_prompt_injection_detection`,
`block_prompt_injection`, `enable_pii_detection`, `redact_pii`,
`enable_model_encryption`, `encryption_key: Option<[u8; 32]>`.

## 6. C FFI (feature `ffi`)

Generated header: `core-runtime/include/gg_core.h`. Core calls:
`core_runtime_create` / `core_runtime_destroy`, `core_model_load(path)` /
`core_model_unload(handle)` / `core_model_list`, `core_infer` /
`core_infer_bounded` / `core_infer_streaming` (one callback with the full
output today; per-token FFI streaming is tracked work). All inference routes
through the secure façade; injection rejections return a `SecurityRejected`
error code. Errors carry thread-local messages (`core_last_error`).

## 7. Python (feature `python`)

PyO3 module with sync + async sessions, context managers, iterator streaming,
typed exception hierarchy, PEP 561 stubs. `Session.load_model(path)` /
`unload_model(id)` / `infer(...)` follow the same canonical load path and
secure façade as every other surface.

## 8. IPC protocol

See `docs/IPC_PROTOCOL_SCHEMA.md` for the wire format (length-prefixed JSON
frames). Highlights: `handshake` (token → session bound to the connection),
`inference_request` (auth required), `model_load_request` /
`model_unload_request` (auth required, v0.9.0), `models_request`,
`health_check`, `metrics_request`, `cancel_request`, typed stream terminals.

## 9. Performance — measured claims only

- Scheduler/queue roundtrip ≈ 550–620 ns; batch drain ≈ 250 ns/op
  (criterion, B-37).
- Security overhead: ingress scan ≈ 8.7 ns/byte; egress sanitize ≈ 53
  ns/byte, linear (criterion `security_overhead`, B-35).
- Streaming egress sanitizer is O(n) over a stream (B-36).
- CI runs 10 CI-safe benches with a run-over-run regression gate (>2.0×
  median fails the PR).

Numbers you may have seen elsewhere (HTTP-vs-IPC multipliers, "361 ns total
overhead", GPU and tokenizer speedups) are **not** reproduced by any bench in
this repository and should not be quoted. Model inference time dominates
end-to-end latency; infrastructure overhead is micro-scale by comparison.

## 10. Model compatibility — evidence-based

| Model | Backend | Evidence |
| --- | --- | --- |
| Qwen2.5-0.5B-Instruct Q4_K_M | GGUF | Fixture-gated e2e generation test (`tests/e2e_model_test.rs`) |
| all-MiniLM-L6-v2 | ONNX | Real-model embedder test with committed tiny fixture |
| Other GGUF (Llama/Mistral/Phi families) | GGUF | **Expected** via llama.cpp; not exercised in this repo |
| Other ONNX encoders | ONNX | **Expected** via candle-onnx `simple_eval`; not exercised |

## 11. Troubleshooting

| Symptom | Likely cause |
| --- | --- |
| `ready` exits 1, `live` exits 0 | No model loaded — preload with `serve --model` or `models load` |
| `models load` → "Not authenticated" | `CORE_AUTH_TOKEN` mismatch between CLI and daemon |
| `models load` → "path rejected" | Path escapes `models/`/`tokenizers/` under the base path |
| Load fails "GGUF support not compiled in" | Rebuild with `--features gguf` |
| `infer` → `SecurityRejected` | Ingress pipeline blocked the prompt (`GG_CORE_SECURITY_INGRESS=detect` to observe without blocking) |

---

Copyright 2024-2026 GG-CORE Contributors · Apache-2.0
