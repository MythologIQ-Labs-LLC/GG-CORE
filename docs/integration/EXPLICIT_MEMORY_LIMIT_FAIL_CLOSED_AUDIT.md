# AUDIT REPORT

**Tribunal Date:** 2026-11-12
**Target:** `docs/integration/EXPLICIT_MEMORY_LIMIT_FAIL_CLOSED_PLAN.md` (sha256 `41c0d6fc0ca4d8a610531229273cdc077fee0cfb3358597abce81f5961af3157`), Rev 1 + Rev 2
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge (independent subagent, read-only)
**VERDICT:** VETO

## Executive summary
The security design holds against the source. The defect is real: `config.rs:141-142` raises the total, and that raised value becomes the Job Object cap (`sandbox_boot.rs:64` → `sandbox/windows.rs:135-137`). The R2.1 resolver closes the defect on every daemon path:
- it never raises the total;
- it rejects zero, which the sandbox would otherwise treat as unlimited;
- it reads `var_os`;
- it runs before auth, `Runtime::new`, preload, sandbox and bind.

The VETO is for three execution gaps in the plan: a projected Razor breach in `config.rs`, existing tests that the plan does not list, and a shared test lock that cannot compile.

## Passes
- **Security: PASS.**
  - Ordering is correct: `main.rs:94` runs after the FIPS self-test and before auth, `Runtime::new`, preload, sandbox and IPC.
  - `JobMemoryLimit` is `SIZE_T`, so the cap needs no conversion.
  - With a total T below 1 GiB, a model that has no working-memory estimate reserves T and is refused. That is fail-closed and not a regression.
- **Ghost paths: PASS with observations.** All three in-tree callers that read the memory env are covered. FFI and Python use `ResourceLimitsConfig::default()` (O1).
- **Razor: FAIL (V1).**
- **Dependencies: PASS.**
- **Orphans: FAIL (V2, V3).**
- **Macro architecture: PASS.**

## Violations
| ID | Category | Location | Description |
|---|---|---|---|
| V1 | Razor | `config.rs` (246 lines) | Adding `ConfigError` with its impls, `try_load`, the panicking `load` and the deprecated wrapper brings the file to about 265-275 lines. The plan does not say where these items go. |
| V2 | Orphans/Tests | `config_tests.rs:154-171`, `cli/config_cmd.rs:230-240`, `runtime_init_tests.rs:86-90` | Three existing tests contradict Revision 2 but are not listed: `"10"`/`"not-a-number"` now panic, P > T now panics, and `load_config` gets a new return type. A deprecated call inside a test fails `-D warnings`. |
| V3 | Tests/Build | `config.rs:244-246` | `crate::config::tests::ENV_LOCK` is unreachable because `mod tests` is private. A panic while the lock is held poisons it, and `.lock().unwrap()` then cascades the failure. |

## Required remediation
1. Move `ConfigError` and its impls out of `config.rs`, and state the projected line count for each file.
2. List every changed test with its new assertion, add `#[allow(deprecated)]` where a test calls the deprecated wrapper, and add a test that pins the `load()` panic contract.
3. Put the shared lock in a crate-visible `#[cfg(test)]` module, recover from poisoning with `unwrap_or_else(|e| e.into_inner())`, and test the panic with `catch_unwind` or in a child process.

## Non-blocking observations
- **O1:** The env vars govern only the standalone daemon and the config CLI; FFI and Python use the defaults. Fix the stale 2 GiB default in `docs/RELEASE_SUPPORT_AND_MAINTENANCE.md:25`.
- **O2:** Changing `run_show() -> i32` alters a public API. Keep it and add `try_show()`.
- **O3:** As planned, the deprecated `per_call_ceiling_from_env` would also panic on an invalid total. Resolve only the per-call half.
- **O4:** In the Linux default (seccomp-only) mode, the total is enforced by admission, not by the OS. Make no OS-cap claim unless `GG_CORE_SANDBOX=enforce` is set.
- **O5:** The CLI child tests must start from a cleared environment.
- **O6:** The R2.6 scope boundary is sound.

---

# AUDIT REPORT: resubmission (Revision 3)

**Tribunal Date:** 2026-11-12
**Target:** `EXPLICIT_MEMORY_LIMIT_FAIL_CLOSED_PLAN.md`, sha256 `5bc41558ae7dc5046b588c66d6b5b0bf5eb4c7f657613de173fee43fc3d0441c`, Revision 3
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge (independent subagent, read-only)
**VERDICT:** PASS, with residual conditions

## Remediation check
- **V1 (Razor), resolved.**
  - `config.rs` projects to about 240-245 lines: tight, but credible.
  - `config_memory.rs` projects to about 200 lines.
  - `cli/config_cmd.rs` projects to about 235 lines, with a named fallback if that is exceeded.
- **V2 (test inventory), resolved.** Every library test that reads or sets the memory variables was re-checked. The remaining tests are pure, or still hold under `try_load().unwrap()`. Nothing in `tests/`, `benches/` or doc comments calls `config::load` or `per_call_ceiling_from_env`.
- **V3 (shared lock), resolved.**
  - `#[cfg(test)] pub(crate) mod test_env` in `lib.rs` is reachable from both test modules.
  - `into_inner` recovery prevents a poisoned lock from cascading failures.
  - The `catch_unwind` reasoning is correct: the guard lives outside the closure.
  - `runtime_init_tests` compiles in the binary test target, so its local lock is sufficient there.
- **O1–O5:** accepted.

## Residual conditions for implementation
1. `main.rs::run_serve` is already about 62 lines. The change must not grow it: extract the config resolution and ceiling install into helpers, or record the pre-existing violation.
2. `catch_unwind` wraps only `load` or `per_call_ceiling_from_env`, and environment cleanup runs while the guard is still held.
3. Report the actual post-change line counts at substantiation. A file over 250 lines stops the work for a refactor; there is no waiver.
4. `daemon_config_requires_model_for_readiness` clears the memory variables under its lock.
5. The Revision 2 matrix and the Windows Job Object/packaged qualification remain required evidence. This PASS covers the plan only.
