# Research Brief — B-07: Degraded-Mode Policy for Constrained Local Inference

**Date**: 2026-07-28
**Analyst**: The Qor-logic Analyst
**Target**: B-07 (issue #53, P2) — define GG-CORE's governance + runtime behavior under
resource pressure: intentional, explainable degradation instead of hard failure.
**Scope**: the resource-pressure decision points (context, memory, capability), the
existing limits/error surfaces, and where a degraded-mode policy hooks in.

---

## Executive Summary

Degraded mode is a **policy + decision layer at the resource-pressure boundary**, not a
new backend. Today, three boundary checks fail *hard*: context over budget
(`inference.rs:130` → `ContextExceeded`), per-call/total memory over budget
(`memory/limits.rs:57` `try_acquire` → `MemoryExceeded`), and unsupported capability
(`error.rs:41` `CapabilityNotSupported`). B-07 introduces a `DegradedModePolicy` and a
**pure decision function** that maps a resource-pressure signal to an explainable
`DegradedDecision` (reduce context, reject-with-reason, disable-capability-preserve-base),
plus one concrete safe mechanism — **context reduction before hard-fail** — and a
documented hook where a future smaller/BitNet model swap plugs in. This matches the
CONCEPT triage thesis (`CONCEPT.md:9`: system stability + fair allocation over individual
request optimization). Model-swap execution is out of scope (BitNet is B-02..B-06); B-07
is the policy the swap will consume.

## Findings (verified)

### F1 — three hard-fail boundary points are the degraded-mode hook sites
- **Context**: `InferenceEngine::check_context` (`inference.rs:130`) estimates tokens
  (`prompt.len()/4`) and returns `Err(ContextExceeded { max, got })` when over
  `max_context_length` — a hard fail with no reduction attempt.
- **Memory**: `ResourceLimits::try_acquire(memory_bytes)` (`memory/limits.rs:57`) returns
  `Err(MemoryExceeded { used, limit })` when a call exceeds `max_memory_per_call` or the
  running total exceeds `max_total_memory`.
- **Capability**: `InferenceError::CapabilityNotSupported(String)` (`error.rs:41`) —
  e.g. the ONNX embedder rejects chat input; no graceful "preserve base capability" path.

### F2 — the config surface exists and is env-driven
- `ResourceLimitsConfig { max_memory_per_call, max_total_memory, max_concurrent }`
  (`memory/limits.rs:12`), defaulted (1GB/2GB/2) and loaded from env
  (`config.rs:120 load_resource_limits`). A `DegradedModeConfig` fits the same
  env-driven config pattern (`config.rs:74` composes sub-configs into `RuntimeConfig`).

### F3 — two `InferenceError` enums exist (decision must not complect them)
- `inference_types.rs:10` (`ContextExceeded { max, got }`, `MemoryExceeded { used, limit }`)
  and `engine/error.rs:9` (`MemoryExceeded`, `CapabilityNotSupported`). The degraded-mode
  decision must take a **neutral pressure signal**, not either error type directly, so the
  policy stays independent of which error surfaced it. A small `ResourcePressure` input enum
  decouples the decision from the error taxonomy.

### F4 — explainability is a first-class requirement (issue #53)
- Issue #53: "degrade intentionally and **explain the tradeoff**." Every `DegradedDecision`
  must carry a human-readable reason string; a telemetry/log surface records it. This is
  the product-thesis differentiator, not an afterthought.

### F5 — the FFI/error mapping is already layered
- `ffi/error.rs` maps `ContextExceeded`/`MemoryExceeded` → `CoreErrorCode`. A
  reject-with-explanation decision reuses this; a reduce-context decision succeeds (no new
  error code) and surfaces the explanation via telemetry, not the error channel.

## Blueprint Alignment

| Claim (issue #53 / CONCEPT) | Actual finding | Status |
|---|---|---|
| Reduce context before failing hard | `check_context` fails hard with no reduction | GAP → B-07 mechanism |
| Prefer smaller/BitNet model under memory pressure | No multi-model swap; BitNet is B-02..B-06 | DEFERRED (documented hook) |
| Disable unsupported capability, preserve chat | `CapabilityNotSupported` hard-rejects | GAP → policy decision (mechanism partial) |
| Degrade intentionally + explain | No policy/explanation layer exists | GAP → B-07 core |
| Offline-first (no cloud fallback) | Already enforced (no network; CORE constraint) | MATCH (policy asserts it) |

## Recommendations (scope forks for the plan — decide at cycle start)

1. **Bounded single cycle (L2)**: ship the **policy + pure decision + context-reduction
   mechanism + explanation**, defer model-swap execution:
   - `DegradedModePolicy` / `DegradedModeConfig` (`allow_context_reduction: bool`,
     `min_context_tokens: usize`, `reject_on_memory: bool`) — env-driven like
     `ResourceLimitsConfig`.
   - Pure, total `evaluate(&policy, ResourcePressure) -> DegradedDecision` where
     `ResourcePressure = { Context { max, got }, Memory { used, limit }, Capability { name } }`
     and `DegradedDecision = { ReduceContextTo(usize), Reject { reason }, DisableCapability
     { name, reason } }`; every arm carries an explanation string. Fully unit-testable.
   - **Mechanism**: `check_context` consults the policy — on context overflow with
     `allow_context_reduction`, return `ReduceContextTo(max)` so the engine truncates the
     prompt and proceeds (emit the explanation to telemetry) instead of `ContextExceeded`.
     Memory/capability decisions return `Reject { reason }` in this cycle (swap is future).
2. **Home**: a new `engine/degraded_mode.rs` (or `memory/`), Razor-clean and unit-testable.
3. **Explicitly document** the future `DegradedDecision::PreferModel { model_id, reason }`
   variant the BitNet backend (B-02..B-06) will consume — declared but not implemented.
4. **Governance note**: this is L2 (policy/runtime behavior, no security/auth surface).
   Truncating context is a behavior change under pressure — the explanation surface makes
   it auditable, satisfying the "intentional + explained" requirement.

## Updated Knowledge (Shadow Genome)

No new failure pattern. Reinforces the decision/IO decompose discipline (cf. B-29a,
B-29b-2): the degraded-mode *decision* (policy → action) is pure and testable; the
*mechanism* (truncate + emit) is the thin effectful edge.

---

_Research complete. Findings advisory; the scope fork (policy+context-reduction now vs
also capability-preserve mechanism) is an operator decision at cycle start._
