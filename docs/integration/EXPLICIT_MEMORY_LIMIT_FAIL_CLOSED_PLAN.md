# #182: Fail-closed explicit memory limits (implementation plan, review required)

Status: **DRAFT PLAN, NOT AUDITED OR IMPLEMENTED**. Risk L2. Exact inspected base: `main@b06aaa04b8d0cf60c9480b626ad8502d392a2428`, 2026-11-10.

Owner's active delivery scope is **GG-CORE standalone inference runtime**. Do not modify, build, install, or manage CoreForge. Do not alter the published GG-CORE `v0.10.0-rc.1` or frozen N7/C2 candidate artifact. This plan is independent of issue #184.

## Grounded defect and secondary finding

- `core-runtime/src/config.rs::load_resource_limits` defaults `GG_CORE_MAX_MEMORY_PER_CALL` to 1 GiB and sets `total = total.max(per_call)`. An operator who explicitly sets `GG_CORE_MAX_TOTAL_MEMORY=268435456` (256 MiB), leaving per-call unset, receives **1 GiB** total and Windows Job Object caps, not 256 MiB.
- Explicit numeric or malformed per-call settings can be silently floored/fallback parsed by `per_call_ceiling_from_env`; invalid explicit values return `None` (thus no explicit per-call ceiling).
- `core-runtime/src/cli/config_cmd.rs::run_validate` compares the **already elevated** effective total to per-call. Worse, the test `test_validate_fails_when_total_lt_per_call` explicitly expects `run_validate() == 0` for incompatible explicit total/per-call values, explaining why CI didn't catch the defect. Correct that regression.
- `core-runtime/src/main.rs::run_serve` currently calls `runtime_init::load_config` and then applies the process sandbox, with no strict pre-sandbox numeric memory-limit admission. The effective total flows into `core-runtime/src/sandbox_boot.rs::daemon_sandbox_config` and into Windows Job Object memory governance.

## Required behavior and decisions to audit

1. **Explicit total is an upper bound, never a suggestion.** A syntactically valid `GG_CORE_MAX_TOTAL_MEMORY` must never be silently increased, including when `GG_CORE_MAX_MEMORY_PER_CALL` is unset. Reject explicit total zero, non-decimal, overflow and any documented unsafe minimum instead of converting them into a permissive default or floor. The exact minimum, if any, must be justified by real process/Job Object behavior rather than assumption.
2. When total is explicitly set but per-call is unset, keep total unchanged and set the internal **fallback reservation** to `min(1 GiB, total)`. Per-call remains **no additional operator ceiling** (`per_call_ceiling=None`); actual model residency/working-memory admission must still respect total.
3. When both are explicitly set, accept positive valid values with per-call ≤ total; if per-call > total, **abort standalone daemon startup with a clear actionable configuration error**. Do not silently raise total or silently lower an explicitly requested per-call limit. Likewise fail startup on malformed/zero explicit per-call values rather than mislabeling them as unset. If a product compatibility decision calls for an alternative, record an explicit owner disposition before implementation.
4. When total is absent but per-call explicit is valid, retain the existing host-derived default total unless per-call is larger, in which case **fail startup clearly rather than changing a total limit without acknowledgement**; document the exact decision. Treat the no-explicit-total case separately from the explicit-total ceiling to avoid misleading operators.
5. Preserve current `ResourceLimitsConfig` public struct layout and other embedded APIs where possible. Do not invent an infallible startup success: add a strict validation or fallible config-resolution path used **before runtime allocation/model preload/sandbox installation**. Keep `config show` coherent with effective values, and `config validate` non-zero for invalid explicit settings. Record any unavoidable compatibility adjustments.
6. Do not generalize this change to all environment variables without a separate issue and risk assessment. However, explain the impact of permissive parsing in the memory-related settings that this issue actually covers.

## Implementation surfaces

