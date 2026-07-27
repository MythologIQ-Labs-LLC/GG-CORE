# AUDIT REPORT — Gate Tribunal (pyo3 0.21→0.29 migration)

**Target**: docs/plan-pyo3-migration-2026-07-26.md (iteration 1)
**Date**: 2026-07-26
**Session**: 2026-07-26T2010-pyo3
**Risk Grade**: L2
**Mode**: adversarial (independent fresh-context Judge subagent)

## VERDICT: PASS

- Infrastructure verified: Cargo.toml pyo3 0.21 + pyo3-asyncio-0-21 (features
  extension-module/abi3-py38/tokio-runtime); async site session.rs:194;
  InferenceParams `#[pyclass] #[derive(Clone)]` extracted by value in
  AsyncSession::infer (session.rs:184) → 0.27 `from_py_object` genuinely needed.
- **Sync audit clean**: all 9 pyclasses hold only Arc/plain/Option/bool (+
  SessionToken); no RefCell/Cell/Rc → no 0.23 Sync break.
- **No missed breaking change**: no `Python::with_gil`/`allow_threads`, no custom
  IntoPy/ToPyObject/FromPyObject, no pyo3 `.downcast()`, no `From<Utf8Error>`, no
  `Py::clone`. Code already on modern Bound/`#[pymodule]` idioms → 0.22 GIL
  refactor is a no-op → migration is nearly mechanical.
- MSRV 1.83 satisfied (CI @stable, no rust-toolchain pin). maturin
  `>=1.0,<2.0` compatible with pyo3 0.29. abi3-py38 preserved.
- LD-5 compiler-driven residual approach is sound L2 governance; DoD rests on
  real CI gates (python feature build+clippy+test) + default workspace.

### Advisories (non-blocking)
- LD-4 could name `SessionToken` explicitly (Sync not in doubt).
- The by-value `from_py_object` trigger is AsyncSession::infer (not Session::infer,
  which takes it by ref); LD-5 compiler-driven catch covers it regardless.
- Pre-existing Cargo 0.8.1 vs pyproject 0.7.0 version drift — out of scope.

### Next action
`/qor-implement` authorized under LD-5 (apply pyo3 v0.29 migration-guide fix per
diagnostic; cite guide; no guessing). Commit locally; push/PR at operator direction.
