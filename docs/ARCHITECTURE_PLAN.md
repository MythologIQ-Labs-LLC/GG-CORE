# Architecture Plan

Workspace-level architecture contract for **GG-CORE** (Greatest Good —
Contained Offline Restricted Execution). Reconstructed 2026-07-08 during
governance remediation: the original bootstrap-era plan was never committed,
so this document records the contract the shipped `core-runtime/` already
fulfills (79 sealed ledger entries through the Veritas-Shim seal). Per-feature
plans live in `docs/plan-*.md` via `/qor-plan`; this file is the standing
workspace contract they chain from.

## Risk Grade

**Selected Grade**: [x] L3

### Risk Assessment Checklist

- [x] Contains security/auth logic -> **L3** (IPC caller auth, sandbox isolation, encryption, PII detection)
- [x] Modifies encryption or PII handling -> **L3** (`security/encryption*.rs`, `security/pii_*.rs`, FIPS mode)
- [x] Modifies existing APIs or data schemas -> **L2** (IPC protocol versioning)
- [x] Adds new business logic -> **L2** (scheduler, deployment strategies)
- [ ] UI-only changes, no logic -> L1
- [ ] Documentation or comments only -> L1

**Justification**: The runtime is a security boundary by definition — it
authenticates IPC callers, enforces sandbox isolation, and handles encryption
and PII redaction. Any change touching `ipc/`, `sandbox/`, or `security/` is
L3 and requires `/qor-audit` before implementation. Engine/scheduler logic
changes are L2 minimum.

---

## File Tree (The Contract)

```
core-runtime/
|-- src/
|   |-- main.rs             # Entry point; process setup
|   |-- lib.rs              # Public API surface
|   |-- config.rs           # Config & resource limits
|   |-- health.rs           # Health probe
|   |-- runtime_init.rs     # Runtime initialization
|   |-- shutdown.rs         # Graceful drain & shutdown
|   |-- cli_parser.rs       # CLI argument parsing
|   |-- ipc/                # Named-pipe/Unix-socket transport, auth, protocol
|   |-- scheduler/          # Queue, priority, batching, timeout/cancel
|   |-- engine/             # Inference, tokenizer, streaming, GPU, SIMD
|   |   |-- gguf/           # GGUF backend (llama-cpp-2)
|   |   |-- onnx/           # ONNX backend (candle: embedder, classifier)
|   |   `-- moe/            # Mixture of Experts (router, executor, combiner)
|   |-- models/             # Loader, registry, hot swap, versioning, routing
|   |-- memory/             # Pool, GPU memory, cache
|   |-- sandbox/            # OS-level isolation (seccomp / AppContainer)
|   |-- security/           # Encryption, FIPS, PII, prompt-injection, sanitizer
|   |-- telemetry/          # Prometheus metrics, OpenTelemetry spans
|   |-- deployment/         # Canary, blue-green, thresholds, rollback
|   |-- ab_testing/         # Traffic splitting + per-variant metrics
|   |-- k8s/                # Hardware profiles & validation
|   |-- shim/               # Veritas shim: rate limiter, service tiers
|   |-- ffi/                # C FFI bindings (cbindgen)
|   |-- python/             # PyO3 Python bindings
|   `-- cli/                # Health/status/config/models subcommands
|-- tests/                  # ~75 integration test binaries (see FEATURE_INDEX)
|-- benches/                # Criterion benchmark suite
|-- include/                # Generated C headers
`-- Cargo.toml              # Feature-gated: onnx, gguf, cuda, metal, advanced
```

Feature-to-file-to-test bindings are maintained in `docs/FEATURE_INDEX.md`
(47 indexed subsystems). New work MUST update that index in the same commit
(Phase 73 obligation).

---

## Interface Contracts

### IPC Server

**Purpose**: Sole ingress — authenticated request/response over named pipes
(Windows) or Unix domain sockets; no HTTP/REST/WebSocket/localhost ports.

**Input**: Length-prefixed framed messages per `docs/IPC_PROTOCOL_SCHEMA.md`
(versioned protocol; see `tests/protocol_version_test.rs`).

**Output**: Streamed or unary inference responses; structured error frames.

**Side Effects**: None outside `temp/` and `cache/` writes. No network I/O
(denied by design).

**Error Handling**: Typed error frames; caller auth failures close the
connection; errors never leak model paths or internal state.

### Scheduler

**Purpose**: Triage-principled admission — priority queueing, batching, and
resource-aware scheduling that favors system stability over individual
requests.

**Input**: Authenticated inference requests with priority class.
**Output**: Scheduled batch executions against the engine.
**Side Effects**: Queue state, telemetry counters.
**Error Handling**: Timeout/cancellation (`tests/timeout_cancel_test.rs`),
back-pressure rejection under load, graceful drain on shutdown.

### Engine

**Purpose**: Pure compute — model execution only, zero decision authority.

**Input**: Tokenized prompts + generation parameters from the scheduler.
**Output**: Token streams / embeddings / classifications.
**Side Effects**: GPU memory allocation via `memory/`; KV-cache state.
**Error Handling**: Backend errors surface as typed engine errors
(`engine/error.rs`); no panics across the FFI boundary.

---

## Data Flow

```
Caller (Vault/Construct via IPC)
      |
      v
ipc/ (auth -> protocol decode) --> reject unauthenticated
      |
      v
scheduler/ (priority queue -> batch) --> back-pressure / timeout
      |
      v