- `core-runtime/src/config.rs`: one canonical explicit/missing/invalid memory limit resolver; consistent effective total, per-call fallback and explicit ceiling; no unchecked raising. Prefer a pure parser with inputs (raw total/per-call Option) for deterministic tests.
- `core-runtime/src/main.rs` and/or `core-runtime/src/runtime_init.rs`: invoke strict validation before preload and sandbox. Return non-zero with a concrete error and no IPC listener when invalid.
- `core-runtime/src/cli/config_cmd.rs`: make `show` and `validate` report the same semantics. Fix misleading tests that intentionally accepted silently raised totals. Preserve `defaults` semantics.
- `core-runtime/src/config_tests.rs`, `core-runtime/src/cli/config_cmd.rs` tests: test a pure parser, plus process-isolated CLI integration for env behavior. Existing `ENV_LOCK` mutexes are **module-local**, not a single shared lock; avoid a new cross-module environment race.
- `core-runtime/src/sandbox_boot.rs` and `core-runtime/src/sandbox/windows.rs`: verify configured `max_memory_bytes` matches the **unchanged explicit total** before applying the Windows Job Object. Check conversion/overflow/platform-specific failure paths.
- `SECURITY.md`, `docs/USAGE_GUIDE.md`, README configuration reference: document fail-loud invalid values, optional per-call ceiling versus fallback reservation, and actual OS containment distinctions.

## Required verification matrix

| total setting | per-call setting | expected |
| --- | --- | --- |
| absent | absent | existing host default, fallback 1 GiB, no explicit per-call ceiling |
| 256 MiB | absent | **exactly 256 MiB** effective total, fallback ≤256 MiB; model larger than cap refused |
| 2 GiB | 128 MiB | exact explicit values and explicit per-call ceiling |
| 256 MiB | 512 MiB | startup refusal, no IPC listener or model preload |
| absent | explicitly above host default | startup refusal with actionable operator configuration error unless separately approved |
| 0, malformed, overflow | absent | startup refusal, no silent fallback |
| absent | 0, malformed, overflow | startup refusal, no silent fallback |
| 2 GiB | 2 GiB | admitted if model and working memory fit; no off-by-one cap inflation |
| 1 MiB or documented minimum | absent | documented exact-limit behavior, no zero=unlimited interpretation |

Also cover Linux and Windows numeric-width behavior, model-load denial, recovery after **request** refusal (as distinct from expected startup refusal), and sanitized error/log content. Exercise `config show`/`config validate` as child processes with independent env vars. Confirm Job Object cap and actual process memory limit on a disposable offline Windows VM with a real, pinned GGUF model; inspect the exact packaged binary, not only source tests. Include tamper/authorization, default configuration and resource-concurrency regressions.

## Governance and integration gates

1. **Before source changes:** review plan using the required Qortara `/ql-audit` lifecycle. Record PASS or VETO receipt and approved implementation tree scope. This file does not attest an audit.
2. Implement on a separate `fix/gg-core-182-memory-ceiling-...-no-ci` branch off a verified compatible GG-CORE base. No destructive force-push, no release/pin changes, and no mixing with the N7 latency patch. The N7 Dev artifact remains byte-identical.
3. Run fmt, strict Clippy, full Rust feature/test matrix, real packaged Windows and Linux security/resource tests, and external API compatibility. Independent review must compare the tested SHA and full changed file list.
4. Post before/after numeric traces and exact source/CLI/DLL/archive hashes to #182. Bring into the integrated `v1.0.0` line only through standard reviewed/protected-branch gates and explicit owner release authorization. A green CI result alone is not real process/Job Object qualification.

**Success criterion:** no standalone GG-CORE execution path can quietly apply more memory authority than an explicit, valid operator limit requested, and invalid settings fail clearly *before* work begins.

---

## Revision 2: concrete decisions for audit (2026-11-12)

Base: `main@b06aaa04`. Branch `fix/gg-core-182-memory-ceiling-20261112-no-ci`. Revision 1 above left several behaviours open ("unless separately approved", "documented minimum"). This revision fixes one behaviour for each so the plan, not the implementation, is the source of truth.

### R2.1 Pure resolver (`core-runtime/src/config_memory.rs`, new)

```rust
pub const MIN_EXPLICIT_MEMORY: usize = 1 << 20;        // 1 MiB
pub const DEFAULT_PER_CALL_FALLBACK: usize = 1 << 30;  // 1 GiB
pub struct MemoryLimits { pub total: usize, pub per_call_fallback: usize, pub per_call_ceiling: Option<usize> }
#[non_exhaustive] pub enum MemoryConfigError {
    Invalid { var: &'static str, reason: &'static str },          // empty, non-digit, non-UTF-8, overflow
    BelowMinimum { var: &'static str, value: usize, min: usize }, // includes 0
    PerCallExceedsTotal { per_call: usize, total: usize, total_explicit: bool },
}
pub fn resolve(raw_total: Option<&OsStr>, raw_per_call: Option<&OsStr>, host_default: usize)
    -> Result<MemoryLimits, MemoryConfigError>;
pub fn from_env() -> Result<MemoryLimits, MemoryConfigError>; // reads var_os, host::default_total_memory()
```

