# Research Brief — B-13 + B-14: Documentation & Feature-Index Accuracy

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-13 (CLAUDE.md cites a nonexistent `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md`)
and B-14 (deep-verify the `verified` rows in `docs/FEATURE_INDEX.md` per SG-035). Combined
documentation-accuracy cycle.
**Scope**: `docs/architecture/`, `CLAUDE.md`, `docs/TANDEM_EXPERIMENTS_PROPOSAL.md`,
`docs/FEATURE_INDEX.md` + the tests it cites.

---

## Executive Summary

B-13: `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` does not exist (the dir holds ADR-006,
ADR-007, SCALABILITY_REMEDIATION, V0.6.0_TRADE_OFFS), yet it is cited as the "Full technical
specification" in `CLAUDE.md:92` and referenced load-bearingly in `TANDEM_EXPERIMENTS_PROPOSAL.md`
(as the source for the C.O.R.E. constraints). Deleting the reference would orphan those citations,
so the correct fix is to **create** the doc, grounded in the current code. B-14: the two
`unverified` FEATURE_INDEX rows are both fixable — F-45 (Veritas shim) cites `n/a` but the shim has
14 inline unit tests; F-38 (sandbox) cites a real functional test that is unix-gated (CI-verified,
not verifiable on the Windows dev host). The other 58 rows are tool-verified (cited test exists +
passes). Recommendation: create a code-grounded `CORE_RUNTIME_ARCHITECTURE.md`, correct F-45's test
citation, mark F-38 CI-verified, and record the deep-verify methodology.

## Findings (verified)

### F1 — B-13: the cited architecture doc is genuinely absent
- `docs/architecture/` contains `ADR-006-DEPLOYMENT-STRATEGIES.md`, `ADR-007-CONSOLIDATION-AUDIT.md`,
  `SCALABILITY_REMEDIATION_UPGRADE_PATH.md`, `V0.6.0_TRADE_OFFS.md` — **no**
  `CORE_RUNTIME_ARCHITECTURE.md`. Cited at `CLAUDE.md:92` ("Full technical specification") and in
  `docs/TANDEM_EXPERIMENTS_PROPOSAL.md:14,20-26` (attributing the process/IPC/network/filesystem
  C.O.R.E. constraints to it). A prior reconciliation brief flagged it as known drift (B-13).

### F2 — B-13: the content to document already exists in code + docs (so the doc can be accurate)
- The current architecture is fully determinable from the repo: the C.O.R.E. constraints
  (`CLAUDE.md`, `docs/CONCEPT.md`), the module tree (`core-runtime/src/{ipc,scheduler,engine,models,
  memory,telemetry,security,sandbox,ffi,python,cli}`), the secure inference façade
  (`runtime_facade.rs` `Runtime::infer` → scan → engine → sanitize), model dispatch
  (`models/backend_dispatch.rs` GGUF/ONNX via manifest), and the security/sandbox boundaries. A new
  doc consolidates these with file references — no invention required.

### F3 — B-14: F-45 (Veritas shim) is mis-cited, not untested
- `docs/FEATURE_INDEX.md` F-45 row: `core-runtime/src/shim/` | test `n/a` | `unverified`. But
  `src/shim/` is real (`lib.rs:51 pub mod shim;`; `mod.rs`, `rate_limiter.rs`, `service_tier.rs`)
  and carries **14 inline unit tests** (`mod.rs` 3, `rate_limiter.rs` 6, `service_tier.rs` 5). The
  `n/a` test citation is wrong; the feature is tested. Fix: cite the inline tests, status →
  verified.

### F4 — B-14: F-38 (sandbox) is real + functional but platform-gated
- F-38 cites `core-runtime/tests/sandbox_test.rs` (exists, 1471 B) whose tests invoke real behavior
  (e.g. `sandbox_default_config_is_reasonable` asserts `SandboxConfig::default()` invariants —
  functional, not presence-only). It is marked `unverified` because the sandbox path is unix-gated
  (`#[cfg(unix)]`; not buildable on the Windows dev host) — so it is CI-verified (ubuntu), not
  locally. Fix: status → verified with a CI-gated note (consistent with how other platform-gated
  features are treated, per `ci-only-runs-on-main-prs`).

### F5 — B-14: the other 58 rows are tool-verified; SG-035 is the added lens
- `feature_index_verify` reports total=60 verified=58 unverified=2 — the 58 have an existing,
  passing cited test. SG-035 deep-verify adds "the test must EXERCISE the feature (invoke + assert
  output), not merely assert presence." The rows added under governance this session (F-53..F-60)
  were authored to that standard (their tests invoke the unit + assert). The deliverable records the
  methodology: tool-confirmed existence+pass for all 60; the 2 defective rows fixed; SG-035
  presence-only check applied to the fixed rows and a spot-check of the pre-existing set.

## Recommendations

1. **B-13**: create `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` — a code-grounded technical
   spec (purpose/C.O.R.E., module map with file refs, the secure `Runtime::infer` data flow,
   GGUF/ONNX dispatch, scheduler/memory/security/sandbox boundaries, FFI/PyO3/IPC surfaces,
   consumable-dependency shape). Keep `CLAUDE.md:92` and the TANDEM references pointing at it.
2. **B-14**: fix F-45 (cite the 14 inline shim tests; verified) and F-38 (verified, CI-gated note);
   re-run `feature_index_verify`; record the deep-verify methodology + SG-035 result in the seal.
3. Single combined cycle (both are documentation-accuracy; no source/behavior change).

## Updated Knowledge (Shadow Genome)

**An index row can be wrong in two directions.** F-45 was marked untested when it has 14 tests
(under-crediting → a real feature looked unverified); a mis-cited `n/a` is as much drift as a
missing test. Deep-verify must reconcile the CITATION against reality, not just check the cited path
exists.

---

_Research complete. B-13 = create the code-grounded architecture doc; B-14 = fix the 2 mis-stated
index rows + record the deep-verify. Documentation-only; no source/behavior change._
