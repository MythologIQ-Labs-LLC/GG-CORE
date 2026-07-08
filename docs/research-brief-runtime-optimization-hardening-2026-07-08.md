# Research Brief

**Date**: 2026-07-08
**Analyst**: The Qor-logic Analyst
**Target**: Runtime optimization + hardening surface of `core-runtime/` (session `2026-07-08T1556-3b7852`; ideation gate `.qor/gates/2026-07-08T1556-3b7852/ideation.json`)
**Scope**: (1) issue #54 clippy failures vs PR #47; (2) uncommitted working tree + branch topology; (3) performance baseline surface; (4) hardening coverage gaps F-38/F-40/F-45; (5) GitHub remediation-reporting map

---

## Executive Summary

Issue #54 and PR #47 are **complementary, not overlapping**: PR #47's branch tip (`b661403`) is the exact commit COREFORGE pinned when it observed the 23 sandbox lints, proving the PR does not fix them; its 25-file diff never touches `sandbox/unix.rs`. The "~50-file" working tree is actually a **193-file repo-wide `cargo fmt` sweep** (`cargo fmt --check` exits 0; sampled security-critical diffs are pure reformatting), and local `main` has **diverged** (ahead 1 / behind 1) from origin. The single most consequential drift found: **this repo has no Rust CI** — `.github/workflows/` contains only `codeql.yml`, so the `-D warnings` gate issue #54 describes and the evidence chain the ideation gate requires are currently unenforceable in-repo.

## Findings

### T1 — Issue #54 lint surface and PR #47 overlap

- Issue #54 (filed 2026-07-06 from COREFORGE cross-platform CI): 23 clippy `-D warnings` errors, all `#[cfg(unix)]`/`#[cfg(target_os = "linux")]`-gated in `core-runtime/src/sandbox/unix.rs` (522 lines). Verified present in the current file:
  - unused import `File` — `core-runtime/src/sandbox/unix.rs:20` (`use std::fs::{self, File, OpenOptions};`)
  - dead constant `AUDIT_ARCH_AARCH64` — `core-runtime/src/sandbox/unix.rs:98-99`
  - unconstructed struct `SeccompData` — `core-runtime/src/sandbox/unix.rs:101-110`
  - 18 dead BPF opcode constants (`LDX`…`JSET`) — lines 47–90 per issue body
  - `u64 as u64` redundant cast — ~line 180; `needless_borrow` — ~line 423