Parsing of an explicitly present value: UTF-8, non-empty, ASCII digits only (no sign, whitespace, or unit suffix), fits `usize`, and `>= MIN_EXPLICIT_MEMORY`. Anything else is an error; nothing is floored, defaulted or clamped. `0` is rejected (`BelowMinimum`) because the sandbox treats `max_memory_bytes == 0` as "no Job Object / cgroup memory limit" (`sandbox/windows.rs:135`, `unix_cgroup.rs:38`), so zero would otherwise mean unlimited.

The 1 MiB minimum is a syntax/sanity floor, not a claim that the daemon can serve in 1 MiB: a real model will be refused by residency admission (`ModelLifecycle` → `Insufficient memory budget`) and the operator sees that error. It is the existing per-call floor (`config.rs:141`), now applied by refusal instead of silent raise.

| total | per-call | effective total | fallback reservation | per-call ceiling |
|---|---|---|---|---|
| absent | absent | host default (max(2 GiB, 50% RAM)) | min(1 GiB, total) | None |
| T valid | absent | **T exactly** | min(1 GiB, T) | None |
| T valid | P valid, P ≤ T | T | P | Some(P) |
| T valid | P valid, P > T | **error** `PerCallExceedsTotal{total_explicit:true}` | – | – |
| absent | P valid, P ≤ host default | host default | P | Some(P) |
| absent | P valid, P > host default | **error** `PerCallExceedsTotal{total_explicit:false}` (message tells the operator to set `GG_CORE_MAX_TOTAL_MEMORY`) | – | – |
| invalid/0/overflow/empty | any | **error** | – | – |
| any | invalid/0/overflow/empty | **error** | – | – |

Total is never raised on any path. An explicit per-call is never lowered. An empty value (`VAR=` on Unix) is invalid, not "unset"; on Windows `set VAR=` removes the variable, so it is genuinely absent.

### R2.2 Public Rust API

- `ResourceLimitsConfig`, `EnvConfig`, `EffectiveConfig` layouts unchanged. `max_memory_per_call` = fallback reservation, `max_total_memory` = effective total, `EnvConfig::per_call_ceiling` = ceiling.
- New `config::try_load() -> Result<EnvConfig, ConfigError>`; `ConfigError` is `#[non_exhaustive] enum { Memory(MemoryConfigError) }` with `Display` + `std::error::Error`.
- `config::load()` keeps its signature. It now calls `try_load()` and **panics** with the error message when memory settings are invalid. Rationale: the infallible signature cannot represent "the operator limit is invalid" without inventing a limit, and a silent substitute is exactly the defect. The doc comment states the panic and points to `try_load()`. All in-tree callers move to `try_load()`; `load()` remains for external compatibility. Non-memory variables keep their existing lenient parsing (R2.6).
- `config::per_call_ceiling_from_env()` becomes `#[deprecated]`, implemented via the resolver, with the same panic rule as `load()` for invalid memory settings (it previously returned `None`, i.e. no ceiling, for an invalid explicit value). No in-tree non-test callers remain.

### R2.3 Daemon (`main.rs`, `runtime_init.rs`)

- `runtime_init::load_config()` → `Result<(RuntimeConfig, Option<usize>), gg_core::config::ConfigError>` (binary-internal). `run_serve` matches it immediately after the FIPS self-test and **before** auth policy, `Runtime::new`, preload, sandbox and IPC bind. On `Err`: `serve: invalid configuration: <msg>` on stderr, `ExitCode::FAILURE`. No listener, no model load.
- The per-call ceiling installed on `runtime.resource_limits` comes from the same resolved value (one parse, one source).

### R2.4 CLI (`cli/config_cmd.rs`)

- `run_show() -> i32`: prints effective values from `try_load()`; on error prints `ERROR: <msg>` to stderr, prints no values, returns 1. `main.rs` maps the code.
- `run_validate()`: `try_load()` error → `ERROR: <msg>`, return 1. The post-inflation total-vs-per-call comparison is removed (the resolver now rejects the contradiction before any inflation could occur). Stall and concurrency checks unchanged.
- `run_defaults()`: unchanged.
- Replace `test_validate_fails_when_total_lt_per_call` (which asserts the defect, exit 0) with a test asserting exit 1 for explicit total 500 000 000 < per-call 2 000 000 000.

