# README Verification Record

**Repository:** `MythologIQ-Labs-LLC/GG-CORE`  
**Audit date:** 2026-08-01  
**Baseline:** `main` at `d81296196471eae6dbfa629c009f81b94a49c690`  
**README target:** platinum upgrade with standalone and consumer guidance

## Purpose

This record substantiates the public README without treating missing or incomplete evidence as proof that a product expectation is false. Each material claim is classified as verified, conditional, experimental, planned, or rejected.

The intended behavior is:

- preserve legitimate product ambition;
- distinguish implementation from deployment assurance;
- link incomplete expectations to owned work;
- avoid fixed badges and marketing numbers that silently rot;
- prefer current code and exact-head CI over older narrative documents.

## Source hierarchy

The audit used this authority order:

1. Current source code and `core-runtime/Cargo.toml`.
2. CI workflow and exact-head merged change evidence.
3. Direct source-to-test bindings in `docs/FEATURE_INDEX.md`, followed by deep verification of load-bearing claims.
4. `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` and accepted ADRs.
5. `CHANGELOG.md` for release chronology.
6. Reproducible benchmark documentation with model, build, and hardware metadata.
7. Older narrative documents only after reconciliation against the sources above.

## Files and surfaces inspected

- `README.md`
- `core-runtime/Cargo.toml`
- `core-runtime/src/lib.rs`
- `core-runtime/src/runtime_facade.rs`
- `core-runtime/src/main.rs`
- `core-runtime/src/runtime_init.rs`
- `core-runtime/src/cli_parser.rs`
- `core-runtime/src/config.rs`
- `core-runtime/src/ipc/protocol_types.rs`
- `core-runtime/src/models/backend_dispatch.rs`
- `core-runtime/src/models/manifest.rs`
- `core-runtime/src/engine/onnx/`
- `core-runtime/src/engine/adaptive_speculative/`
- `core-runtime/src/engine/gguf/speculative_session.rs`
- `core-runtime/src/ffi/` and `core-runtime/include/gg_core.h`
- `core-runtime/src/python/`
- `.github/workflows/rust.yml`
- `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md`
- `docs/FEATURE_INDEX.md`
- `docs/BENCHMARKS.md`
- `CHANGELOG.md`
- `ROADMAP.md`
- `SECURITY.md`
- `docs/USAGE_GUIDE.md`
- Open issues and recent merged/open pull requests relevant to current capability state

## Claim decisions

| Claim area | Decision | Evidence and qualification |
| --- | --- | --- |
| Product identity | **Verified** | Canonical name is `Greatest Good - Contained Offline Restricted Execution`; current crate description and architecture agree. Older `Secure Performance-Accelerated Runtime Kernel` references are legacy drift. |
| Release version | **Verified** | `core-runtime/Cargo.toml` and release changelog identify `0.8.2`. Unreleased work exists on `main`. |
| Pre-production status | **Verified** | The project has real runtime and test surfaces but known first-run, documentation, packaging, hardware-acceptance, audit, and certification gaps. |
| No HTTP/REST/WebSocket inference surface | **Verified** | The runtime uses Unix sockets / named pipes and framed JSON IPC; forbidden dependency policy excludes HTTP and WebSocket server stacks. |
| Physically incapable of network access | **Reclassified: conditional deployment claim** | The crate does not expose a network inference service, but strong deny-all/air-gap assurance requires OS/container/VM policy and deployment verification. `tokio` also enables `net` capabilities needed by local transport internals. |
| Secure façade | **Verified** | `Runtime::infer` / `infer_stream` wrap ingress scan, engine execution, and egress sanitization. Direct engine run methods are crate-private. |
| GGUF execution | **Verified + conditional** | `llama-cpp-2` backend and real-model E2E evidence exist. Qwen 2.5 0.5B Q4_K_M is the documented verified model baseline. Broader GGUF families are backend expectations, not blanket compatibility certification. |
| ONNX support | **Verified + conditional** | Real Candle ONNX embedder/classifier code exists. Manifest-driven dispatch reaches FFI/Python load paths. Requires `onnx`, CPU execution, a supported manifest capability, and local tokenizer/model artifacts. |
| Standalone daemon | **Verified + conditional** | Server, authenticated IPC, scheduler worker, probes, status, inference, streaming, cancellation, metrics, and model listing exist. The daemon starts with an empty model registry. |
| Complete standalone first-run inference | **Planned** | CLI/startup model preload and load/unload are not implemented. Detailed completion contract filed as issue #106. |
| Rust embedding | **Verified** | `Runtime`, model loader, backend dispatch, lifecycle, and secure inference façade are public and used by downstream embedding work. |
| C FFI | **Verified + conditional** | `cdylib`, generated header, model lifecycle, authentication, inference, streaming, health, and typed error codes exist. Dedicated CI feature leg runs. |
| Python binding | **Verified + conditional** | PyO3 runtime/session/model lifecycle/inference code exists and a dedicated CI feature leg runs. Published wheel/developer packaging is not yet a verified release surface. |
| Fixed total test count | **Removed as a badge, not as evidence** | Counts vary by feature and commit. Dynamic CI and feature-index bindings are more durable evidence than a manually maintained number. |
| `A+ 98/100` security rating | **Not promoted to README** | It is an internal/self-assigned score in stale documentation, not an independent audit or certification. The underlying controls remain documented individually. |
| FIPS 140-3 | **Reclassified: readiness controls, not certification** | Power-on self-tests and FIPS-oriented cryptographic controls exist. No certification claim is made. |
| Rust memory safety | **Narrowed** | Rust protects the broad safe-code surface. `unsafe` remains at FFI, native backend, cryptographic, and platform boundaries, with an unsafe audit document. |
| Single binary / no dependencies | **Narrowed** | A CLI/daemon binary is produced, but build and runtime linkage vary by selected native backend, platform, and packaging choices. |
| 40 tokens/sec baseline | **Retained with exact scope** | Historical release benchmark records Qwen 2.5 0.5B Q4_K_M on an i7-7700K Windows host at approximately 40 tokens/sec. No extrapolated hardware promise is made. |
| Infrastructure multiplier advantage over other runtimes | **Not promoted** | Existing comparisons mix control-path measurements and external runtime assumptions. GG-CORE's differentiated boundary is described without claiming model-kernel superiority. |
| Speculative decoding speedup | **Reclassified: experimental performance** | Adaptive speculation, persistent target KV reuse, prompt-lookup draft support, and model-pair wiring are merged behind `advanced` and off by default. Correctness and rollback are tested, but the merged PR #105 records the CPU path as slower than single-model inference; a general speedup still requires hardware-specific evidence, especially the GPU benchmark work tracked as B-21e. |
| CUDA/Metal/multi-GPU | **Experimental + conditional** | Feature-gated implementations and test bindings exist. Default CI does not provide complete hardware-specific end-to-end acceptance. |
| Commercial advanced components | **Clarified** | The repository is Apache 2.0 licensed. `advanced` is a feature group in this source tree; commercial extension policy belongs to separate product packaging such as the shim/Nexus boundary, not an unsupported README licensing claim. |
| Model encryption, prompt scanning, PII sanitization, audit/telemetry | **Retained** | Source and test bindings exist. Deployment and threat-model caveats are explicit. |
| Independent security assurance | **Planned** | Internal adversarial testing and governance evidence exist; independent audit, certification, and regulated-environment acceptance remain future work. |

