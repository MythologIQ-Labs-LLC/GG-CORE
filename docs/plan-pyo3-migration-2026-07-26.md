# Plan: pyo3 0.21 → 0.29 migration (clears RUSTSEC CVEs)

**change_class**: feature
**doc_tier**: standard
**iteration**: 1
**risk_grade**: L2 (security dependency migration + mechanical binding API
updates on the optional `python` surface; no auth/security-logic code change;
python bindings are gated behind the `python` feature that CI now builds)
**high_risk_target**: false
**originating_research**: this session's pyo3-migration research (external
migration spec + internal call-site inventory; research gate artifact
2026-07-26T2010-pyo3/research-iter1.json).

**terms_introduced**: none new.

**boundaries**:
- limitations:
  - The migration is **compiler-driven**: apply the known change table, then
    resolve every remaining `cargo build --features python` / clippy error using
    the official pyo3 v0.29 migration guide's documented fix for that exact
    diagnostic (per Shadow Genome #3 — derive the map from emitted output, never
    guess). Behavior is preserved; only the pyo3 API surface changes.
  - No new end-to-end Python-interpreter test (the existing binding test is
    conversion-only; a live-interpreter test is a separate gap, unchanged by
    this migration). The gate is: python feature builds + existing tests pass +
    clippy `-D warnings` clean under the feature.
- non_goals:
  - No change to the Rust engine/runtime logic; no change to the binding
    behavior/semantics; no new Python API.
  - No `rand` migration (separate cycle).
- exclusions:
  - No `security/`, `engine/`, `ffi/`, or scheduler changes.

## Open Questions

None blocking. Target + surface verified: pyo3 0.29.0 is the first release
fixing RUSTSEC-2026-0176 (high, PyList/PyTuple iter OOB) + RUSTSEC-2026-0177
(medium, PyCFunction Sync) — 0.24.1 fixed RUSTSEC-2025-0020 (low). Async
successor is `pyo3-async-runtimes` 0.29 (`tokio::future_into_py`, same shape).
`extension-module`/`abi3-py38` unchanged. MSRV 1.83 (CI stable satisfies).

## Design summary

Bump pyo3 0.21 → 0.29 and replace `pyo3-asyncio-0-21` with `pyo3-async-runtimes`
0.29, clearing the three flagged advisories. Update the one async call site path
and apply the two documented 0.21→0.29 breaking changes that touch our bindings
(`from_py_object` opt-in on the arg-extracted `InferenceParams`; verify pyclass
`Sync`). Resolve any residual compiler diagnostics via the official migration
guide. The `python` CI leg (added in B-25) verifies the result.

## Locked Decisions

- **LD-1 — Pin pyo3 0.29 + pyo3-async-runtimes 0.29.** Cargo.toml:112-113,135:
  `pyo3 = { version = "0.29", features = ["extension-module","abi3-py38"],
  optional = true }`; replace `pyo3-asyncio-0-21` with
  `pyo3-async-runtimes = { version = "0.29", features = ["tokio-runtime"],
  optional = true }`; `python = ["pyo3","pyo3-async-runtimes"]`. 0.29 is the only
  target clearing the high+medium CVEs.
- **LD-2 — Async path rename.** `session.rs:194`
  `pyo3_asyncio_0_21::tokio::future_into_py` → `pyo3_async_runtimes::tokio::future_into_py`
  (identical signature; `T: IntoPyObject` is satisfied by the returned pyclass
  `InferenceResult`).
- **LD-3 — `from_py_object` opt-in on arg-extracted Clone pyclasses (0.27).**
  `InferenceParams` (`inference.rs:18`, `#[pyclass] #[derive(Clone)]`) is
  extracted as a function argument (`Session::infer`/`AsyncSession::infer`
  `params`), so under 0.27+ it needs `#[pyclass(from_py_object)]`. Apply it there
  and to any other Clone pyclass the compiler flags as an argument.
