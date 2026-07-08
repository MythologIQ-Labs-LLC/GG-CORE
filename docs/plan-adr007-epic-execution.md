# Execution Plan: ADR-007 Epic — TierSynergy Adaptive Speculative Decoding

> **Audience**: an implementer (possibly a smaller model) executing the ADR-007
> issue set (#60–#68) one governed cycle at a time. This is a decomposition +
> per-issue brief, NOT a single mega-plan. Do ONE issue per governed cycle
> (`/qor-plan` → `/qor-audit` → `/qor-implement` → `/qor-substantiate`).

**ADR doc**: `docs/architecture/ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md`
(currently on PR **#59**, branch `docs/adr-007-tiersynergy-dspark` — NOT yet on
main; see runbook for merge order). **Status: Proposed.**
**Canonical issues**: GitHub #60–#68. **Backlog**: B-21 (epic pointer).

**Prime directive from the ADR (do not violate)**: GG-CORE stays a pure compute
boundary — Contained, Offline, Restricted, Execution. The speculative path must
remain local, offline, IPC-bound, auditable, fail-closed. NO network model
server, NO Python/DSpark/DeepSeek/SGLang dependency, NO agent/orchestration/policy
authority, NO persistence of prompt/output/PII in telemetry, NO GPU requirement
for v1, NO copied speedup claims without local GG-CORE benchmarks.

**Existing baseline to build ON (already in tree, verified 2026-07-08)**:
- `core-runtime/src/models/tier_synergy.rs` — `TierSynergy`, `SynergyMode`
  (`Single`, `SpeculativeLightQuality`, `SpeculativeLightBalanced`,
  `SpeculativeBalancedQuality`), Light/Balanced/Quality tiers, single-model fallback.
- `core-runtime/src/engine/speculative.rs` + `speculative_v2.rs` (feature `advanced`).
- `core-runtime/src/engine/gguf/speculative.rs` — GGUF draft/target wrappers.
- Bench suite `core-runtime/benches/` + `docs/BENCHMARKS.md` CPU baseline.

---

## Dependency order (execute top-to-bottom; each is its own cycle)

```
#68  audit consolidation (design/docs) ─┐  can run in parallel with #61
#61  config + runtime modes ────────────┤
#62  decoder interfaces (traits/types) ─┤ depends on #61 config
#63  heuristic confidence + windows ────┤ depends on #62 interfaces
#64  TierSynergy integration ───────────┤ depends on #62, #63
#65  telemetry + auto-disable + fallback ┤ depends on #62, #64
#67  threat model + security tests ─────┤ depends on #64, #65 (validate boundary)
#66  benchmark matrix ──────────────────┘ depends on #64, #65 (measure the result)
```

Rationale: config → interfaces → algorithm → integration → observability →
security validation → measurement. #68 (repo consolidation audit) is design-only
and can proceed independently first.

---

## Per-issue execution briefs

Each brief: **files** (where the work lands), **deliverable** (the concrete
artifact + acceptance), **guardrails** (ADR constraints that apply). Feature-gate
all new speculative code behind an appropriate cfg (reuse `advanced` or add a
dedicated `speculative` feature — decide in #61 and hold it constant).

### #68 — Consolidation audit (design/docs only)
- **Files**: new `docs/architecture/ADR-007-CONSOLIDATION-AUDIT.md`; possibly
  follow-up issues.
- **Deliverable**: documented recommendation on whether/what to migrate from the
  standalone `GG-CORE-TierSynergy` repo into `core-runtime/src/models/tier_synergy.rs`;
  inventory of every in-tree TierSynergy folder/artifact/doc/test/build-ref;
  migrate-or-reject decision per item; a deprecation notice for the standalone
  repo. GG-CORE's internal impl stays canonical.
- **Guardrails**: no code authority added to TierSynergy; standalone repo deleted/
  archived ONLY after migration verified. No autonomous deletion — operator acts.
- **AC**: recommendation documented; divergent responsibilities identified;
  migration items opened as issues; canonical status affirmed.

### #61 — Adaptive speculative config + runtime modes
- **Files**: `core-runtime/src/models/tier_synergy.rs` (or a new
  `core-runtime/src/models/speculative_config.rs`); export via `models/mod.rs`.
- **Deliverable**: `AdaptiveSpeculativeConfig` (serde-serializable) with: mode,
  draft-token limit, verification-token min/max, confidence floor, acceptance
  floor, auto-disable behavior, telemetry toggle, runtime cost-profiling toggle,
  tier-aware flag. Runtime modes represented explicitly (extend/rename around
  the existing `SynergyMode`). **Safe defaults: speculation DISABLED or
  conservative; must be fully disable-able.**
- **Guardrails**: config introduces NO network/tool/agent/orchestration authority.
- **AC**: type exists; modes explicit; CPU-only-safe defaults; `disable`
  path exists; serializable for CLI/IPC/status. Unit test: default config has
  speculation off; round-trips through serde.

### #62 — DSpark-inspired decoder interfaces
- **Files**: new `core-runtime/src/engine/adaptive_speculative/mod.rs` (traits +
  types), behind the chosen feature gate; export from `engine/mod.rs`.
- **Deliverable**: traits `BlockDraftModel`, `ConfidenceEstimator`,
  `VerificationScheduler`, `TargetVerifier`; types `DraftBlock`, `SurvivalProfile`,
  `VerificationPlan`, `VerificationResult`. Backend-agnostic (GGUF/CPU/GPU/future).
- **Guardrails**: NO learned confidence heads in v1; interfaces support fallback
  to standard target decoding; implementable for the existing GGUF draft/target
  wrappers (`engine/gguf/speculative.rs`).
- **AC**: compiles behind the feature gate; unit tests cover success, rejection,
  and fallback paths against a mock draft/target.

### #63 — Heuristic confidence scheduling + verification windows
- **Files**: `core-runtime/src/engine/adaptive_speculative/` (impl of the #62
  `ConfidenceEstimator` + `VerificationScheduler`).
- **Deliverable**: heuristic estimator using available signal (draft token
  probability, entropy, temperature, top-p, repetition penalty, model pair,
  prompt class if available, historical acceptance rate); verification-window
  selection within min/max bounds; acceptance-floor + confidence-floor behavior;
  auto-disable when speculation underperforms.
- **Guardrails**: works WITHOUT GPU; low-confidence draft tails are NOT
  over-verified.
- **AC**: windows shrink/expand within bounds; auto-disable triggers below
  threshold; tests cover high-confidence, low-confidence, and underperforming paths.

### #64 — Integrate adaptive speculation with TierSynergy tiers
- **Files**: `core-runtime/src/models/tier_synergy.rs`.
- **Deliverable**: TierSynergy returns a complete speculative execution plan from
  {available tiers, load hints, hardware profile, observed acceptance}. Support
  Light→(Balanced|Quality) and Balanced→Quality draft/target pairings. Add
  tokenizer-compatibility + model-family-compatibility checks. Single-model
  fallback stays the DEFAULT when a pairing is unsafe/unavailable.
- **Guardrails**: quick queries can stay single-model; only batch/complex select
  speculation when beneficial.
- **AC**: unit tests cover Light/Quality, Light/Balanced, Balanced/Quality, and
  single-tier fallback; incompatible pairings fall back safely.

### #65 — Telemetry, auto-disable, fallback semantics
- **Files**: `core-runtime/src/telemetry/` + `core-runtime/src/models/tier_synergy.rs`;
  surface through existing status/diagnostics (`core-runtime/src/cli/status.rs`).
- **Deliverable**: aggregate `SpeculativeStats` (draft/verification/accepted/
  rejected token counts, acceptance rate, mean accepted length, latency,
  throughput, overhead, net speedup); auto-disable reason codes; status output
  shows enabled/disabled/auto-disabled.
- **Guardrails (SECURITY)**: telemetry stores NO prompt text, output text, PII,
  or model secrets. Every speculative failure falls back to single-model;
  rejected tokens are NEVER committed.
- **AC**: metrics content-safe; auto-disable reason observable; fallback tested;
  rejected suffix cannot corrupt output.

### #67 — Threat model + security tests
- **Files**: `docs/security/THREAT_MODEL.md`; `core-runtime/tests/security_*` (new
  e.g. `security_speculative_test.rs`).
- **Deliverable**: threat-model update for speculative decoding; security tests
  for draft-model loading, target verification, rejected-token handling,
  telemetry safety, fallback. Confirm prompt-injection filtering runs BEFORE and
  output sanitization AFTER speculation; incompatible/unsafe pairings fail closed.
- **AC**: rejected draft suffixes cannot be emitted; telemetry content-safe; any
  security-validation failure falls back or fails closed.

### #66 — Benchmark matrix (CPU→GPU)
- **Files**: `core-runtime/benches/` (new e.g. `speculative_matrix.rs`);
  `docs/BENCHMARKS.md` (results section).
- **Deliverable**: benchmark scenarios Tier 0→5 hardware scopes, CPU-only first
  (0.5B/1.5B/3B/7-8B GGUF where available); prompt classes (short-factual,
  long-form, code, structured JSON, boilerplate, creative, high-temp, injection
  attempts, long-context). Measure tokens/sec, first-token + e2e latency, draft/
  verification overhead, acceptance rate, mean accepted length, memory, CPU/GPU
  util, net speedup, auto-disable frequency, correctness.
- **Guardrails**: CPU-only path works without GPU; results distinguish VERIFIED
  from ESTIMATED; report MUST include cases where speculation HURTS; no copied
  DSpark speedup claims.
- **AC**: harness compares speculative vs non-speculative GG-CORE; honest reporting.

---

## Governance notes for the whole epic

- Each issue = its own `/qor-plan` cycle. Several are L3 (#65 telemetry-safety,
  #67 security) — `/qor-audit` PASS is mandatory before implementing those.
- Every plan that touches `src/` MUST update `docs/FEATURE_INDEX.md` in the same
  commit (Phase 73 obligation) — add rows for the new speculative surfaces.
- Do NOT close GitHub issues from an agent session — comment evidence; operator closes.
- Keep the feature-gate name constant across #61–#67 (decided in #61).
- The ADR is still `Proposed` — if implementation reveals the design is wrong,
  amend the ADR via a new PR, do not silently diverge.
