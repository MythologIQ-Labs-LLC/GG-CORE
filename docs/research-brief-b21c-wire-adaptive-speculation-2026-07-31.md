# Research Brief — B-21c: Wire Adaptive Speculative Decoding into `Runtime::infer`

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-21c — make adaptive speculative decoding LIVE (reachable from `Runtime::infer`),
config-gated-off by default, correct (rejected tokens never committed; single-model fallback), and
telemetry-observable. The "make it live" step of ADR-007. L3 (secure inference path).
**Scope**: `engine/inference.rs`, `engine/adaptive_speculative/`, `engine/gguf/`,
`models/speculative_config.rs`, `engine/adaptive_speculative/telemetry.rs`, `cli/status*`.

---

## Executive Summary

The full ADR-007 adaptive stack exists (traits, heuristic estimator/scheduler, telemetry, tier
plan) but has **no executor and no engine wiring** — it is dormant. B-21c builds the missing
executor (promoting the tested `run_step` compose loop to a real outer loop), a GGUF adapter for the
adaptive traits, a target→draft resolver, and a **config-gated branch inside `InferenceEngine::run`**
that runs speculation when active and a draft resolves, else falls through to today's single-model
path. Because `Runtime::infer` brackets `run` with prompt-injection scan (before) and PII sanitize
(after), the speculative branch inherits both — no security change. **Load-bearing caveat**: the
GGUF backend rebuilds a `LlamaContext` per step (no KV reuse) and verifies greedily, so the wired
path is *correct but likely net-slower* → `auto_disable` (threshold 1.05) will fire; real speedup
needs a KV-reuse backend change, filed as **B-21f**. B-21c still kills the dormant state (speculation
becomes reachable, exercised, correct, opt-in, observable); B-21f makes it fast.

## Findings (verified, file:line-grounded via survey)

### F1 — the executor blueprint exists as a per-step helper
`adaptive_speculative/tests.rs:79-100` `run_step`: `draft()` → empty⇒`generate_one` fallback →
`estimate()` → `plan()` → fallback if `plan.is_fallback()` → `verify()` → `result.into_tokens()`.
This is per-step; the real executor owns the outer `while tokens<max` loop, context extension, EOS
check, and per-cycle `scheduler.record_result` / `estimator.record_acceptance` /
`telemetry.record_step`. Commit/fallback mechanics mirror v2's `speculative_step`
(`speculative_v2.rs:216-256`, `accept_tokens` 325-337): take `accepted_count` draft tokens + push
`correction_token` if present; empty accept ⇒ single-token fallback; **rejected suffix never
committed**.

### F2 — adaptive traits (async map)
`adaptive_speculative/mod.rs:157-215`: `BlockDraftModel::draft` (async), `TargetVerifier::{verify,
generate_one}` (async) + `eos_token` (sync); `ConfidenceEstimator::estimate` + `VerificationScheduler
::plan` (sync). Heuristic ctors: `HeuristicConfidenceEstimator::new(temperature_hint,
repetition_penalty_hint)`; `AdaptiveVerificationScheduler::new(AdaptiveSpeculativeConfig)` with
`record_result` / `auto_disable_fired`. Both map to `InferenceConfig.temperature` /
`.repetition_penalty`.

### F3 — the GGUF adaptive adapter is buildable from existing generator methods
`gguf/generator.rs`: `generate_tokens(ctx,count)` (`:113`), `verify_draft_tokens(ctx,draft)->
VerifyResult` (`:126`), `eos_token_id()` (`:139`). A new adapter (analogous to the v2
`GgufDraftModel`/`GgufTargetModel`) implements `BlockDraftModel::draft` (→ `generate_tokens` +
`DraftBlock::from_tokens`) and `TargetVerifier::{verify (map VerifyResult→VerificationResult),
generate_one, eos_token}`. **Degradation**: the backend surfaces no per-token log-probs, so
`DraftBlock.log_probs` = NEG_INFINITY and the estimator leans on temperature/rep/history, not draft
confidence — functional, not ideal.

