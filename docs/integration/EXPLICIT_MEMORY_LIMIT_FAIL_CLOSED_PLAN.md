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
