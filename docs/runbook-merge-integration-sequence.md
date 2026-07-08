# Runbook: Merge / Integration Sequence (branch stack landing)

> **Audience**: an operator or a smaller model landing the in-flight branch stack
> onto `origin/main`. This is a RUNBOOK — exact branches, order, commands, and
> gotchas. All remote actions (push/PR-merge/tag) are **operator-gated**: an
> autonomous session prepares and verifies; a human approves the merge.

Snapshot date: 2026-07-08. Verify every SHA/branch with `git branch -vv` before
acting; if reality diverges, STOP and re-derive.

---

## 0. Environment gotchas (READ FIRST — these bit us repeatedly)

- **Python for qor-logic** lives in a venv, not on PATH by default. Use the
  explicit interpreter for gate/ledger scripts:
  `/d/Myth-TechForge/Alden_Calindron/venv/Scripts/python`
  The CLI is `qor-logic` (same venv `Scripts/`). Confirm with `qor-logic --version` (expect ≥ 0.119.0).
- **Gate-artifact writes require env vars**, or they raise `ProvenanceError`:
  prefix Python that calls `gate_chain.write_gate_artifact` with
  `QOR_SKILL_ACTIVE=<phase> QOR_MODEL_FAMILY=claude`. For `audit`/`substantiate`
  phases, `ai_provenance.build_manifest` needs `human_oversight` = PASS/VETO/OVERRIDE
  (not ABSENT); for `plan`/`research`/`implement`/`ideation` use ABSENT.
- **Ledger newlines**: append META_LEDGER entries with `newline="\n"` (LF). The
  repo is under autocrlf; `ledger_hash.content_hash` normalizes CRLF→LF before
  hashing, so hashes stay stable — but write LF to avoid churn.
- **Chain hash formula**: `chain = SHA256(content_hash + "|" + previous_hash)`
  (note the literal `|` separator — Phase 23 format). Seal =
  `SHA256(chain_hash + "SEALED")`. Use `qor.scripts.ledger_hash.chain_hash` /
  `content_hash`; never hand-fill hex.
- **Serialize cargo builds.** Two concurrent `cargo` invocations on the same
  `core-runtime/target/` cause Windows `Access is denied (os error 5)`. Run
  clippy and test SEQUENTIALLY, and never launch a second cargo job while one is
  running in the background.
- **Don't trust pipe exit codes.** `cargo ... | tail` masks cargo's exit status
  and reports 0. Redirect to a file and check `$?` on the bare command, or grep
  the log for `test result: FAILED` / `^error`.
- **CRLF warnings** on `git add` (`LF will be replaced by CRLF`) are benign.

---

## 1. Current branch topology (2026-07-08)