### R2.5 Tests

- `config_memory_tests.rs`: pure table tests over `resolve()` for every row of R2.1, plus `+5`, ` 5`, `5 `, `1e9`, `1GiB`, `-1`, `usize::MAX`+1 as a decimal string, exactly `MIN_EXPLICIT_MEMORY` and `MIN-1`, P == T, P == T+1, and non-UTF-8 (Unix `OsStr::from_bytes`; Windows `OsString::from_wide` with a lone surrogate). No env access.
- Env-touching unit tests: `config_cmd` tests take the same lock as `config_tests` (`crate::config::tests::ENV_LOCK`, made `pub(crate)`), removing the cross-module env race between these two modules.
- `sandbox_boot_tests`: `daemon_sandbox_config` built from a resolved explicit total of 268 435 456 yields `max_memory_bytes == 268 435 456` (the Job Object cap equals the explicit total, never larger).
- New process-isolated integration test `core-runtime/tests/config_memory_cli_test.rs` spawning `CARGO_BIN_EXE_gg-core-cli` with the memory variables controlled explicitly: `config show` with total-only 268435456 prints exactly `GG_CORE_MAX_TOTAL_MEMORY=268435456`; `config validate` exits 1 for P > T, `0`, `abc` and overflow; `serve` exits non-zero with `invalid configuration` and without the listener banner for the same invalid inputs.
- Native Windows packaged qualification (separate evidence, offline VM): release build of this exact SHA; `GG_CORE_MAX_TOTAL_MEMORY=268435456` alone → the Job Object `JobMemoryLimit` read back with `QueryInformationJobObject` equals 268 435 456, and loading Qwen2.5-0.5B Q4_K_M (491 400 032 bytes) is refused with `Insufficient memory budget … limit 268435456`; the daemon stays ready (`health` / `models list` succeed) after the refusal; total 2 GiB loads and generates. Invalid settings: `serve` exits non-zero before listening.

### R2.6 Scope boundary

Only `GG_CORE_MAX_TOTAL_MEMORY` and `GG_CORE_MAX_MEMORY_PER_CALL` become strict. Other variables (`GG_CORE_MAX_CONCURRENT`, queue, batch, frame, connections, timeouts, `N_CTX`) still fall back or floor silently. For them a fallback cannot increase *memory* authority beyond the total, which remains the Job Object and admission bound. A follow-up issue will be filed for strict parsing of the remaining resource variables. `GG_CORE_MAX_CONCURRENT=0` still floors to 1 (unchanged).

### R2.7 Docs

`config.rs` module table, `SECURITY.md` resource-limit rows, `docs/USAGE_GUIDE.md` (stale 1 GiB / 2 GiB defaults corrected) and the `README.md` config table: an explicit total is an exact ceiling and the Windows Job Object cap; invalid or contradictory memory settings abort startup.

### R2.8 Not changed

N7 branch `55d9fd0` and its artifact, `v0.10.0-rc.1`, CoreForge, CI workflows, release and signing.

---

## Revision 3: remediation of audit VETO V1–V3 and O1–O5 (2026-11-12)

Supersedes the conflicting points of Revision 2. Everything in Revision 2 that is not mentioned here still applies.

### V1: placement and line budget
- **`config_memory.rs` (new, about 200 lines):**
  - `MemoryLimits`, `MemoryConfigError` with `Display` and `Error`;
  - `parse_explicit`, `resolve`, `from_env`;
  - `ConfigError` (`#[non_exhaustive] enum { Memory(MemoryConfigError) }`) with `Display`, `Error` and `From<MemoryConfigError>`;
  - `MemoryLimits::to_resource_limits(concurrent) -> ResourceLimitsConfig` (under 15 lines);
  - `per_call_ceiling_strict() -> Option<usize>`, which panics on an invalid per-call value.
- **`config.rs`:**
  - removes `load_resource_limits` and `per_call_ceiling_from_env` (about 25 lines);
  - adds `mod`/`pub use` lines, `try_load` (under 30 lines), the `load()` wrapper and its doc, and the deprecated one-line `per_call_ceiling_from_env` that delegates to `config_memory`;
  - `GG_CORE_MAX_CONCURRENT` parsing moves into `try_load`, still with `.max(1)`;
  - projected size about 240 lines, within 250.
