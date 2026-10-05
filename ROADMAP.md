# GG-CORE Roadmap

**Last updated:** 2026-10-05 · **Current version:** 0.9.0
**Product:** GG-CORE (Greatest Good - Contained Offline Restricted Execution) —
a sandboxed, offline, IPC-only inference runtime. Pure compute; no network,
no business logic, no decision authority.

This roadmap is rebuilt from code, green CI, `docs/FEATURE_INDEX.md`,
`docs/BACKLOG.md`, CHANGELOG, and open issues (per issue #107). Each item
carries its real maturity. Four maturity levels are used:

- **Shipped** — on `main`, exercised by CI.
- **Shipped (conditional)** — real, but gated on a feature flag, platform,
  or local model availability.
- **Library-only** — implemented and tested as code, not yet wired into the
  serving daemon.
- **Planned** — tracked work, not yet built.

---

## Shipped (v0.9.0 line)

### Core runtime
- Secure inference façade: `Runtime::infer` / `infer_stream` is the sole
  external inference path (ingress injection scan → inference → egress PII
  sanitization); the raw engine is `pub(crate)`.
- IPC server (Unix sockets / Windows named pipes), length-prefixed framing,
  authenticated sessions, connection pooling, graceful drain.
- Standalone model bootstrap (issue #106): `serve --model <path>
  [--model-id <id>]` preload (fail-loud), `GG_CORE_PRELOAD_MODELS` env,
  authenticated IPC `model_load/unload`, CLI `models load/unload/list
  [--json]`, readiness gated on a loaded model.
- Scheduler: priority queue + batching + cancellation; degraded-mode policy
  (context reduction under resource pressure instead of hard failure).
- Memory governance: pool, per-call/total/concurrency limits, prompt cache
  (O(n) prefix hashing).
- Model lifecycle: atomic load/unload via `ModelLifecycle`, registry with
  versioning/search/persistence, manifest-driven backend dispatch.

### Backends
- **GGUF (feature `gguf`)**: llama-cpp-2-backed generation + streaming, CPU;
  evidence model: Qwen2.5-0.5B-Instruct Q4_K_M (fixture-gated e2e).
- **ONNX (feature `onnx`)**: real embedding inference (masked mean pooling,
  L2-norm, batch) + classifier via candle-onnx; WordPiece tokenizer offline;
  served end-to-end through manifest dispatch.

### Security (serving path)
- Prompt-injection detection/blocking (55+5+5 patterns, zero-width strip).
- PII detection/redaction (13 types, NFKC); streaming egress sanitizer with
  holdback (raw tokens never leave the runtime).
- Path confinement for model loads; NUL/traversal rejection.
- 85-test penetration suite (auth, boundaries, crypto, IPC fuzzing) in CI.
- See `SECURITY.md` for the full honest maturity table, including what is
  library-only.

### Consumable surfaces
- Embedded Rust (`gg-core` crate), C FFI (cdylib + generated `gg_core.h`),
  Python (PyO3 0.29, abi3), raw IPC protocol, standalone daemon + CLI.

### Engineering infrastructure
- CI: fmt + clippy `-D warnings` on 3 OSes; feature matrix (gguf/onnx/ffi/
  python/advanced); full test suite on 3 OSes; CI-safe bench set with a
  run-over-run perf regression gate; CodeQL; `cargo-deny` supply-chain gate;
  Dependabot (grouped RustCrypto/candle updates).
- Release pipeline: tag-triggered verify → 3-platform artifact build
  (binary + cdylib + header, SHA-256 checksums) → GitHub Release with
  CHANGELOG notes. v0.9.0 is the first release published through it.
- Governance: QoreLogic Merkle-chained ledger, FEATURE_INDEX (63+ verified
  rows), reconciled BACKLOG.

### Advanced (feature `advanced`, off by default)
- Adaptive speculative decoding (sole executor — the earlier v1/v2
  implementations are retired), KV-cache reuse across draft/verify steps,
  prompt-lookup drafting, speculative telemetry in `status`. Correctness is
  proven token-identical to single-model greedy. **Wall-clock speedup is a
  GPU/batch phenomenon and is not demonstrable on CPU** — the >1× demo is
  deferred to a GPU host (B-21e).

## Shipped (conditional) — read the caveats

- **Platform support**: Linux/macOS/Windows are CI-verified for build, lint,
  and tests. No Docker image or Kubernetes deployment is CI-verified.
- **Model compatibility**: claims are limited to the models actually
  exercised (Qwen2.5-0.5B GGUF; all-MiniLM-L6-v2 ONNX embeddings). Other
  models are *expected* to work via llama.cpp/candle but are untested here.

## Library-only (exists, tested, NOT wired into the daemon)

These were previously listed as "complete"; that overstated them:

- **Sandbox** (Job Objects / cgroups+seccomp, 49-syscall allowlist): the
  daemon never calls `create_sandbox`. Wiring it into `serve` startup is a
  tracked known gap (SECURITY.md).
- **Model encryption** (AES-256-GCM stack): the load path never decrypts.
- **Paged KV attention** (`memory/paged.rs`): no live caller on the serving
  path.
- **MoE** (router/combiner/executor), **A/B testing** (traffic splitting,
  variant metrics), **deployment automation** (canary; blue-green exists
  only in tests): libraries without production call sites.
- **SIMD tokenizer v2**: compiled under `advanced`, unused by the pipeline.

## Explicitly not real yet (previously claimed complete — corrected)

- **GPU execution**: CUDA/Metal modules do device *detection* only. GPU
  memory allocators are bookkeeping mocks (TODOs in `gpu_allocator.rs`);
  flash-attention kernels return fail-loud "not implemented"; multi-GPU
  paths are simulated; the GGUF backend runs CPU-only (`n_gpu_layers: 0`).
- **llama.cpp comparative benchmarks / GPU-vs-CPU benchmarks**: do not
  exist (the GPU bench exercises the mock allocator).

## Planned

### Near-term (owned, sequenced)
1. **Daemon sandbox activation** — apply the shipped sandbox in `serve`.
2. **Model hash enforcement** — verify manifest `sha256` at load, fail loud.
3. **MSRV / toolchain pin** — CI currently floats `stable` (new rustc 1.99
   lints landed mid-PR and broke green).
4. **Dependency currency wave** (each its own PR): RustCrypto generation
   (sha2/aes/aes-gcm/pbkdf2), candle 0.8 → 0.11 + tokenizers 0.23,
   llama-cpp-2 → latest 0.1.x (FP4 quants, upstream speculative rework),
   thiserror 2 / toml 1 / rand 0.10 / metrics 0.24; abi3 floor → py310.
5. **CLI `infer` authentication** — the authenticated exchange shipped for
   `models load/unload`; route `infer` through it.
6. **Fixture-gated standalone e2e smoke in CI** (needs a small committed or
   cached GGUF fixture) — closes the last #106 acceptance box.

### Backend capability epic (issues #48–#52)
ADR-first: backend capability contract, `RuntimeBackendCapabilities` schema,
hardware profile + selection policy, experimental BitNet adapter. The 2026
Rust inference ecosystem (candle 0.11, mistral.rs paged attention + FP8 KV
cache, Burn-LM) validates this abstraction; design against it.

### Competitive features (post-currency)
- Structured output via **llguidance** (pure Rust, offline — fits the
  sandbox constraint).
- MTP-aware / prompt-lookup drafting improvements; B-21e GPU speculative
  benchmark.
- imatrix-aware GGUF metadata handling in the registry.

### Post-traction
- Independent security audit; SOC 2; FIPS validation (today: power-on
  self-tests only, no certification). Multi-tenant isolation (GG-CORE
  Nexus shim).

## Release history (real)

| Version | Date | Notes |
| --- | --- | --- |
| **0.9.0** | 2026-10-05 | Standalone bootstrap; security suite activation; supply-chain gate; first pipeline-published release |
| 0.8.2 | 2026-07-27 | Security & dependency hardening (tagged; predates the release pipeline) |
| 0.8.0/0.8.1 | 2026-02 | GG-CORE rebrand (from Veritas SPARK), hardening |
| 0.7.0 | 2026-02-19 | Streaming inference (tagged) |
| ≤ 0.6.x | 2026-02 | Early development line (0.6.5 tagged) |
| 1.0.0 | Planned | Production stable: sandbox-wired daemon, hash enforcement, GPU story decided, independent audit scheduled |

## Contributing

See `CLA.md`. Priority areas: the Near-term list above, GPU execution
(behind the capability contract), examples/tutorials, and edge-case tests.

---

Copyright 2024-2026 GG-CORE Contributors
Licensed under the Apache License, Version 2.0
