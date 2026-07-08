# Plan: Runtime Hardening Cycle 1 — CI gate, sandbox lints, tree reconciliation

**change_class**: feature
**doc_tier**: standard
**high_risk_target**: false (infrastructure runtime, not an EU AI Act Annex III decision system; all changes are behavior-preserving: lint hygiene, CI scaffolding, test additions, git topology)

**Session**: 2026-07-08T1556-3b7852 (ideation + research gates sealed; brief: `docs/research-brief-runtime-optimization-hardening-2026-07-08.md`)

**boundaries**:
- limitations: Windows dev host — cfg(unix) lint fixes are compile-verified only via the new CI after operator push; no remote mutation this cycle (Review Boundary).
- non_goals: PR #47 merge (operator), worktree-branch fate (operator), backend-capability epic #48–52, bench re-baselining, README.md rework (pre-existing operator edit, stashed and restored untouched).
- exclusions: no changes under `ipc/` or `security/`; `sandbox/unix.rs` edits are lint-only (no control-flow, no constant-value, no syscall-filter changes).

## Open Questions

None blocking. Deferred to handoff: (a) when to flip FEATURE_INDEX F-38 to a clean `verified` (requires green Linux CI run after push); (b) whether `.claude/.codex/.gemini/.kilo` host installs should be committed (left untracked).

## Phase 0: Tree & Branch Reconciliation (operational, no code edits)

### Affected Files
- (git topology only)

