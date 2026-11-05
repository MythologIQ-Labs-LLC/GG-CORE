# GG-CORE Roadmap

**Qualification checkpoint: 2026-11-05.** `0.10.0-rc.1` at `main@d55ba50a` passed Rust, CodeQL and packaged Linux/Windows/macOS GGUF inference. **As of this checkpoint, no public tag existed**; see [Releases](https://github.com/MythologIQ-Labs-LLC/GG-CORE/releases) for current publication status. Owner-controlled publication [#130](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/130), performance [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136) and switching [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137) remain distinct.
**Product:** GG-CORE (Greatest Good - Contained Offline Restricted Execution) is a local CPU inference runtime with authenticated IPC, not a plugin or external inference router. Security containment varies by platform; see `SECURITY.md`.

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
- **Platform containment (serving daemon):** Linux seccomp is enabled before accepting IPC. Windows Job Objects limit process/resources but not filesystem or network authority; macOS has no daemon OS sandbox. This behavior is **wired and packaged-qualified**, not library-only. See `SECURITY.md` and #130.

## Library-only (exists, tested, NOT wired into the daemon)

These were previously listed as "complete"; that overstated them:
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
1. **Publish the qualified RC:** reconcile final docs, requalify any new source SHA, verify private vulnerability reporting, obtain owner GO, then publish `v0.10.0-rc.1` and validate downloaded artifacts. See #130.
2. **Keep dependency advisories current:** preserve the Cargo lockfile, cargo-deny policy and weekly Dependabot checks. The `pin-utils` advisory was resolved by #141; do not suppress a later feed failure without risk review.
3. **MSRV/toolchain reproducibility:** `Cargo.toml` declares Rust 1.97; CI uses floating stable. Only change this in a separately qualified maintenance PR if drift creates a repeatable problem.
4. **Performance evidence (#136):** real packaged model throughput, latency, memory, and security overhead on comparable hosts before making claims.
5. **Embedded consumer contract (#112–#114):** stable API, exact-version distribution and downstream conformance are future milestones, not silent assurances of this RC.
6. **Adaptive model switching (#137):** real model loading, memory reclamation and active-request correctness; not required by one-resident-model RC.

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
| **0.10.0-rc.1** | **Qualified candidate; unpublished** | CPU GGUF, authenticated local IPC, scoped platform confinement and real packaged inference; exact final tag requires #130 owner GO |
| 1.0.0 | Planned | Stable public API, verified platform security boundaries, repeatable product acceptance and independent audit plan |

## Contributing

See `CLA.md`. The immediate priority is the release gate above, not expanding supported features. Later GPU/adapter work, API distribution and examples belong in separately reviewed milestones.

---

Copyright 2024-2026 GG-CORE Contributors
Licensed under the Apache License, Version 2.0