- **LD-4 — `Sync` pyclass audit (0.23).** The 9 pyclasses hold only
  `Arc<..>`/plain/`Option`/`bool` fields (no `RefCell`/`Cell` per the
  inventory), so `Sync` is expected to hold; if the compiler flags any, replace
  interior mutability with `Mutex` (guide 0.22→0.23). No violations anticipated.
- **LD-5 — Compiler-driven residuals.** After LD-1..4, run
  `cargo build --features python` then `cargo clippy --features python
  --all-targets -- -D warnings`; for each remaining error, apply the pyo3 v0.29
  migration-guide fix for that exact diagnostic. Do not guess; cite the guide
  section in the commit. `create_exception!`, `*::new_err`, and the
  `#[pymodule]`/`add_class` registration are documented as unchanged.

## Phase 1: Dependency bump + async path

### Affected Files

- `core-runtime/Cargo.toml` — pyo3 0.29, pyo3-async-runtimes 0.29, `python`
  feature (LD-1)
- `core-runtime/Cargo.lock` — updated by `cargo update`
- `core-runtime/src/python/session.rs` — async call-site path (LD-2)

### Changes

Dep + feature swap; one-line async path change. `cargo update` resolves the new
pyo3 tree.

## Phase 2: Binding API adjustments (0.21→0.29 breaking changes)

### Affected Files

- `core-runtime/src/python/inference.rs` — `#[pyclass(from_py_object)]` on
  `InferenceParams` (LD-3)
- `core-runtime/src/python/*.rs` — any residual compiler-flagged fixes (LD-4/5;
  Sync, signatures, conversions) — exact set from `cargo build --features python`

### Changes

Apply the documented change table; iterate on compiler output until the python
feature builds and lints clean.

### Tests

- Existing `tests/python_binding_test.rs` (conversion round-trips) must still
  pass under pyo3 0.29 — the migration must not change conversion behavior.

## Phase 3: Verify + governance

### Affected Files

- `docs/FEATURE_INDEX.md` — F-40 (python): note the pyo3 0.29 migration cleared
  the RUSTSEC advisories; python leg still green
- `docs/BACKLOG.md` — mark the pyo3 Dependabot migration done (clears
  RUSTSEC-2026-0176/0177/2025-0020); note `rand` migration remains
- `docs/ARCHITECTURE_PLAN.md` — dependency note: python bindings on pyo3 0.29 /
  pyo3-async-runtimes

### Verify (the gate)

`cargo build --features python`; `cargo clippy --features python --all-targets
-- -D warnings`; `cargo test --features python`; default `cargo test --workspace`
+ `clippy --all-targets -- -D warnings` + `fmt --check`.

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| F-40 | MODIFIED | core-runtime/tests/python_binding_test.rs | Python bindings build + conversion tests pass on pyo3 0.29 (RUSTSEC CVEs cleared); fails if the migration breaks the binding surface |

## Definition of Done

### Deliverable: pyo3 0.29 migration
- **D1**: The python bindings compile and pass their tests on pyo3 0.29 /
  pyo3-async-runtimes 0.29; the three RUSTSEC advisories are cleared.
- **D2**: Cargo.toml/lock pinned to 0.29; `future_into_py` path updated;
  `InferenceParams` `from_py_object` opt-in; any compiler-flagged residuals
  fixed per the migration guide.
- **D3**: FEATURE_INDEX F-40 note; BACKLOG pyo3-Dependabot done; ARCHITECTURE_PLAN
  dependency note.
- **D4**: `cargo build --features python` + `cargo clippy --features python
  --all-targets -- -D warnings` + `cargo test --features python` all pass;
  default workspace test + clippy + fmt clean.

## CI Commands

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings` (default)
- `cargo test --workspace` (default)
- python feature leg (CI matrix + local): build, clippy `--all-targets -- -D
  warnings`, and the test suite under the `python` feature.
