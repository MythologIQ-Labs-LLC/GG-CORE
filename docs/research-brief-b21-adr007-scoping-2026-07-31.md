# Research Brief — B-21: ADR-007 Epic Scoping (TierSynergy Adaptive Speculative Decoding)

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-21 — decompose the ADR-007 epic (issues #60–#68) into governed sub-cycles by
reconciling the epic execution plan (`docs/plan-adr007-epic-execution.md`) against the current code
and ledger. **Read-only scoping — no implementation.**
**Scope**: `core-runtime/src/engine/{speculative.rs, speculative_v2.rs, adaptive_speculative/}`,
`core-runtime/src/models/{speculative_config.rs, tier_synergy*, tier_synergy_speculative.rs}`,
`benches/speculative_matrix.rs`, `docs/architecture/ADR-007-*`, `docs/security/THREAT_MODEL.md`,
`docs/FEATURE_INDEX.md`, `docs/META_LEDGER.md`.

---

## Executive Summary

**B-21 is not an unbuilt epic — it is a built, sealed, but DORMANT one.** All eight issues (#61–#68)
were implemented, unit-tested, and formally sealed as governed cycles (ledger entries #87–#94), and
appear in FEATURE_INDEX as verified features F-48–F-54. **However, the entire ADR-007 surface is
built-but-dormant behind the `advanced` feature gate: nothing is wired into the real inference path**
(`Runtime::infer` / `InferenceEngine` have zero references to any speculative/adaptive/tier_synergy
code). Three parallel speculative implementations coexist unused, the consolidation audit is stale
(it predates the third one), the canonical ADR design doc is missing from `main` (5 dangling
FEATURE_INDEX citations), and a few audit follow-ups + one security test are outstanding. So the
real remaining work is **not** "build the epic" — it is: reconcile the docs, resolve the redundancy,
and (the substantial, L3 part) decide whether and how to WIRE adaptive speculation into production.
That last decision is a strategic fork for the operator, because the ADR is still "Proposed" (PR #59
never merged).

## Findings (verified — agent-surveyed, file:line grounded)

### F1 — all 8 issues are DONE and SEALED (not unbuilt)
| Issue | Feature | Impl (file:line) | Sealed |
|---|---|---|---|
| #61 config + modes | F-48 | `models/speculative_config.rs:30-95` (`AdaptiveSpeculativeConfig`, `AdaptiveMode`) | ledger #88 |
| #62 decoder interfaces | F-49 | `engine/adaptive_speculative/mod.rs:157-215` (4 traits + types) | #89 |
| #63 heuristic + windows | F-50 | `engine/adaptive_speculative/heuristic/mod.rs:50-234` | #90 |
| #64 TierSynergy integration | F-51 | `models/tier_synergy_speculative.rs:101-155` (`TierSpeculativePlan::select`) | #91 |
| #65 telemetry + auto-disable + CLI | F-53 | `engine/adaptive_speculative/telemetry.rs:116-214` + `cli/status_format.rs:160` | #93 |
| #66 benchmark matrix | F-54 | `benches/speculative_matrix.rs:32-179` | #94 |
| #67 threat model + security tests | F-52 | `docs/security/THREAT_MODEL.md §12` + `tests/security_speculative_test.rs` | #92 |
| #68 consolidation audit | — | `docs/architecture/ADR-007-CONSOLIDATION-AUDIT.md` | #87 |
Each has real tests that exercise it (inline or path-included), all sealed "at local hold."

### F2 — the entire stack is DORMANT (no production wiring) — the load-bearing finding
- `SpeculativeDecoder` (v1 + v2): defined + re-exported (`engine/mod.rs:118-123`) but **never
  instantiated** outside tests. `TierSpeculativePlan::select`: called only in its tests + the bench.
  `adaptive_speculative` traits/heuristic/telemetry: composed into a decode loop only in
  `adaptive_speculative/tests.rs`. `TierSynergy::request_*_speculative` (`tier_synergy/mod.rs:121,167`)
  load model handles but run NO draft-verify loop. **`engine/inference.rs` / `Runtime::infer` have
  zero speculative references.** The whole ADR-007 surface is unit-tested scaffolding — no path from
  IPC/Runtime/InferenceEngine reaches it. Consistent with the ADR being "Proposed" and PR #59
  unmerged.

### F3 — triple redundancy; the consolidation audit is stale
- Three parallel implementations coexist: `engine/speculative.rs:85` (v1 `SpeculativeDecoder`),
  `engine/speculative_v2.rs:163` (a second `SpeculativeDecoder` + `SpeculativeConfig`/`Stats`), and
  `engine/adaptive_speculative/` (the trait-based ADR-007 impl). The #68 audit
  (`ADR-007-CONSOLIDATION-AUDIT.md`, 2026-07-08) declared `tier_synergy`/`service_routing`/
  `speculative*` canonical and endorsed keeping v1+v2 feature-gated — but it **predates**
  `adaptive_speculative` (#62+), so it never addressed the three-way redundancy. The audit is now
  stale w.r.t. the very stack the epic later built.

### F4 — B-13-class doc-drift: the canonical ADR doc is missing on `main`
- `docs/architecture/` holds only `ADR-006-*` and `ADR-007-CONSOLIDATION-AUDIT.md`.
  `ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md` is **absent** from the tree + `git ls-files`
  — it exists only on the unmerged remote branch `origin/docs/adr-007-tiersynergy-dspark` (PR #59).
  Yet FEATURE_INDEX rows **F-48, F-49, F-50, F-51, F-53** cite it as their design-doc home
  (`FEATURE_INDEX.md:79-85`) → 5 dangling references (same class as B-13, just fixed for
  `CORE_RUNTIME_ARCHITECTURE.md`).

### F5 — audit follow-ups + a security-test gap remain
- Audit F1 (tier_synergy Razor refactor) DONE (`models/tier_synergy/` split, sealed `#…:6442`). F2
  (remove stale "provided by GG-CORE-TierSynergy" comments) NOT done — still at `engine/mod.rs:8,42`.
  F3 (standalone `tests/tier_synergy_test.rs` + FEATURE_INDEX rows) PARTIAL — rows added, no
  standalone test. Separately, `THREAT_MODEL.md §12.2` cites a test
  `t1_draft_model_path_enforces_allowlist` that does **not** exist in `security_speculative_test.rs`
  (only T2–T5 present) — a doc↔test binding gap in F-52.

### F6 — the benchmark is overhead-only, not end-to-end
- `speculative_matrix.rs` measures construction/selection overhead on synthetic inputs — **no GGUF
  models, no speculative-vs-non-speculative end-to-end speedup** (file header `:1-12`). It cannot yet
  answer "does speculation help?"; a real e2e benchmark requires the production wiring (F2) to exist.

## Blueprint Alignment

| Epic-plan assumption (`plan-adr007-epic-execution.md`) | Reality | Status |
|---|---|---|
| #61–#68 to be executed "one cycle at a time" | all 8 sealed (#87–#94) | DRIFT — plan is the executed record, not a to-do |
| Feature-gate new speculative code | behind `advanced`, but never wired past the gate | DRIFT — gated AND dormant |
| #66 benchmark measures net speedup honestly | measures overhead only, synthetic | PARTIAL |
| #68 audit affirms canonical impl | stale; predates `adaptive_speculative` triple-redundancy | DRIFT |
| Design doc = ADR-007-TIERSYNERGY-… | missing on main (unmerged PR #59) | DRIFT (F4) |

## Recommendations — reframed decomposition

B-21's remainder is NOT the original #61–#68 (those shipped). Proposed governed sub-cycles:

1. **B-21a — ADR doc reconciliation (doc-only, L1, do first).** Create/merge the canonical
   `ADR-007-TIERSYNERGY-ADAPTIVE-SPECULATIVE-DECODING.md` on `main` (from the PR #59 branch or
   authored fresh, grounded in the sealed impl) so F-48/49/50/51/53's citations resolve. B-13-class,
   low-risk, closes the dangling references.
2. **B-21b — refresh the consolidation audit + resolve redundancy (L2).** Update
   `ADR-007-CONSOLIDATION-AUDIT.md` to account for `adaptive_speculative`; decide the canonical
   speculative impl among v1/v2/adaptive and deprecate/remove the losers (operator-approved
   deletion). Removes the three-way redundancy the stale audit missed (F3).
3. **B-21d — audit loose ends (L1/L2).** Audit F2 (strip stale `engine/mod.rs:8,42` comments), F3
   (standalone `tests/tier_synergy_test.rs`), and the missing `t1_draft_model_path_enforces_allowlist`
   security test (or correct THREAT_MODEL §12.2 to match reality). Small, closes F5.
4. **B-21c — production wiring (L3, the substantial remainder) — GATED ON A STRATEGIC DECISION.**
   Wire adaptive speculation into `Runtime::infer`/`InferenceEngine` so it is no longer dormant:
   prompt-injection scan BEFORE and output sanitize AFTER speculation, rejected tokens never
   committed, single-model fallback the default, auto-disable honored. L3 (inference hot path +
   security boundary). **This is the real "make it live" work and MUST NOT start until the operator
   confirms adaptive speculation is wanted in production** — the ADR is still "Proposed" (PR #59
   unmerged), and wiring an unvalidated speedup into the secure path is a deliberate product choice.
5. **B-21e — real end-to-end benchmark (L2, gated on B-21c).** Replace the overhead-only bench with a
   speculative-vs-non-speculative e2e matrix (GGUF, prompt classes, honest "where it hurts"
   reporting) to justify keeping speculation on. Depends on B-21c wiring existing.
6. **Backlog reconciliation (immediate, any cycle).** Mark B-21's #61–#68 build work DONE (sealed
   #87–#94); re-scope the B-21 row to the a/b/c/d/e remainder so "open" no longer implies "unbuilt."

**Strategic fork for the operator:** either (A) invest in B-21c wiring + B-21e (make speculation
live — significant L3 work), or (B) keep the scaffolding dormant and reduce B-21 to the doc/
consolidation/loose-end cleanup (B-21a/b/d) that stops the drift while deferring wiring. The dormant
stack is safe as-is (gated, unwired), so (B) carries no risk; (A) is a product bet on the speedup.

## Updated Knowledge (Shadow Genome)

**A sealed epic can be built-but-dormant — "sealed" ≠ "wired to production."** All 8 ADR-007 issues
passed governed cycles (#87–#94) and show as verified features, yet none is reachable from
`Runtime::infer`. A FEATURE_INDEX "verified" row proves a test exercises the unit, NOT that the unit
is on a production path. Scoping an "open" epic must check production wiring (grep the real entry
points), not just whether the code + tests + seals exist — otherwise "done" hides "never turned on."

---

_Research complete. B-21's #61–#68 are built + sealed but dormant; the remainder is doc reconciliation
(B-21a), consolidation (B-21b), loose ends (B-21d), and — behind a strategic go/no-go — production
wiring (B-21c) + a real e2e benchmark (B-21e). Findings advisory; the wiring decision is the
operator's._
