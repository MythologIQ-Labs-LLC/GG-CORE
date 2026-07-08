# Backlog

Reconstructed governance artifact (required, non-scaffold). Repaired via
`/qor-remediate` on 2026-07-08 because the Phase 109 governance-health schema
requires it and it had never been created since bootstrap.

This is a **pointer layer**. Canonical work lives in GitHub issues and PRs on
`MythologIQ-Labs-LLC/GG-CORE`; rows here reference that work and never duplicate
it. Priority and status reflect observable state at reconstruction time.

## Legend

- **Priority**: P1 (blocks a clean gate / release), P2 (planned enhancement),
  P3 (opportunistic).
- **Status**: open, in-review, in-progress.

## Engineering backlog (canonical: GitHub issues)

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-01 | clippy `-D warnings` fails on Linux/macOS: dead code + lints in `sandbox/unix.rs` | issue #54 | P1 | open | Resolve dead-code/lints; unblocks F-38 sandbox verification |
| B-02 | ADR: Backend Capability Contract & BitNet-compatible runtime adapter | issue #48 | P2 | open | Author ADR; parent of B-03..B-06 |
| B-03 | Implement `RuntimeBackendCapabilities` schema | issue #49 | P2 | open | Define schema after ADR #48 lands |
| B-04 | Add hardware profile & backend selection policy | issue #50 | P2 | open | Design policy over K8s hardware profiles (F-44) |
| B-05 | Create experimental BitNet backend adapter wrapper | issue #51 | P3 | open | Prototype behind an experimental feature flag |
| B-06 | Build benchmark harness for backend perf & wrapper overhead | issue #52 | P2 | open | Extend existing `core-runtime/benches/` (F-47) |
| B-07 | Define degraded-mode policy for constrained local inference | issue #53 | P2 | open | Specify governance + runtime behavior under resource pressure |
| B-15 | Add Rust CI workflow (fmt --check, clippy -D warnings, cargo test; ubuntu/macos/windows matrix) | research brief 2026-07-08 | P1 | open | Prerequisite for verifying B-01/#54 fix and all hardening evidence |
| B-16 | `sandbox/unix.rs` exceeds Section 4 Razor (523 lines > 250) — pre-existing debt | audit 2026-07-08 (R2) | P3 | open | Future `/qor-refactor` under its own L3 audit; out of scope for lint-only cycle |

## In-flight delivery

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-08 | Cfg-gate advanced-feature tests + clippy cleanup (COREFORGE deep-audit) | PR #47 (branch tip `b661403` on current origin/main) | P1 | in-review | Mergeable, CodeQL green. Research 2026-07-08: does NOT overlap B-01/#54 (disjoint file surfaces) — merge first |
| B-09 | Local `main` diverged from origin (ahead 1: `5d0e5a5`; behind 1: `575d703`) + 6-commit worktree branch `claude/affectionate-edison-7e6b8a` (shim/TierSynergy refactors) | local git graph | P2 | in-progress | Rebase onto origin/main; operator decides worktree-branch fate |
| B-10 | Uncommitted 193-file repo-wide `cargo fmt` sweep (+4079/−2040; `cargo fmt --check` clean) | local git status | P1 | in-progress | Commit as isolated `style:` commit after rebase; run full `cargo test` as semantic check |

## Governance backlog

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-11 | Seed scaffold-owned `docs/ARCHITECTURE_PLAN.md` (absent since bootstrap) | governance-health (Phase 109) | P2 | open | `qor-logic seed` (scaffold-owned; safe to seed) |
| B-12 | Seed scaffold-owned `docs/GOVERNANCE_INDEX.md`; restores Governance Index drift check | governance-health / governance-index (Phase 112/120) | P2 | open | `qor-logic seed`, then re-run `qor-logic governance-index` |
| B-13 | Doc drift: CLAUDE.md cites `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` which does not exist | CLAUDE.md vs `docs/architecture/` | P3 | open | Create the doc or correct the reference |
| B-14 | Deep-verify `verified` rows in `docs/FEATURE_INDEX.md` per SG-035 | docs/FEATURE_INDEX.md | P3 | open | Operator confirms each test truly exercises its feature |

## Notes

- ~~B-01 and B-08 overlap on the clippy cleanup~~ **Corrected by research
  2026-07-08** (`docs/research-brief-runtime-optimization-hardening-2026-07-08.md`):
  PR #47's branch tip `b661403` is the exact commit where COREFORGE observed the
  #54 lints — the surfaces are disjoint. Merge #47 first, then fix #54 separately.
- **B-15 (new, P1)**: no Rust CI exists (`.github/workflows/` = CodeQL only) —
  fmt/clippy/test workflow required before any hardening evidence is obtainable.
- Governance items B-11/B-12 are the remaining findings from the same
  governance-health run that produced this artifact; they are seed-repairable
  (unlike this file and `docs/FEATURE_INDEX.md`, which required reconstruction).
