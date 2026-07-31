# Plan: B-21c — Wire Adaptive Speculative Decoding into `Runtime::infer`

**change_class**: feature

**doc_tier**: system

**terms_introduced**:
- term: AdaptiveSpeculativeExecutor
  home: core-runtime/src/engine/adaptive_speculative/executor.rs

**boundaries**:
- limitations:
  - Makes adaptive speculation LIVE: reachable from `Runtime::infer` via a config-gated branch in
    `InferenceEngine::run`, correct (rejected suffix never committed; single-model fallback the
    default), off by default, telemetry-observable. Does NOT add KV-cache reuse — the wired path is
    correct but likely net-slower (auto-disables); the speedup is B-21f.
- non_goals:
  - No KV reuse (B-21f); no per-token log-probs from the backend (estimator runs degraded); no
    TierSynergy↔engine ModelHandle unification (a minimal id-keyed `register_draft_pair` is used);
    no security-pipeline change (scan/sanitize already wrap `run`).
- exclusions:
  - No change to the default (non-speculative) path when the config is inactive or no draft resolves.

## Open Questions

None. Surfaces mapped in research #188 (executor blueprint, trait async-map, GGUF adapter methods,
config gate, the `run` seam + draft-plumbing gap, telemetry readout, the KV-reuse caveat).

## Design Rationale (Simple Made Easy)

The security boundary stays in the façade (`Runtime::infer` scans-before/sanitizes-after around the
whole `engine.run`), so the speculative branch lives *inside* `run` and inherits both guarantees —
compute in the engine, enforcement in the façade (C.O.R.E.). Speculation is opt-in (`is_active()` is
false by default) and self-protecting (`auto_disable`), and any resolution/downcast miss falls
through to today's single-model path — so the change is inert unless deliberately enabled and a draft
pair is registered. The executor is the tested `run_step` compose loop promoted to an outer loop with
commit/fallback mechanics mirroring v2's proven `speculative_step`.

## Phase 1: The executor

### Affected Files

- `core-runtime/src/engine/adaptive_speculative/executor.rs` (NEW, `advanced`-gated) — `pub struct
  AdaptiveSpeculativeExecutor` composing `&dyn BlockDraftModel`, `&dyn TargetVerifier`, `&dyn
  ConfidenceEstimator`, `&dyn VerificationScheduler`, and `&SpeculativeTelemetry`. `pub async fn
  run(&self, prompt_tokens, max_tokens) -> Result<Vec<u32>, InferenceError>`: outer loop —
  `draft()` (empty ⇒ `generate_one` fallback) → `estimate()` → `plan()` (fallback ⇒ `generate_one`)
  → `verify()` → `into_tokens()` committed to `context`; after each verify call
  `scheduler.record_result`, `estimator.record_acceptance`, `telemetry.record_step`; stop at
  `max_tokens` or when a committed token == `target.eos_token()`. **Rejected suffix is never
  committed** (only `accepted_count` + optional correction). Returns the generated token vec.
- attach `pub mod executor;` in `adaptive_speculative/mod.rs` (`advanced`-gated).

### Unit Tests (`adaptive_speculative/executor_tests.rs`, `#[path]`)

- `accepts_full_block` (all draft accepted advances by block); `commits_correction_on_reject`
  (accepted prefix + correction only, rejected suffix absent); `empty_draft_falls_back_to_one`;
  `stops_at_eos`; `disabled_scheduler_plan_fallback_uses_single_token` — all via mock traits.

## Phase 2: GGUF adaptive adapter

### Affected Files

- `core-runtime/src/engine/gguf/adaptive_speculative.rs` (NEW, `all(gguf, advanced)`-gated) —
  `GgufBlockDraftModel` (`impl BlockDraftModel`: `draft` → `generator.generate_tokens` then
  `DraftBlock::from_tokens`) and `GgufTargetVerifier` (`impl TargetVerifier`: `verify` → map
  `generator.verify_draft_tokens`'s `VerifyResult{accepted_count,correction_token}` into
  `VerificationResult::{accept_all|reject_at}`; `generate_one` → `generate_tokens(ctx,1)`;
  `eos_token` → `eos_token_id`). Re-export from `gguf/mod.rs` (gated).