### Changes
Locked sequence (each step verified before the next):
1. On `main` (at `5d0e5a5`): commit governance batch — `docs/*.md` (BACKLOG, FEATURE_INDEX, ARCHITECTURE_PLAN, GOVERNANCE_INDEX, META_LEDGER Entry #80, research brief), `.gitignore` (seed marker block), `.qor/gates/2026-07-08T1556-3b7852/*.json`, `.agent/staging/.gitkeep`, `.qor/gates/.gitkeep`.
2. `git switch -c style/cargo-fmt-sweep`; stage all modified `*.rs` (192 files, `cargo fmt --check` clean); commit `style: apply cargo fmt across core-runtime`.
3. `git stash push -m "operator README rework (out of scope)" -- README.md`.
4. `git switch main && git rebase origin/main` → main = `575d703` + `5d0e5a5'` + governance commit. LD-grep evidence: `git log --oneline -1 origin/main` → `575d703 fix(core-runtime): drop orphan [[bench]] llama_cpp_comparison declaration (#45) (#46)`; only `Cargo.toml` touched upstream, no path overlap with either replayed commit except none.
5. `git rebase main style/cargo-fmt-sweep` (fmt commit replays clean: touches only `*.rs`, upstream delta is `Cargo.toml`).
6. `git switch main && git stash pop` (README rework restored, left uncommitted).
7. `git switch -c chore/hardening-ci-sandbox-lints main` — implementation branch.

### Unit Tests
- (none — verified by `git status`/`git log` postconditions: main ahead 2 / behind 0; style branch = main+1; README modification present and uncommitted)

## Phase 1: Rust CI workflow (BACKLOG B-15)

### Affected Files
- `.github/workflows/rust.yml` — NEW: fmt + clippy + test gate

### Changes
Single workflow, two jobs, default features only (backends `gguf`/`onnx`/`cuda`/`metal`/`python` need external toolchains — out of CI scope v1):
- `lint`: matrix `{ubuntu-latest, macos-latest, windows-latest}` → `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`. The ubuntu + macos legs are exactly the gate issue #54 describes.
- `test`: same matrix → `cargo test --workspace`.
- Actions: `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`. Working directory `core-runtime/`.

### Unit Tests
- (D4.d waiver — GitHub Actions cannot execute locally; YAML validated by parse + the identical commands run locally on Windows. **Follow-up phase**: first operator push triggers the live run.)

## Phase 2: Issue #54 — sandbox/unix.rs lint fixes (lint-only, behavior-preserving)

### Affected Files
- `core-runtime/tests/sandbox_test.rs` — unchanged; regression oracle (listed first per TDD)
- `core-runtime/tests/security_sandbox_escape_test.rs` — unchanged; regression oracle
- `core-runtime/src/sandbox/unix.rs` — lint fixes only

### Changes
Exactly the four fix classes from issue #54 (all sites re-verified in current source; file untouched by any in-flight work):
1. `unix.rs:21` — drop `File` from `use std::fs::{self, File, OpenOptions};` (unused). [audit R1: line corrected 20→21]
2. BPF opcode reference tables (`mod bpf`/`bpf_size`/`bpf_mode`/`bpf_src`/`bpf_jmp`, lines 44–91), `AUDIT_ARCH_AARCH64` (:98–99), `struct SeccompData` (:101–110) — add `#[allow(dead_code)]` with a one-line comment (deliberately complete seccomp/BPF reference table kept for filter maintenance). Constant values untouched.
3. `unix.rs:180` — remove the redundant same-type cast in `(self.config.max_cpu_time_ms as u64) * 1000` (field already `u64`; verify with `grep -n "max_cpu_time_ms" core-runtime/src/sandbox/mod.rs` at implement time; if the field is *not* u64 the cast stays and the clippy suggestion is followed verbatim instead).
4. `unix.rs:423` — remove the needless borrow in `&[("error", &e)]` per clippy's exact suggestion.

### Unit Tests
- `core-runtime/tests/sandbox_test.rs` — existing suite passes unchanged (behavior preservation); on Windows this exercises the cross-platform surface; the unix path is compile-gated to the CI legs from Phase 1.

## Phase 3: F-40 python-bindings test binding

### Affected Files
- `core-runtime/tests/python_binding_test.rs` — NEW (listed first per TDD)
- `docs/FEATURE_INDEX.md` — F-40 row: test path bound; F-38 row: note lint fix pending CI verification

### Changes
New integration test, `#![cfg(feature = "python")]`, no Python interpreter embedding:
- `default_params_convert_losslessly`: `InferenceParams::default()` → assert documented defaults (256, 0.7, 0.9, 40, false, None) → `engine::InferenceParams::from(&py_params)` → assert every field survives conversion including the `u32→usize` widenings. This is the behavior seam between the Python surface and the engine (`core-runtime/src/python/inference.rs:83-95`).
- `result_roundtrip_preserves_output`: construct `engine::InferenceResult` → `python::InferenceResult::from` → assert `output`/`tokens_generated`/`finished` preserved and `__len__` == `tokens_generated`.

FEATURE_INDEX updates in the same commit (Phase 73 obligation).

### Unit Tests
- `core-runtime/tests/python_binding_test.rs` — both tests above; run with the python feature enabled where a Python toolchain exists (see CI Commands proxy); compile-gated otherwise (binding declared in FEATURE_INDEX with that caveat).

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|----------|-----------|-----------|-----------------|
| F-38 | MODIFIED | core-runtime/tests/sandbox_test.rs | sandbox config/apply surface behaves identically after lint-only fixes (suite passes unchanged) |
| F-40 | MODIFIED | core-runtime/tests/python_binding_test.rs | InferenceParams::default() converts losslessly into engine params; result roundtrip preserves output fields |

## Definition of Done

### Deliverable: Rust CI gate
- **D1**: repo enforces fmt/clippy(-D warnings)/test on 3 OSes so #54-class drift cannot land silently.
- **D2**: `.github/workflows/rust.yml`, two jobs, matrix as specified, `working-directory: core-runtime`.
- **D3**: BACKLOG B-15 → in-progress with commit ref; ledger entry at substantiate.
- **D4.d**: waiver — Actions run requires push (Review Boundary). **Follow-up phase**: operator push; identical commands executed locally on Windows as proxy.

### Deliverable: sandbox lint fix
- **D1**: `cargo clippy -- -D warnings` compiles clean on Linux/macOS (issue #54 closed by operator once CI proves it).
- **D2**: `sandbox/unix.rs` — import pruned, `#[allow(dead_code)]` on reference tables, two nit fixes; zero behavior delta.
- **D3**: FEATURE_INDEX F-38 note updated; evidence comment on #54 drafted for operator.
- **D4**: `cargo test --workspace` (Windows) — sandbox + security suites pass unchanged; Linux/macOS leg via CI follow-up.

### Deliverable: F-40 binding
- **D1**: python bindings surface has a behavior-asserting test binding (FEATURE_INDEX gap closed).
- **D2**: `core-runtime/tests/python_binding_test.rs`, cfg-gated on `python` feature.
- **D3**: FEATURE_INDEX F-40 row updated in same commit.
- **D4**: both tests run green with the python feature enabled where a Python toolchain exists; `cargo check --features python --tests` as local proxy if the pyo3 build is unavailable on this host.

### Deliverable: tree reconciliation
- **D1**: main linear on origin/main; fmt sweep isolated and committed; no work lost.
- **D2**: branch topology per Phase 0 step 7 postconditions.
- **D3**: BACKLOG B-09/B-10 updated to done-pending-push.
- **D4**: `git status -sb` shows `ahead 2` (no behind); `cargo test --workspace` green on `style/cargo-fmt-sweep` (semantic-equivalence check for the fmt sweep).

## CI Commands

- `cargo fmt --check` — formatting gate (run in `core-runtime/`)
- `cargo clippy --all-targets -- -D warnings` — lint gate (Windows leg locally; unix legs via CI)
- `cargo test --workspace` — full test suite, default features
- `cargo check --features python --tests` — F-40 binding compiles (proxy where no Python toolchain)