| Branch | Tip | Base | Contents |
|--------|-----|------|----------|
| `origin/main` | `575d703` | — | upstream; PR #46 (dropped orphan bench) is newest |
| `main` (local) | `354d41d` | origin/main | ahead 2: `b048869` ONNX embedder + `354d41d` cycle-1 governance |
| `chore/hardening-ci-sandbox-lints` | `d050025` | `354d41d` | cycle-1: sandbox #54 fix, Rust CI, F-40 test, **cycle-1 seal** |
| `style/cargo-fmt-sweep` | `774df09` | `354d41d` | 191-file `cargo fmt` sweep, isolated |
| `chore/hardening-cycle2-validate-clippy` | `43cc89c` | `d050025` | cycle-2: governance `bc0c70c` + code `43cc89c` (#55/#56/#57) |
| `integration/preview-cycle1` | `2014b6b` | `b661403` (PR #47 tip) | **verification preview**: PR#47 + cycle-1 + cycle-2 code. DISPOSABLE. |
| `claude/affectionate-edison-7e6b8a` | `735d63f` | origin/main | worktree: 6-commit shim/TierSynergy refactor (relates to ADR-007) |
| `origin/chore/deep-audit-test-gates-clippy` | `b661403` | origin/main | **PR #47** |
| `origin/docs/adr-007-tiersynergy-dspark` | — | origin/main | **PR #59** (ADR-007 doc) |

`main` is `ahead 2, behind 0` of origin/main. The preview branch is throwaway —
never merge it; it only proves the code compiles/tests green on the PR-#47 base.

---

## 2. Open PRs / blocking issues

- **PR #47** (`chore/deep-audit-test-gates-clippy`): mergeable, CodeQL green. Does
  NOT fix #54 (disjoint). **Merge FIRST** — it is the base every hardening fix and
  the new CI gate assume.
- **PR #59** (`docs/adr-007-tiersynergy-dspark`): ADR-007 doc only. Merge when the
  epic is accepted; low risk (docs). Independent of the hardening stack.
- **Green-CI blockers** (all reproduced on bare `b661403`, pre-existing):
  - #55 gguf `validate_path` existence tests — FIXED in cycle-2 (`43cc89c`).
  - #56 13 residual clippy errors — FIXED in cycle-2 (`43cc89c`).
  - #57 NUL-byte validation — FIXED in cycle-2 (`43cc89c`).
  - #58 KV cross-sequence leak — NOT fixed (B-20 redesign; `kv_cache_test` stays 13/14).
  - **NEW, uncatalogued (found 2026-07-08, needs an issue)**: `cargo clippy
    --all-targets` still errors after cycle-2 with a non-exhaustive match on
    `FinishReason::Cancelled` at `core-runtime/tests/bluegreen_state_rollback_test.rs:94`
    and `core-runtime/benches/generation_throughput.rs:65` (E0004), plus one
    residual field-assign. These are OUTSIDE #56's 13 and block a clean
    `--all-targets` clippy leg. File as a new issue; small mechanical fix
    (add the `FinishReason::Cancelled` arm / `_ =>` in both matches).

---

## 3. Target landing order (operator-gated)

Each arrow is a checkpoint: verify green, then a human approves the next step.

```
1. Merge PR #47            → origin/main = b661403-equivalent
2. Rebase `main` (b048869 + 354d41d cycle-1 governance) onto new origin/main
3. Rebase `chore/hardening-ci-sandbox-lints` (cycle-1 code+seal) onto main; open PR; merge
4. Fix the NEW FinishReason::Cancelled errors (§2) — tiny branch; needed for green clippy leg
5. Rebase `chore/hardening-cycle2-validate-clippy` (#55/#56/#57) onto main; open PR; merge
6. Rebase `style/cargo-fmt-sweep` onto main LAST (it touches 191 files; rebasing
   it before the others maximizes conflicts). Merge.
7. Merge PR #59 (ADR-007 doc) whenever the epic is accepted — order-independent.
8. Decide the fate of worktree `claude/affectionate-edison-7e6b8a` (shim/TierSynergy
   refactor) WITH the operator — it overlaps ADR-007 #64/#68; likely folds into that epic.
```

After step 6, run the full gate on the merged main: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` — expect
green EXCEPT `kv_cache_test` (13/14, until B-20 lands).

**Why fmt sweep last**: it reformats nearly every file; landing it before the
targeted fixes turns every fix into a merge conflict. Land semantic changes
first, formatting last.

---

## 4. Finishing / sealing cycle 2 (loose end)

Cycle-2 code (`43cc89c`) and governance (`bc0c70c`) are committed on
`chore/hardening-cycle2-validate-clippy` but the session was NOT sealed
(no `/qor-substantiate` Entry #85). Verification result to record in the seal:

- On `integration/preview-cycle1` (PR#47 + cycle-1 + cycle-2): all 13 `#56`
  clippy sites fixed (clippy error count 15→2); the 2 remaining are the
  uncatalogued `FinishReason::Cancelled` errors from §2 (out of cycle-2 scope).
- `validate_path` NUL rejection + gguf existence-contract tests: implemented.
- Full-suite: expect green except `kv_cache_test` (13/14, #58/B-20) and blocked
  only by the §2 clippy errors on the `--all-targets` leg.

To seal (Judge phase, session `2026-07-08T1651-6c68b6`): append META_LEDGER
Entry #85 (SESSION SEAL) chaining from Entry #84's chain hash; content_hash over
`SYSTEM_STATE.md` + the cycle-2 code files; write
`.qor/gates/2026-07-08T1651-6c68b6/substantiate.json`
(`human_oversight=PASS`); run `qor-logic verify-ledger`,
`qor-logic reliability seal_entry_check`, then commit with the full attribution
trailer (`qor.scripts.attribution.commit_trailer("Claude Opus 4.8 (1M context)")`)
and rotate the session (`session.rotate()`). Follow cycle-1's Entry #81 seal
(in META_LEDGER) as the template. Honestly record the disclosed SKIPs (intent_lock
absent, version/tag/push deferred per Review Boundary).

---

## 5. Verification command (canonical, serialized)

Run from `core-runtime/`, one at a time, checking each `$?`:

```bash
cargo fmt --check                                   # formatting gate
cargo clippy --all-targets -- -D warnings           # lint gate (fix §2 errors first)
cargo test --workspace --no-fail-fast               # full suite; expect kv_cache 13/14 pre-B-20
```

Do NOT run these in parallel or in the background alongside another cargo job
(os error 5). To capture output: `cargo test --workspace --no-fail-fast > /tmp/t.log 2>&1; echo $?`.

---

## 6. Standing boundaries (all cycles)

- No push / PR-merge / tag / version bump without explicit operator approval
  (Review Boundary). Local commits + local verification only.
- Never close a GitHub issue from an agent session — comment evidence; operator closes.
- Never force-push shared branches. Verify branch lineage vs `origin/main` before any PR.
- Keep governance artifacts (docs/gates/ledger) in commits SEPARATE from code.