### F4 — config: OFF by default; the correct gate
`models/speculative_config.rs:32-75` `AdaptiveSpeculativeConfig` defaults: `enabled=false`,
`mode=Disabled`, `max_draft_tokens=4`, verification 1..8, `confidence_floor=0.70`,
`acceptance_floor=0.60`, `auto_disable=true`, `auto_disable_threshold=1.05`. `is_active() = enabled
&& mode != Disabled` (`:80`). So the wired branch is inert unless explicitly enabled — the safe
default.

### F5 — the wiring seam + the draft-plumbing gap
Seam: **`InferenceEngine::run` (`inference.rs:59-69`), after `get_model`/`apply_degraded_context`,
before `infer_with_model`** — `Runtime::infer` (`runtime_facade.rs:66-80`) scans-before/sanitizes-
after around the whole `run`, so the branch inherits both (C.O.R.E.: enforcement in the façade,
compute in the engine). Do NOT branch in the façade. **Gap**: `run` carries only the target
`model_id`; the engine's `models: HashMap<String, Arc<dyn Model>>` (`:20`) can hold a draft as a
second id, but there is no target→draft resolver, and `TierSynergy`'s `ModelHandle`-based pairing is
not wired to the id map. B-21c adds a minimal explicit target→draft **id** map on the engine
(`register_draft_pair(target_id, draft_id)`), plus an optional `AdaptiveSpeculativeConfig` +
`Arc<SpeculativeTelemetry>`. Models are downcast to `GgufGenerator` via `Model::as_any`
(`model.rs:41`); any resolution/downcast miss ⇒ single-model fallthrough.

### F6 — telemetry readout is unplumbed
`telemetry.rs` `SpeculativeTelemetry::{record_step, record_auto_disable, snapshot}`; CLI
`status_format.rs:160 print_speculative` expects `SystemStatus.speculative_stats:
Option<SpeculativeSessionStats>`, currently hardcoded `None` (`status.rs:272`). B-21c routes
`snapshot()` into `SystemStatus` so the live stats show.

### F7 — the perf caveat (→ B-21f)
`gguf/backend.rs`: `generate_from_tokens` (`:175`) + `verify_tokens` (`:207`) each build a fresh
`LlamaContext` and decode the full context — **no KV reuse across steps**, greedy verify. So the
naive wired path does ~2× the model work per step and is likely NET-SLOWER than single-model →
`auto_disable` fires. Wiring is still correct + live; the speedup needs KV-cache reuse across
draft/verify steps — out of B-21c scope, filed **B-21f**.

## Recommendations

1. **B-21c deliverable** (one cohesive cycle, ends LIVE): build `adaptive_speculative/executor.rs`
   (outer loop + telemetry), a GGUF adaptive adapter (`gguf/adaptive_speculative.rs` or extend
   `gguf/speculative.rs`), engine plumbing (`register_draft_pair` + optional config/telemetry +
   `as_any` downcast), the config-gated branch in `run` with single-model fallthrough, and the
   telemetry readout into `SystemStatus`. Unit-test the executor (accept / reject-correction /
   empty-draft fallback / EOS / disabled-config passthrough) with mocks, and an engine-level test
   that an active config + registered draft pair takes the speculative path and produces
   sanitized-equivalent output. All `advanced`(+`gguf`)-gated.
2. **File B-21f** — KV-cache reuse across speculative steps in the GGUF backend (the real speedup);
   without it the wired path auto-disables. Honest follow-on, not a deferral of the wiring.
3. Keep speculation OFF by default (F4); the branch is opt-in and self-protecting (auto-disable).

## Updated Knowledge (Shadow Genome)

**Wiring ≠ speedup — but wiring ≠ dormant, either.** Making a built-but-dormant feature reachable +
correct + observable on the production path is the load-bearing anti-dormancy step, even when the
first wired version isn't yet faster (a backend limitation — no KV reuse). Ship the live, correct,
gated path; file the perf follow-on explicitly so "live" is honest about "not yet fast," rather than
leaving the whole stack dormant waiting for perfection.

---

_Research complete. B-21c = build the executor + GGUF adaptive adapter + engine draft-pair plumbing +
a config-gated branch in `run` (fallback default) + telemetry readout → speculation LIVE, correct,
opt-in, observable. B-21f (KV reuse) filed for the actual speedup. Security unchanged (scan/sanitize
wrap `run`)._
