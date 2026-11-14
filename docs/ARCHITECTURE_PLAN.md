# Architecture Plan

Historical workspace-level architecture plan for **GG-CORE** (Greatest Good —
Contained Offline Restricted Execution). Reconstructed 2026-07-08 during
governance remediation: the original bootstrap-era plan was never committed,
so this document preserves the historical architecture intent and implementation trail (79 sealed ledger entries through the Veritas-Shim seal). **Do not treat this file as proof of present release capability.** The [standalone runtime architecture](architecture/CORE_RUNTIME_ARCHITECTURE.md), [live source](../core-runtime/src/), and [workflow qualification map](architecture/WORKFLOW_AND_RELEASE_BOUNDARIES.md) control current factual claims. Per-feature
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
|   |   |-- onnx/           # ONNX backend (candle: embedder, classifier, manifest dispatch)
|   |   `-- moe/            # Mixture of Experts (router, executor, combiner)
|   |-- models/             # Loader, registry, hot swap, versioning, routing
|   |-- memory/             # Pool, GPU memory, cache
|   |-- sandbox/            # Linux seccomp; Windows Job Object resources; no macOS daemon sandbox
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
(see the live index; historical counts are not immutable). New work MUST update that index in the same commit
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

## Data flow (current standalone boundary)

Earlier historical Vault/Construct and CoreForge-centric examples were superseded by the standalone-runtime decision. Runtime architecture is application-independent, and the request pipeline must not be confused with the model-loader control path.

```text
Any local client or application
  → Authenticated local IPC (daemon) OR embedded Rust / C / Python Runtime facade
  → Request validation + ingress security scan
  → Resource admission, priority queue and scheduler
  → Registered model instance and native GGUF / source-conditional ONNX backend
  → Sanitized ordinary response OR incrementally sanitized streaming text
  → Same local client
```

Separately, a client-operated model load goes through allowed relative paths, optional manifest/hash validation, backend dispatch, model lifecycle registration and resident-memory limits. An unload can report registry removal while a held native instance is still charged; `model_release_wait_request` verifies actual last-holder release. See [IPC protocol](IPC_PROTOCOL_SCHEMA.md).

**Current versus developmental switching:** `v0.10.0-rc.1` and `main@1032d1b` do not publish an authenticated atomic A→B switch. An independently reviewed Windows **in-process** coordinator exists on a development branch; an additive authenticated switch IPC/status journal exists on a separate, unqualified branch. Registration alone is not inference readiness. See [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137).

Embedding in a host inherits host authority; GG-CORE's daemon containment differs by OS and cannot be represented as universal filesystem/network isolation. No HTTP, remote model download, cloud routing or business/tool execution is added by this plan.
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

**Python bindings**: on `pyo3` 0.29 + `pyo3-async-runtimes` 0.29 (MSRV 1.83,
satisfied by CI stable); RUSTSEC-2026-0176/0177/2025-0020 are cleared.

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

**ONNX classifier**: `engine/onnx/classifier.rs` now performs real candle-onnx
inference producing a `ClassificationResult` (no longer a fail-loud stub),
mirroring the ONNX embedder. Real coverage is the `logits_to_classification`
unit tests. A real subword tokenizer and manifest-driven classifier-vs-embedder
dispatch remain follow-ups (`docs/BACKLOG.md` B-28/B-29).

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