## Standalone verification finding

The current binary provides a legitimate standalone process surface:

```text
serve
health / live / ready
status
infer / streaming
models list
config
```

However, `run_serve` constructs an empty runtime, the IPC message enum has no model load/unload request, and the CLI `models` implementation supports only `list`. This means the existing `infer` command needs a model registered through another consumer path.

This is treated as a product-completion gap rather than a reason to delete standalone positioning. Issue [#106](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/106) defines startup preload, authenticated model lifecycle IPC, readiness semantics, security constraints, and a command-driven acceptance test.

## Documentation drift finding

`SECURITY.md`, `ROADMAP.md`, and `docs/USAGE_GUIDE.md` contain material version, API, support, feature-flag, contact, compatibility, and assurance drift. The README does not silently inherit those claims. Reconciliation is owned by issue [#107](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/107).

Until #107 is resolved:

- current code and exact-head CI win;
- the feature index is the primary subsystem evidence map;
- the living architecture document is the primary narrative architecture source;
- older examples must be compiled or manually reconciled before reuse;
- placeholder contact or certification language must not be repeated as fact.

## README acceptance checklist

- [x] Clear product identity and trust boundary.
- [x] Dynamic CI badge instead of a fixed passing-test count.
- [x] Release, license, and pre-production status visible above the fold.
- [x] Architecture diagram.
- [x] Separate standalone, Rust, C, Python, and raw IPC paths.
- [x] Feature-specific build commands and prerequisites.
- [x] Current standalone limitation stated without deleting the product expectation.
- [x] Owned standalone completion issue linked.
- [x] Accurate Rust embedding example using loader, lifecycle, and secure façade.
- [x] ONNX manifest example and dispatch rules.
- [x] Security controls separated from deployment responsibilities.
- [x] FIPS self-tests separated from certification.
- [x] Reproducible performance baseline separated from estimates and comparisons.
- [x] Advanced/speculative performance claim bounded to current evidence.
- [x] Source/test evidence and governed architecture linked.
- [x] Documentation drift disclosed and assigned rather than hidden.

## Maintenance rule

Any pull request that changes a README-level capability must update at least one of:

- the README maturity table;
- this verification record;
- `docs/FEATURE_INDEX.md`;
- `CHANGELOG.md`;
- the architecture or threat model;
- an owned issue that closes the remaining expectation.

A claim must not be downgraded from verified to vague marketing, nor deleted merely because its evidence needs completion. The correct response is to verify it, qualify it, or assign the work required to make it true.