## Phase 3: Engine plumbing + the gated branch

### Affected Files

- `core-runtime/src/engine/inference.rs` — add to `InferenceEngine`: `spec_config:
  AdaptiveSpeculativeConfig` (default = off) + `Arc<SpeculativeTelemetry>` + `draft_pairs:
  HashMap<String,String>` (target_id→draft_id); `pub fn set_speculative_config(&mut, cfg)` +
  `pub async fn register_draft_pair(&self, target_id, draft_id)`. In `run` (`:59-69`), after
  `get_model`+`apply_degraded_context`: if `spec_config.is_active()` and `draft_pairs` resolves a
  draft `Arc<dyn Model>`, downcast both to `GgufGenerator` via `as_any`; on success build the
  adapters + `HeuristicConfidenceEstimator::new(cfg.temperature, cfg.repetition_penalty)` +
  `AdaptiveVerificationScheduler::new(spec_config.clone())` + the shared telemetry, tokenize the
  prompt, run `AdaptiveSpeculativeExecutor::run`, detokenize → `InferenceResult`. **Any miss ⇒
  fall through to `infer_with_model` (the single-model default).** Speculative path is
  `all(gguf, advanced)`-gated; the default path is unchanged.

### Unit Test

- `speculative_branch_falls_through_when_inactive` (default config ⇒ single-model path) and, gated,
  `active_config_with_registered_pair_uses_executor` (records telemetry steps) — engine-level.

## Phase 4: Telemetry read-out

### Affected Files

- `core-runtime/src/cli/status.rs` — populate `SystemStatus.speculative_stats` from the engine's
  `Arc<SpeculativeTelemetry>::snapshot()` (was hardcoded `None`), so `print_speculative` shows live
  data.

## Feature Inventory Touches

- `entry_id`: `F-61` (Adaptive speculative decoding — LIVE on the inference path) — `operation`:
  `NEW` — `test_path`: `core-runtime/src/engine/adaptive_speculative/executor_tests.rs`. Marks the
  transition from dormant scaffolding to a wired, config-gated production path.

## Definition of Done

### Deliverable: adaptive speculation reachable from `Runtime::infer`, correct + gated + observable

- **D1**: With `AdaptiveSpeculativeConfig` active and a registered draft pair, `Runtime::infer` runs
  the speculative executor (scan-before/sanitize-after intact); rejected tokens are never emitted;
  single-model fallback is the default; telemetry is observable via `status`. Off by default.
- **D2**: `executor.rs` + `gguf/adaptive_speculative.rs` + the `run` branch + `register_draft_pair`
  + the status readout, all `advanced`(+`gguf`)-gated.
- **D3**: META_LEDGER entries (canonical markup) research #188, plan, audit, seal; BACKLOG B-21c →
  done, B-21f filed; FEATURE_INDEX F-61 NEW; CHANGELOG note.
- **D4**: `cargo build -p gg-core --features "gguf advanced"` compiles; `cargo test -p gg-core
  --features advanced adaptive_speculative::executor` — the executor tests pass (accept / correction
  / empty-fallback / eos / disabled-fallback); `cargo test -p gg-core --features "gguf advanced"`
  green; fmt + clippy (changed files) clean.

## CI Commands

- `cargo build -p gg-core --features "gguf advanced"` — the wired speculative path compiles
- `cargo test -p gg-core --features advanced adaptive_speculative::executor` — executor tests pass
- `cargo fmt --check`
- `cargo clippy -p gg-core --features "gguf advanced" -- -D warnings` (changed files clean; note B-40 pre-existing advanced lints)