- **Other files:**
  - `cli/config_cmd.rs`: 241 lines today, net change about +5. If it exceeds 250, its tests move to `cli/config_cmd_tests.rs`.
  - `runtime_init.rs`: 320 lines today, a pre-existing violation. This change adds no net lines; it changes the return type only.
  - `main.rs`: 202 lines today, about +3.
- Every new function stays at 40 lines or fewer, with nesting of 3 or less.

### V2: every changed test and its new assertion
| Test | New assertion |
|---|---|
| `config_tests.rs:154-171` (per-call `"10"` floored to 1 MiB, `"not-a-number"` → `None`) | `try_load()` is `Err(Memory(BelowMinimum{..}))` for `"10"` and `Err(Memory(Invalid{..}))` for `"not-a-number"` |
| `config_tests.rs` existing tests that call `load()` | switched to `try_load().unwrap()` |
| `cli/config_cmd.rs::test_validate_fails_when_total_lt_per_call` (asserted exit 0, the defect) | renamed `validate_rejects_per_call_above_total`; asserts `run_validate() == 1` for T = 500 000 000, P = 2 000 000 000 |
| `cli/config_cmd.rs::validate_ignores_per_call_when_unset_and_loads_explicit_ceiling` (T = 2 000 000, P = 3 000 000 → `Some(3_000_000)`) | P changed to 1 500 000 (≤ T), asserting `Some(1_500_000)` and `run_validate() == 0`; plus a P > T case asserting `run_validate() == 1` |
| `runtime_init_tests.rs::daemon_config_requires_model_for_readiness` | `load_config().unwrap().0.health.require_model_loaded` under the shared env lock with the memory variables cleared |
| new `config_tests.rs::load_panics_on_invalid_memory_setting` | `catch_unwind(load)` is `Err` for T = `"0"`; the panic happens inside `catch_unwind` while the guard is held, so the guard is not dropped during unwinding and the lock is not poisoned |
| new `config_tests.rs::deprecated_per_call_ceiling_is_strict` | `#[allow(deprecated)]`; valid P → `Some(P)` and unset → `None`; an invalid value panics (checked with `catch_unwind`) |
| new `config_tests.rs::try_load_total_only_is_exact` | T = 268 435 456 with P unset gives `max_total_memory == 268435456`, fallback 268 435 456 and ceiling `None` |

The `runtime_init_tests.rs` test is in the binary crate's test target, which is a separate process from the library tests, so it uses its own existing `ENV_LOCK` there.

### V3: shared lock
- `lib.rs` gets `#[cfg(test)] pub(crate) mod test_env;`, a file holding `pub(crate) static ENV_LOCK: Mutex<()>` and `pub(crate) fn env_guard() -> MutexGuard<'static, ()>`. `env_guard` uses `lock().unwrap_or_else(|e| e.into_inner())`.
- Both `config_tests.rs` and `cli/config_cmd.rs` tests use `crate::test_env::env_guard()`, and their module-local locks are removed.
- A grep of the library for other tests that set `GG_CORE_MAX_TOTAL_MEMORY` or `GG_CORE_MAX_MEMORY_PER_CALL` found none besides these two modules.

### O2: `run_show`
`run_show()` keeps its public signature. It now delegates to the new `pub fn try_show() -> i32` and discards the exit code; on error it prints `ERROR:` and no values. `main.rs` calls `try_show()` and returns its exit code.

### O3: deprecated per-call ceiling
`per_call_ceiling_from_env()` resolves only the per-call variable through `parse_explicit`. It panics on an invalid per-call value only, and does not compare against the total. This is documented.

### O1, O4: documentation truths
The docs will say the following:
- The memory variables govern the standalone daemon and the `config` CLI. In-process Rust, FFI and Python hosts pass a `ResourceLimitsConfig` and do not read them.
- On Windows the explicit total is also the Job Object `JobMemoryLimit`.
- On the Linux default (seccomp-only) mode, the total is enforced by GG-CORE admission only. It is an OS cgroup cap only with `GG_CORE_SANDBOX=enforce`.
- macOS has no OS cap.

Files: `docs/RELEASE_SUPPORT_AND_MAINTENANCE.md:25` (stale 2 GiB default) is added to the R2.7 list.

### O5: CLI child tests
Each child process starts with `env_clear()` and sets only `PATH`, `SystemRoot`, `TEMP`/`TMP` (Windows) and the variables under test. This is the first test in the repository to use `CARGO_BIN_EXE_gg-core-cli`.