- **Issue-state pre-check**: `gh pr list --state all --search "54"` returns empty — no PR addresses it.
- **Overlap proof**: `b661403` = tip of `origin/chore/deep-audit-test-gates-clippy` (PR #47), parented directly on origin/main (`575d703`). COREFORGE observed the failures **at** `b661403` (`v0.6.5-74-gb661403`), so PR #47's clippy cleanup demonstrably does not resolve #54. PR #47's file list (25 files) contains no `sandbox/` path.
- PR #47 status: `MERGEABLE`, CodeQL `SUCCESS`, based on the current origin/main tip — fresher than its 2026-05-22 `updatedAt` suggests.
- `sandbox/unix.rs` is **not** modified in the working tree (only `sandbox/mod.rs`, formatting) — clean fix surface.
- These lints are invisible on Windows (cfg-gated), so local Windows verification of a fix is impossible; requires a Linux/macOS run.

### T2 — Working tree and branch topology

- Working tree: **193 modified files, +4079/−2040** — a repo-wide `cargo fmt` sweep. Evidence: `cargo fmt --check` RC=0 on the tree; sampled diffs in `core-runtime/src/ipc/auth.rs` (83 lines), `core-runtime/src/engine/inference.rs` (47), `core-runtime/tests/timeout_cancel_test.rs` (49), `core-runtime/tests/warmup_test.rs` (11) are all pure line-rewrap reformatting. Caveat: full semantic-equivalence across all 193 files was sampled, not machine-proven — commit as an isolated `style:` commit and run the test suite before anything else lands on top.
- Branch topology (post-fetch):
  - `main` (local): `5d0e5a5` — **ahead 1, behind 1** of origin/main (`575d703`, PR #46: drops the orphan `[[bench]] llama_cpp_comparison` declaration).
  - `claude/affectionate-edison-7e6b8a` (worktree): **6 commits** above origin/main — `5d0e5a5` (Candle ONNX embedder) plus 5 in-flight refactors: TierSynergy absorbed into `shim/` (`03c77ee`), `ServiceTier`→`SessionTier` rename (`1f6ff38`), `ModelTier` split (`735d63f`), 2 docs commits. **The F-45 shim surface is actively churning on this branch.**
  - `origin/chore/deep-audit-test-gates-clippy`: `b661403` (PR #47).
- Untracked: qor-logic host installs (`.codex/`, `.gemini/`, `.kilo/`, `.claude/` additions) and today's reconstructed governance docs — commit separately from code.

### T3 — Performance baseline surface

- `docs/BENCHMARKS.md`: Windows-only verified baseline — i7-7700K, Qwen2.5-0.5B Q4_K_M: **40 tok/s release / 21 debug, ~50 ms first-token, 435 MiB model + 6 MiB KV + 299 MiB compute**.
- Criterion suite: 10 bench files in `core-runtime/benches/`; `llama_cpp_comparison.rs` is gitignored (proprietary) and its `[[bench]]` declaration was removed at origin/main `575d703` — the local `Cargo.toml` (behind 1) still carries the orphan declaration, so **bench runs on unrebased local main may fail**; rebase before baselining.
- No CI executes benches; the baseline is manual and single-platform.

### T4 — Hardening coverage gaps

- **CI gap (critical)**: `.github/workflows/` = `codeql.yml` only. No fmt/clippy/test workflow exists. The `-D warnings` gate in issue #54 is enforced only by downstream COREFORGE CI (which has since excluded this repo's code). Every evidence item in the ideation gate that says "on Linux/macOS CI" is currently unsatisfiable in-repo.
- **F-38 sandbox**: `tests/sandbox_test.rs` + `tests/security_sandbox_escape_test.rs` exist, but the Unix path (`sandbox/unix.rs`) never compiles in local Windows dev and has no CI to exercise it.
- **F-40 python bindings**: package scaffold exists (`core-runtime/python/gg_core/`, `pyproject.toml`); zero test binding anywhere in `core-runtime/tests/`.
- **F-45 veritas shim**: no dedicated test; shim is being refactored on the worktree branch (`03c77ee`, `1f6ff38`) — adding test bindings now would collide; sequence after that branch lands.

### T5 — GitHub remediation-reporting map

| Target | Action | Rationale |
|--------|--------|-----------|
| Issue #54 | Evidence comment | Research findings: lint sites verified, no-PR-overlap proof, fix surface clean, Windows-unverifiable caveat |
| PR #47 | Triage comment | Based on current origin/main, MERGEABLE, CodeQL green, does NOT fix #54 — merge-first recommendation |
| Issue #52 | Optional note | Windows Criterion baseline exists in BENCHMARKS.md; harness need confirmed |
| Issues #48–51, #53 | None | Out of initiative scope per ideation gate non-goals |

## Blueprint Alignment

| Blueprint Claim (ARCHITECTURE_PLAN.md) | Actual Finding | Status |
|----------------------------------------|----------------|--------|
| "Linux/macOS verification only via CI" | No Rust CI exists (CodeQL only) | **DRIFT** — add fmt/clippy/test workflow |
| Evidence: "clippy -D warnings clean on Linux/macOS CI" | Unsatisfiable in-repo today | **DRIFT** — same remediation |
| ~75 integration test binaries in `tests/` | Confirmed | MATCH |
| Criterion bench suite operational | Orphan `[[bench]]` decl on unrebased local main | **DRIFT** (resolved by rebase to 575d703) |
| BACKLOG B-10 "~50 modified files" | 193 files, cargo fmt sweep | **DRIFT** — corrected in BACKLOG |
| BACKLOG B-09 "ahead 1 unpushed" | Now ahead 1 / behind 1 (diverged) + 6-commit worktree branch | **DRIFT** — corrected in BACKLOG |

## Recommendations

1. **P1 — Merge PR #47 first** (operator review + merge): freshest base, mergeable, CodeQL green; unblocks everything downstream without conflicting with the #54 fix.
2. **P1 — Add Rust CI workflow** (`fmt --check`, `clippy --all-targets -- -D warnings`, `cargo test`, OS matrix ubuntu/macos/windows): converts issue #54 from aspirational to enforced, and makes all ideation-gate evidence obtainable. Without this, a #54 fix cannot be verified from this Windows workspace.
3. **P1 — Fix issue #54** on a small branch atop post-#47 main: drop `File` import, `#[allow(dead_code)]` (or prune) the BPF opcode table + `SeccompData`, fix the two nits. Verify via the new CI.
4. **P2 — Reconcile local divergence**: rebase `5d0e5a5` onto origin/main `575d703`; decide the fate of the 6-commit worktree branch (shim refactors) with the operator.
5. **P2 — Commit the fmt sweep** as an isolated `style:` commit **after** the rebase (it overlaps PR #47's 25 files; sequencing avoids conflict), with a full `cargo test` run as the semantic-equivalence check.
6. **P2 — F-40**: add a python-bindings smoke test binding. **F-45**: defer until the worktree shim refactor lands.
7. **P3 — Re-baseline benches** post-rebase; issue #52's harness remains a separate initiative.

## Updated Knowledge

- BACKLOG.md B-08/B-09/B-10 corrected this session (overlap claim, divergence state, tree size/nature).
- Corrected assumption from ideation gate: "PR #47's cleanup overlaps issue #54" — **falsified**; they are disjoint surfaces (recorded here; assumption was non-blocking).
- New constraint for /qor-plan: no in-repo Rust CI exists; the plan must include creating it or every hardening evidence requirement fails.

---

_Research complete. Findings are advisory — implementation decisions remain with the Governor._
