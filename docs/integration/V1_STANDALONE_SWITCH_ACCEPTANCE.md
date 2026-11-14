# GG-CORE v1: standalone model-switch contract and qualification frontier

**Status:** implementation-stage safety prototype, **NOT** a production model-switch feature or v1.0.0 RC qualification. Owner approved the priority sequence (model switching → backend capability negotiation → standalone model I/O → GPU-qualified speculative decoding), but this is **not** an owner decision on unspecified transition/eviction/rollback policy in [#153](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/153).

**Baseline:** protected `main@1032d1ba8a229b6690209d4661cba6706a721ea7` includes combined-tree-qualified #195 deterministic prefill, #193 strict admission budgets, and #183 held-instance release tracking. The published `v0.10.0-rc.1` is immutable. No CoreForge work belongs in this lane.

## 1. Existing production primitives, and what they do NOT prove

- Authenticated IPC `model_load_request` and `model_unload_request` route through `ipc/handler_models.rs` and `models/model_ops.rs` to `ModelLifecycle`. The canonical loader validates base-path confinement, optional manifest/integrity, and backend dispatch before registering.
- Resident memory is budgeted before backend allocation. The held-instance release tracker exposes truthful `unloading` state and `model_release_wait_request`; `released:false` cannot be counted as actual release, and the CLI's bounded wait exits **4** if an instance remains.
- `ModelPool::switch_to` only reassigns an **already-prepared handle**; `SwapManager::execute_swap` operates on `ModelRouter`/`ModelRegistry` and does **not** implement the daemon's complete `ModelLifecycle` + backend resource admission + authenticated IPC contract. Neither should be advertised as real serving-path A→B→A switching.
- No dedicated authenticated **atomic model-switch request** currently owns the transition, rollback policy, native release, concurrent normal load/unload and request-specific authorization. A manual unload/wait/load CLI sequence is useful for diagnosis but is **not** an atomic production switch.

## 2. Candidate cold-switch semantics for owner disposition

For the *first* serving-path vertical, favor a bounded **cold, non-overlapping, request-boundary** transition to keep peak resident memory within `GG_CORE_MAX_TOTAL_MEMORY`. Do not evict an in-flight/pinned model just because another request suggests a higher tier; do not copy KV/cache or unfinished generation state across model families.

```text
Idle(A ready)
   │ request: A→B (auth, expected A handle, allowed B, validated relative B path)
   ▼
Admission (concurrency lease, identity, source/target tasks, budget policy)
   ├─ invalid / stale / unauthorized → Refused; A unchanged
   ├─ active request lease → Drain or Refuse (timeout policy awaiting owner decision)
   ▼
Draining(A) → Unloading(A) → Waiting for actual release
   ├─ timeout → Explicit Pending/Unready; never count source memory as free
   ▼
Load/verify B via the canonical admitted loader (no bypass)
   ├─ B fails → Explicit failure, do not claim success; bounded rollback
   │           to A only if original model source is independently validated
   │           and the owner-approved rollback policy permits it
   ▼
Ready(B), with new handle and truthful readiness
   │  later B→A follows an independently admitted transition
   ▼
Ready(A'), with a new never-reused handle
```

**Unresolved owner-dependent choices:** when to refuse versus drain active work; maximum drain/release budget; optional prewarming when A+B fits; whether and under which verified original-path conditions automatic rollback to A is mandatory or only best-effort; request ordering under multiple independently authenticated operators; protected pinned models; cancellation semantics for initiated switches. Do not silently choose these as shipped policy.

**Non-negotiable invariants regardless of choices:**
1. Model-transition admission is authenticated and scoped to the requesting local operator, with no new HTTP, arbitrary path or egress authority.
2. Switching never bypasses canonical model path/hash/manifest validation, engine + registry atomicity or resident/working memory reservations.
3. No new model is advertised ready before real load and capability verification; on failure, error state is truthful and no model-identity or response misattribution occurs.
4. In-flight A work is not silently served by B. The old instance remains charged until the last actual holder releases it.
5. A release timeout is a transition failure/pending condition, not authorization to load past the limit.
6. Every source/target handle ID is unique; retries cannot turn stale handles into a different model. Switching is serialized against other model-mutating operations.
7. Failure/disconnect/cancellation always has bounded cleanup and an auditable terminal state. A rollback is **verified**, not inferred from a CLI exit code.
8. Model readiness, loaded-model listing, peak resident RSS, mmap handles and (where relevant) actual VRAM must agree with the transition state.

## 3. Proof already implemented on the isolated branch

`core-runtime/src/models/lifecycle_transition_tests.rs` is a **model-free regression prototype over the real lifecycle**. It covers:
- A held 700-byte A instance in a 1,000-byte budget preventing a 500-byte B admission until A's final holder drops, then full A→B→A with unique handle identities.
- An attempted overlapping 800-byte A + 300-byte B warm load refusing without evicting A.
- A timed-out release wait keeping memory charged and preventing a 400-byte B load.

These tests **have not been compiled or executed in this GitHub-only session**. They do not prove actual GGUF/ONNX model loads, daemon orchestration, OS RSS or VRAM, or rollback after a failed model-load.

## 4. Next implementation steps (gated)

1. Run native Rust 1.99.1 fmt, Clippy `-D warnings` (default, advanced, gguf, onnx, ffi) and default/advanced/full tests on this exact branch. Independent review before protected-main integration; do not spend paid GitHub Actions without separate permission.
2. Obtain #153's specific cold/warm/drain/rollback authority contract. Design a **single** transition lease shared by every authenticated mutating model operation, including regular `load`/`unload`, to prevent cross-connection races. Use explicit expected-handle preconditions and bounded cancellation.
3. Implement the transition in the real serving path using `ModelLifecycle` and canonical `model_ops`, plus an additive or explicitly versioned authenticated IPC/CLI contract. Preserve old clients and public Rust exhaustiveness commitments (#112).
4. Exercise real license/hash-pinned models: Qwen2.5 → Phi-3.5 → Qwen2.5, Qwen2.5 → SmolLM2 → Qwen2.5, and GGUF↔ONNX embedding/classification once #171 typed serving is qualified. Include active streams, wrong auth/paths/model IDs, 1/4/8 clients, insufficient memory, wrong backend, timeout, disconnect and failed-load rollback.
5. Capture **measured** RSS/mmap/VRAM, working + resident reservations, p50/p95/p99 latency and time-to-ready, actual task output, model/hash, exact installed binary and OS. No fake model or check-only path may establish a production support cell.
6. Only after real switching is proved, allow #49 backend/model capabilities and #50 hardware-aware selection to invoke it under host-approved policy; no autonomous remote fallback, thrash or mid-token cross-model handoff.

**Standalone definition of DONE:** a clean installed GG-CORE distribution, without CoreForge, can perform an authenticated, controlled A→B→A and GGUF↔ONNX task change, preserving security/quality, actually releasing memory, refusing unsafe overlap, reporting truthful failures and recovering on a pinned model/OS/hardware matrix. No version tag or GO implied.
