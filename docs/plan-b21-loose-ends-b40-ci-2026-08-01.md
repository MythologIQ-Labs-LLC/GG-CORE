# Plan: B-21d + B-21h + B-40 — ADR-007 loose ends + advanced-in-CI

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - B-40: fix the 14 `advanced` clippy lints and add an `advanced` leg to the CI features matrix so
    advanced-gated code is linted + tested on every PR. B-21d: verify the (already-resolved) F2/F3
    loose ends, add a grounded speculative-security test, and correct the phantom THREAT_MODEL §12.2
    citation. B-21h: make the dormant Prometheus speculative counters live (executor → metrics) and
    surface counter-derived `speculative_stats` in `status`.
- non_goals:
  - No behavior change to the speculative decode itself; no new IPC status field (full latency/
    net-speedup/auto-disable telemetry over IPC is a follow-on — only the 3 Prometheus-counter-derived
    fields are surfaced this cycle); no `gguf,advanced` CI leg (the 14 lints + all non-gguf advanced
    code are covered by the `advanced` leg; the small `all(gguf,advanced)` surface is linted locally).
- exclusions:
  - No change to default/gguf/onnx/ffi/python CI behavior beyond adding the new leg.

## Open Questions

None. Research #199 verified the 14 lints, that B-21d's F2/F3 are already resolved and §12.2 is
phantom, and B-21h's IPC/dormant-counter reality.

## Design Rationale (Simple Made Easy)

Three independent loose ends, one cycle. B-40 removes an enforcement gap (lint what ships). B-21d
closes real-vs-phantom items honestly (most were resolved by consolidation; the one security test is
grounded in the actual threat model, §4.3, not a nonexistent §12.2). B-21h connects an already-built
but disconnected telemetry surface to its already-built display via the existing metrics channel,
rather than inventing a new one.

## Phase 1: B-40 — fix the 14 advanced lints + add the CI leg

### Affected Files

- `core-runtime/src/engine/quantize.rs` — `div_ceil` (5 sites: 88/113/159/185/193) → `a.div_ceil(b)`;
  same-item-`Vec`-push (101) → `vec![val; n]`/`resize`; needless-range-loop (141/143/161/187) →
  iterator form where behavior-preserving, else `#[allow(clippy::needless_range_loop)]` with a
  one-line justification for the 2-D SIMD-kernel indices.
- `core-runtime/src/engine/flash_attn_gpu.rs:114` — `div_ceil`.
- `core-runtime/src/engine/multi_gpu.rs:44` — replace the manual `Default` impl with
  `#[derive(Default)]` + `#[default]` on the default variant.
- `core-runtime/src/engine/simd_tokenizer_v2.rs:323` — `map_or` → `is_some_and`/`map` per the lint.
- `core-runtime/src/models/speculative_config.rs:21` — `#[derive(Default)]` + `#[default]` on
  `AdaptiveMode`'s default variant, dropping the manual `impl Default`.
- `.github/workflows/rust.yml` — add `advanced` to the `features` job matrix (`feature: [gguf, onnx,
  ffi, python, advanced]`), so it runs `cargo clippy --features advanced --all-targets -- -D warnings`
  + `cargo test --features advanced`.

### Unit Tests

Behavior-preservation is verified by the existing quantize/simd/speculative_config unit tests (all
`advanced`-gated) continuing to pass under `--features "gguf advanced"`; the enum `Default` changes are
checked by their existing `default()`-asserting tests. No new test (pure lint refactors).

## Phase 2: B-21d — verify loose ends + grounded security test + citation fix

### Affected Files

- `core-runtime/tests/security_speculative_test.rs` — add
  `speculative_draft_pair_cannot_bypass_model_allowlist`: an `InferenceEngine` with speculation active
  and a draft pair registered to an **unregistered** draft id → `try_speculative` resolves no draft
  model and the call falls through to single-model (no load of an unvalidated path; no panic). Asserts
  the speculative path inherits model-loading security (drafts are ids of already-loaded, path-
  allowlist-validated models — THREAT_MODEL §4.3).
- `docs/BACKLOG.md` — B-21d row: mark F2/F3 resolved-by-consolidation; **correct the phantom
  "THREAT_MODEL §12.2" citation to §4.3 Model Loading**.

### Unit Test

- `speculative_draft_pair_cannot_bypass_model_allowlist` — the behavior above (fall-through, no
  unvalidated load), asserting the returned inference used the single-model path.

## Phase 3: B-21h — live speculative telemetry in `status`

### Affected Files

- `core-runtime/src/engine/adaptive_speculative/executor.rs` — after each verify, also emit the
  Prometheus counters via `crate::telemetry::record_speculative_cycle(accepted, draft_len - accepted)`
  (making the dormant counters live), alongside the existing in-process `telemetry.record_step`.
- `core-runtime/src/cli/status.rs` — in `build_status`, derive `speculative_stats:
  Option<SpeculativeSessionStats>` from the metrics snapshot: when `core_speculative_drafts_total > 0`,
  populate `draft_tokens_generated`/`accepted_tokens`/`rejected_tokens`/`verification_steps`/
  `acceptance_rate` from the counters; leave latency/net-speedup/auto-disable at defaults (not in the
  metrics channel). `None` when no speculative activity.

### Unit Test

- `core-runtime/src/cli/status_tests.rs` — `build_status_populates_speculative_stats_from_metrics`:
  a metrics snapshot with speculative counters → `speculative_stats` is `Some` with the derived
  acceptance_rate/counts; absent counters → `None`.

## Feature Inventory Touches

- `entry_id`: `F-64` (Speculative telemetry surfaced in `status` — Prometheus counters live +
  metrics-derived stats) — `operation`: `NEW` — `test_path`:
  `core-runtime/src/cli/status_tests.rs`.

## Definition of Done

### Deliverable: advanced linted in CI; loose ends closed; speculative telemetry visible

- **D1**: `cargo clippy --features advanced --all-targets -- -D warnings` is clean and runs in CI;
  the speculative draft path is proven not to bypass model-loading security; `status` shows live
  speculative counts + acceptance rate when speculation has run.
- **D2**: the Phase 1 lint fixes + `rust.yml` `advanced` leg; the Phase 2 security test + backlog
  citation fix; the Phase 3 executor→Prometheus wiring + `build_status` derivation.
- **D3**: META_LEDGER #199 research → plan → audit → seal; BACKLOG B-21d/B-21h/B-40 done; FEATURE_INDEX
  F-64 NEW; GOVERNANCE_INDEX Tier 4; CHANGELOG note.
- **D4**: `cargo clippy --features advanced --all-targets -- -D warnings` clean; `cargo test -p
  gg-core --features "gguf advanced"` green (incl. the new security + status tests); default/gguf CI
  legs unaffected; fmt clean.

## CI Commands

- `cargo clippy --features advanced --all-targets -- -D warnings` — the B-40 gate (must be clean)
- `cargo clippy --features "gguf advanced" --all-targets -- -D warnings` — full advanced surface clean
- `cargo test -p gg-core --features "gguf advanced"` — security + status tests pass
- `cargo build -p gg-core --features gguf` — CI-safe leg unaffected (covers the bare default build too)
- `cargo fmt --check`
