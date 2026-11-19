# v1 backend capability contract: truth before selection

**Status:** isolated source foundation for [#49](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/49), **not complete IPC negotiation** or a shipping v1 support table. Based on `main@1032d1ba8a229b6690209d4661cba6706a721ea7`. User-approved sequence: switching → capabilities/hardware selection → standalone model I/O → measured GPU speculative decoding. No CodeQL/Rust/real-model suite ran in the GitHub-only authoring environment.

## Exact additive source change

`core-runtime/src/models/backend_capabilities.rs` defines serializable, versioned, typed metadata and `compiled_backend_capabilities()`.

The function exposes **compiled** GGUF text generation and, when the `onnx` feature is present, ONNX embedding/classification implementations. It emits only `Cpu` as a real execution target in this source baseline: GPU modules detect devices or have mock/unimplemented execution, but have no qualified serving offload path. It does **not** invent ONNX text generation, BitNet execution, or installed-model compatibility. All `qualified_paths` are empty because only a separate, exact-artifact acceptance matrix can confer shipped support.

A compiled library method, a source test, a real model operation through authenticated IPC, and a release-qualified installed product are **four distinct evidence levels**. This manifest deliberately records only the first, and never asserts the others.

Unit tests assert:
- no CUDA/Metal or qualified task is falsely advertised;
- no unsupported ONNX generation or BitNet;
- advertised compiled backend set matches Cargo feature flags;
- schema v1 round-trips in JSON without invented accelerators; selection rejects schema drift and refuses GPU execution when only a CPU task has qualified, even if a future build has a GPU-compiled path.

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

## Selection prototype (source-only, non-serving)

`core-runtime/src/models/backend_selection.rs` provides `select_qualified_backend`, a fail-closed **eligibility check**, not runtime routing. It requires an exact `ServingPath { task, target }` in **both** compiled and independently qualified paths, exact schema version, optional explicit backend pin, and streaming support. No presumed Cartesian product between model tasks and accelerators. Unknown qualification, unsupported target, unqualified streaming, backend pin mismatch, ambiguous eligible candidates, or schema drift returns a refusal. Tests use **synthetic qualified entries** to verify policy behavior; such entries are NOT installed qualification evidence.

Before calling this from the daemon, the authenticated controller must obtain an evidence-bound **per-model** support snapshot, including model/hash/format/task/hardware and current reservations, then enforce its local host policy. The current static catalog intentionally qualifies **no** cells and therefore selects nothing. This keeps incomplete capabilities from being advertised as usable or auto-selected.

**Security note:** `qualified_paths` is currently an ordinary publicly constructible Rust field. It is not cryptographically attested. Never accept client-supplied `RuntimeBackendCapabilities` as a qualification authority; the eventual IPC response must be created from a trusted server-side registry of reviewed, exact-model acceptance receipts. A malicious caller can otherwise fabricate an eligible cell.

## Deferred: memory budgets, host authority and model selection (#50 / #153)
- **Scope of backend eligibility:** `backend_eligibility::evaluate_backends` and `select_qualified_backend` decide *backend* eligibility only. They do not know a model's resident or working size, choose a model, or change any budget or authority.
- **Deferred to model selection:** checking a candidate model against the memory budget, host-authority rules (pinning a model rather than a backend, operator constraints) and model selection itself. These belong to the model-selection policy under #50 / #153 and are not implemented.
- **In the meantime:** the existing admission and resident-budget checks (#193) apply unchanged to every load.


## 2026-11-17 model-level policy candidate, PR #218

The previous **backend-only** selector is not the full Greatest Good model-selection policy. [PR #218](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/218) adds `core-runtime/src/models/model_selection_policy.rs` and bounded companion modules as a **pure, non-executing** model-level decision engine, pending full review and CI acceptance.

Inputs are explicit host model allowlist/pin, task, target, streaming and quality floor, budgets, current model, switch/reroute permission, cooldown and measured candidate-specific quality, individual inference/cold-start p95 components, resident/working memory, custody and readiness evidence. Outputs are a traceable reuse, resident reroute proposal, cold-switch proposal or refusal. A candidate's backend must have the exact compiled task+target path; missing or contradictory evidence fails closed. Deterministic tie refusal and hysteresis prevent arbitrary preference and model thrashing. **No performance thresholds are hardcoded or invented.**

This change **does not** create a trusted evidence registry; public Rust candidate values are not proof. No production controller consumes this policy yet, and the qualified runtime snapshot still has no trusted installed-model path registry. A future authenticated controller must bind evidence to exact model and binary identities, locally observed memory/hardware and serving readiness; enforce the operator authorization and final admission constraints; and request actual transition only through #137/#153 canonical lifecycle. This source work is not a switch, hardware support claim, qualified benchmark, standalone release, or replacement for native GPU/real-model acceptance. The older documentation above remains the historical prior state where it describes model selection as deferred.

**Latency evidence semantics:** summing the independently measured p95 inference and p95 cold-load components yields an additive planning estimate, **not** a measured end-to-end p95 or probability guarantee. The field is named `estimated_total_ms` and compared with a host-supplied `max_estimated_total_ms`. End-to-end p95 must be measured independently under #136 before any benchmark/support claim.

## Native accelerator compile-path candidate (PR #216, not release-qualified)

The historical CPU-only statements above describe the earlier `main` snapshot. Draft [PR #216](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/216) adds optional `gguf-cuda` and `gguf-metal` features that compile llama.cpp **inference backends**, unlike the older `cuda`/`metal` device-management features. The daemon forwards an explicit `GG_CORE_N_GPU_LAYERS` count to the native GGUF model loader; zero remains the CPU default. A nonzero count on an unsupported build/platform is refused instead of silently ignored.

`compiled_backend_capabilities()` reports a **GGUF text-generation/CUDA** compile path only when `gguf-cuda` is enabled and a **GGUF text-generation/Metal** compile path only under `gguf-metal` on macOS. It always reports GGUF CPU generation when GGUF is compiled and does not fabricate ONNX GPU execution. Its `qualified_paths` stays **empty** for every build, including GPU-enabled builds.

The compiled path is a capability of the binary's *code graph*, **not** evidence of a present NVIDIA/Apple GPU, compatible driver, successful kernel execution, installed-model offload, or release support. The runtime's `hardware.accelerators` remains unknown until measured, and the loaded-model snapshot does not claim GPU use from a configured layer count. Native CUDA/Metal acceptance must record model/binary hashes, GPU utilization, VRAM, correct output, streaming, cancel/drop, memory reclamation, fallback/refusal and standalone packaging. In particular P0 [#219](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/219) must be resolved before any stream/offload release claim.

Any consumer may inspect compiled paths, but a selector still must refuse a target with no **independently verified model+hardware+task** support evidence, irrespective of the compiled feature flag. No global qualification or model-routing approval is implied by this PR.