engine/ (tokenize -> prefill -> decode -> filter) --> telemetry/
      |
      v
security/ (output sanitize, PII redact)
      |
      v
ipc/ (stream response frames back to caller)
```

**Enforcement (SecurityPipeline)**: the `security/` stage is enforced in
`scheduler/worker.rs`. The ingress prompt-injection scan runs before the
resource guard on both the streaming and non-streaming paths; egress output
sanitization (PII redact) runs on the non-streaming response path. Streaming
egress carries u32 token IDs, not text, and is not sanitized in-runtime
(follow-up: `docs/BACKLOG.md` B-24).

**Delivery surfaces & secure entry point**: GG-CORE ships two delivery
surfaces — embedded (in-process `gg_core::Runtime`, e.g. COREFORGE) and
consumable component (FFI / Python bindings). `Runtime::infer` /
`Runtime::infer_stream` is the single secure entry point serving both: it
enforces the SecurityPipeline (ingress prompt-injection scan + egress PII
sanitize) around the engine, which stays pure compute per the C.O.R.E.
charter. The scheduler worker enforces the same pipeline for the IPC-server
path. A security block is a typed `InferenceError::SecurityRejected`
(embedded) — a distinct outcome the caller/UI renders, not a hang. Both
delivery surfaces are now unified on this single enforced entry point: the
embedded surface (COREFORGE via in-process `Runtime::infer`) and the
consumable surface (FFI/Python bindings) route through `Runtime::infer` /
`Runtime::infer_stream` — the FFI/Python bindings no longer enqueue-and-
deadlock and are now security-enforced (B-25b, ledger Entry #107). The
COREFORGE consumer switch (calling `runtime.infer`) is tracked as handoff
`docs/BACKLOG.md` B-26.

---

## Dependencies

| Package | Justification | Vanilla Alternative? |
|---------|---------------|----------------------|
| tokio | Async runtime for IPC + scheduler | No — hand-rolled async is a correctness hazard |
| serde | Protocol/config (de)serialization | No — schema evolution needs derive support |
| interprocess | Named pipes / Unix sockets | No — platform IPC abstraction |
| llama-cpp-2 (feature `gguf`) | GGUF model execution | No — inference backend |
| candle-core / candle-onnx (feature `onnx`) | ONNX embed/classify | No — inference backend |
| cudarc (feature `cuda`) / metal (feature `metal`) | GPU access | No — vendor APIs |
| pyo3 (feature `python`) | Python bindings | No — FFI codegen |
| cbindgen (feature `ffi`) | C header generation | No — FFI codegen |

**Forbidden dependencies** (scope-creep tripwires): `reqwest`, `hyper`, any
WebSocket library, filesystem traversal libraries.

**Forbidden modules** (ABORT if present): `auth/` (beyond IPC caller auth),
`vault/`, `synapse/`, `plugins/`, `network/`.

**Dependency Diet Check**:
- [x] Each dependency is truly necessary
- [x] No dependency replaceable with <10 lines vanilla code
- [x] No God packages; backends are feature-gated so default build is minimal

---

## Section 4 Razor Pre-Check

- [x] Functions <= 40 lines
- [x] Files <= 250 lines
- [x] Nesting <= 3 levels
- [x] No nested ternaries

Violations block implementation; `/qor-refactor` is the remediation path.

---

## Test Strategy

| Test Type | Target | Success Criteria |
|-----------|--------|------------------|
| Unit | per-module `*_tests.rs` in `src/` | logic invariants hold |
| Integration | `core-runtime/tests/` (~75 binaries) | end-to-end flows pass per feature |
| Security | `tests/security_*_test.rs` + `tests/security_audit/` | adversarial inputs rejected; no sandbox escape |
| Chaos | `tests/chaos_*_test.rs`, `tests/*_chaos_test.rs` | resilience under fault injection |
| Regression | `docs/FEATURE_INDEX.md` diff at seal | no outside-scope verified->unverified flips |
| Bench | `core-runtime/benches/` (Criterion) | no unexplained latency/throughput regression |

**CI feature coverage**: `.github/workflows/rust.yml` now carries a `features`
matrix job that builds, lints (`cargo clippy --features <f> --all-targets --
-D warnings`), and tests (`cargo test --features <f>`) each of `gguf/onnx/ffi/
python` — closing the prior gap where only default features were built, which
had hidden per-feature clippy debt (ffi/onnx/python/gguf). `cuda/metal/advanced`
remain CI-unbuilt (no GPU runners / proprietary toolchains).

---

## Security Considerations

- [x] No hardcoded secrets (CodeQL clean as of entry `fb022b1`; 30 alerts resolved)
- [x] No bypass mechanisms — IPC auth is unconditional
- [x] Input validation (`tests/security_input_validation_test.rs`, path-traversal defense)
- [x] Error messages don't leak info (sanitizer rules in `security/`)

**L3 standing rule**: any change under `ipc/`, `sandbox/`, or `security/`
requires a formal `/qor-audit` PASS before implementation. Known open gap:
sandbox verification is `unverified` on Linux/macOS pending issue #54
(clippy `-D warnings` failures in `sandbox/unix.rs`).

---

*Reconstructed under Qor-logic A.E.G.I.S. governance remediation*
*Phase: ENCODE (workspace contract, retrofit)*
*Persona: Governor*
*Status: ACTIVE — per-feature plans chain from this contract via /qor-plan*
