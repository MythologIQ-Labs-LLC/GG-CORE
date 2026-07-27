# Research Brief — Backlog Reconciliation

**Date**: 2026-07-27
**Analyst**: The Qor-logic Analyst
**Target**: `docs/BACKLOG.md` — reconcile every open/in-progress row against the
current green `main` @ v0.8.2 (the backlog's last reconciliation was 2026-07-08,
before this session's hardening + release work).
**Scope**: status verification only; no code changes. Produces a re-graded backlog
and a set of GitHub issue/PR dispositions (held for operator approval per the
Review Boundary).

---

## Executive Summary

The backlog is a pointer layer stale by ~3 weeks. Verified against green `main`,
**7 rows are resolved** (5 by this session's work, 2 already-closed issues),
**1 PR needs an operator close/rebase decision** (superseded), and **7 rows remain
genuinely open** (3 bounded engineering, 2 doc/governance chores, 2 deferred-epic
umbrellas). Four GitHub issues are ready to close.

## Findings (verified)

### Resolved — recommend close / mark done

| Row | Issue | Evidence (file:line / observed state) |
|-----|-------|----------------------------------------|
| B-01 | #54 | clippy `-D warnings` green on all 3 OS legs (this session's CI); #54 not in open-issue list (already closed) |
| B-15 | — | `.github/workflows/rust.yml` exists with fmt+clippy+test ×3 OS + `features` matrix; all 11 legs green on #78/#79/#80 |
| B-17 | #55 | `cargo test --lib validate_path` → 4 passed; `--lib kv_cache` → 7 passed |
| B-18 | #56 | clippy `-D warnings` green (default + gguf/onnx/ffi/python) |
| B-19 | #57 | `models/loader.rs:80` rejects `'\0'` with a fixed `<nul-byte rejected>` sentinel — exactly the requested control |
| B-20 | #58 | already done (PageTable redesign, 15/15); #58 not in open list |
| B-22 | #69 | fixed `626f034` (`Cancelled` arm added); #69 stale-open → close |
| B-10 | — | 193-file fmt sweep no longer present; `git status` clean (only a regenerated `include/gg_core.h`) |
| B-11 | — | `governance-health` reports `OK docs/ARCHITECTURE_PLAN.md` |
| B-12 | — | `governance-health` reports `OK docs/GOVERNANCE_INDEX.md` |

### Needs operator decision

- **B-08 / PR #47** — OPEN, `updatedAt` 2026-07-08, 25 files / +52 −80,
  `mergeStateStatus` UNKNOWN. Touches surfaces main has since rewritten
  (`ab_testing/traffic/bucket.rs` — edited this session for rand 0.9;
  `security/output_sanitizer.rs`, `security/pii_patterns.rs` — reworked by the
  security-chain wiring). Its stated goal (clippy cleanup + cfg-gating advanced
  tests) is already achieved on main. **Recommendation: close as superseded**
  unless a file-level diff surfaces unique content worth rebasing.
- **PR #74** — Dependabot `rand` 0.8.5→0.8.7 in `core-runtime/fuzz` (separate
  lockfile; not the main crate we migrated to 0.9). Low value; **recommend close**
  (or fold the fuzz crate onto rand 0.9 in a later chore).
- **PR #59** — TierSynergy adaptive-speculative-decoding ADR; belongs to the B-21
  epic. **Keep open**, resolve inside the deferred epic effort.

### Genuinely open (carry forward)

| Row | Issue | Disposition |
|-----|-------|-------------|
| B-24 | — | Phase-1 cycle #1 (streaming egress sanitization + IPC protocol decision), L3 |
| B-28 | #72 (follow-up) | Phase-1 cycle #2 (real WordPiece tokenizer), L2 |
| B-29 | #72 (follow-up) | Phase-1 cycle #3 (ONNX registry auto-dispatch), L2 |
| B-07 | #53 | Phase-1 cycle #4 (degraded-mode policy), L2 |
| B-16 | — | Phase-1 cycle #5 (`sandbox/unix.rs` 523>250 Razor refactor), L3 |
| B-13 | — | doc drift: create `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` or fix the CLAUDE.md reference (P3; fold into a docs pass) |
| B-14 | — | deep-verify FEATURE_INDEX `verified` rows (P3; ongoing) |
| B-26 | — | cross-workspace COREFORGE handoff (Personal-Task-Management #37); not executed here |
| B-02..B-06 | #48–52 | **deferred epic**: Backend Capability Contract + BitNet |
| B-21 | #59–68 | **deferred epic**: ADR-007 TierSynergy / DSpark |
| #70 | #70 | Research issue (Hologram κ-addressed substrate) — exploratory; out of the current sequence |

## Blueprint Alignment

| Claim | Finding | Status |
|-------|---------|--------|
| Backlog reflects observable state | 7 rows drifted stale since 2026-07-08 | DRIFT (corrected here) |
| `main` is production-ready/green | confirmed across 11 CI legs on #78/#79/#80 | MATCH |

## Recommendations

1. Close GitHub issues **#55, #56, #57, #69** (resolved). *(Held for operator approval.)*
2. Operator decision on **PR #47** (recommend close-superseded) and **PR #74** (recommend close).
3. Proceed to Phase 1 in order: **B-24 → B-28 → B-29 → B-07 → B-16**, one governed
   `/qor-auto-dev-1` cycle each, stopping at the Review Boundary per cycle.
4. Fold **B-13** (and B-11/B-12 closure) into a lightweight docs/governance pass.

## Updated Knowledge

Reinforces Shadow Genome discipline "verify the ground before building on it": a
pointer-layer backlog decays silently; a reconciliation pass must precede any
sequenced execution against it, or governed cycles get spent on phantom work.

---

_Research complete. Findings advisory; issue/PR mutations held for operator approval._
