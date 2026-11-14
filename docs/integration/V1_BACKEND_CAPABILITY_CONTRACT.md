# v1 backend capability contract: truth before selection

**Status:** isolated source foundation for [#49](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/49), **not complete IPC negotiation** or a shipping v1 support table. Based on `main@1032d1ba8a229b6690209d4661cba6706a721ea7`. User-approved sequence: switching → capabilities/hardware selection → standalone model I/O → measured GPU speculative decoding. No CodeQL/Rust/real-model suite ran in the GitHub-only authoring environment.

## Exact additive source change

`core-runtime/src/models/backend_capabilities.rs` defines serializable, versioned, typed metadata and `compiled_backend_capabilities()`.

The function exposes **compiled** GGUF text generation and, when the `onnx` feature is present, ONNX embedding/classification implementations. It emits only `Cpu` as a real execution target in this source baseline: GPU modules detect devices or have mock/unimplemented execution, but have no qualified serving offload path. It does **not** invent ONNX text generation, BitNet execution, or installed-model compatibility. All `qualified_tasks` are empty because only a separate, exact-artifact acceptance matrix can confer shipped support.

A compiled library method, a source test, a real model operation through authenticated IPC, and a release-qualified installed product are **four distinct evidence levels**. This manifest deliberately records only the first, and never asserts the others.

Unit tests assert:
- no CUDA/Metal or qualified task is falsely advertised;
- no unsupported ONNX generation or BitNet;
- advertised compiled backend set matches Cargo feature flags;
- schema v1 round-trips in JSON without invented accelerators.

## Integration design, not yet built

1. Preserve the established `IpcMessage` source-compatibility decision and existing `models_request` response. Do **not** append required public fields to `ModelInfo`/`ModelsListResponse` and silently break external Rust struct literals.
2. Add a version-negotiated, authenticated backend-capabilities request/response as a separate typed message after independent contract review (#112/#171). A capable caller requests compiled capabilities and separate **per-model actual task/backend** capabilities; an old caller remains on existing V1 behavior.
3. The returned runtime snapshot must additionally include **verified installed model ID/handle, format, model manifest/hash, active backend version, actual task support, optional tested quantization modes, actual context/VRAM limits and authority requirements**. Unknown fields remain absent/unknown, not defaulted to supported.
4. Hardware profile (#50) and selection policy consume only validated local available resources (including measured available VRAM) and owner/operator constraints. Never select a source-only, unqualified or non-admitted backend; honor explicit model pin and offline policy. A denied route must not turn into remote fallback.
5. Model switching (#137/#153) executes through the canonical lifecycle transition owner, not `ModelPool::switch_to` alone. Every routing decision has reason, retry policy, measured model-readiness and bounded peak memory.
6. Qualify native tests under default, `gguf`, `onnx`, `advanced`, `ffi`, and combinations `onnx,gguf,ffi`, plus independent Rust API compatibility. Tests of the IPC mapping need authenticated and unauthenticated negatives and old client wire conformance.
7. Publish only independently measured support cells from real standalone Windows/Linux/macOS binaries, with licensed pinned Qwen2.5, Phi-3.5 and SmolLM2 GGUF plus real ONNX embeddings/classification. CUDA and Metal entries are added only after real serving inference, cancellation, unload, VRAM/accounting and signed package qualification.

## Dependency frontiers

- **#137/#153:** owner-accepted transition semantics and actual A→B→A in the daemon. The switching safety prototype is on independent branch `feat/v1-cold-switch-policy-20261113-no-ci`.
- **#49:** current typed compile-time catalog is source-only. IPC exposure and per-model published compatibility still pending.
- **#50:** local hardware profiling, selection scoring, bounded hysteresis and no-unapproved escalation are next.
- **#171:** ONNX typed embedding/classification serving needs installed evidence before declaration as supported.
- **GPU/speculation:** advance only after native CUDA/Metal is real. A Cargo feature or device-detection result does not establish inference acceleration.

No HTTP, network listener, cloud fallback, signing or release action is authorized by this work.
