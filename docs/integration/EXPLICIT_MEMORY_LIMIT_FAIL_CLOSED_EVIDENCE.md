# #182 explicit memory limits: implementation evidence

**Source under test:** commit `a10909dc6b1f6ab96e764fc1da072a7f7791f422`, tree `e7c2d34c7aaf3386895c81e9e3d7340f1e774f8b`, on branch `fix/gg-core-182-memory-ceiling-20261112-no-ci`, based on `main@b06aaa04`. This evidence file is a later commit and changes no code.
**Host:** offline Windows Server 2025 Hyper-V build VM `coreforge-build`, with rustc 1.99.1 (`E:\w\rust199`), MSVC 14.44 and SDK 10.0.26100. No GitHub Actions minutes were used.
**Plan and audit:** `EXPLICIT_MEMORY_LIMIT_FAIL_CLOSED_PLAN.md` (Revisions 1–3) and `EXPLICIT_MEMORY_LIMIT_FAIL_CLOSED_AUDIT.md`. The Revision 2 audit was a VETO; Revision 3 passed.

**What this evidence is.** It shows that the source tests pass and that the packaged release daemon behaves correctly on one Windows CPU host. It is **not** clean installed-product qualification, Linux or macOS qualification, or release approval.

## 1. Source gate (batch `g182c`, exact tree above)

| Job | Result |
|---|---|
| `cargo fmt --check` | pass |
| `clippy --all-targets -D warnings`: default, advanced, ffi, onnx, gguf | 5 of 5 pass |
| `test` (default) | 1465 passed, 0 failed |
| `test --features advanced` | 1600 passed, 0 failed, 3 ignored |
| `test --features onnx,gguf,ffi` | 1547 passed, 0 failed, 1 ignored |
| `build --release --features gguf --bin gg-core-cli` | pass |

The new tests passed in every suite. They cover:
- the resolver table, the non-UTF-8 case and the panic contract;
- the replacement for the test that used to assert the defect (`validate_rejects_per_call_above_total`);
- the Job Object cap equalling the explicit total (`sandbox_memory_cap_equals_explicit_total_below_fallback`);
- `serve` and config refusals, run as child processes (`tests/config_memory_cli_test.rs`).

An earlier batch (`g182b`) ran on the earlier commit `bf836708`. Its full-feature test run and release build failed with `LNK1180` and os error 112 because the VM's disk was full; no code fault was involved. The commit was re-run as `g182c` after GG-CORE debug build caches were cleared.

## 2. Packaged Windows qualification (`scripts/qualification/explicit_memory_limit_job_object.ps1`)

**Inputs:**
- Release `gg-core-cli.exe` from the tree above: sha256 `c4fa8c782db03ba595c5332ef89995234220b241f3716e73d22e00202c13137a`, 8,222,208 bytes.
- Qwen2.5-0.5B-Instruct Q4_K_M: sha256 `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db`, 491,400,032 bytes.
- The real named-pipe daemon, with a random 32-byte token.

**How the Job Object limit is read.** The test reads `JobMemoryLimit` from outside the daemon. It enumerates the daemon's handles, duplicates the job handle, then calls `QueryInformationJobObject(JobObjectExtendedLimitInformation)` and `IsProcessInJob`.

Raw results: `results.jsonl`, sha256 `8602a5395c7ff8bded7e9ee64fbadf6108c63ed52069242314f4e7ed361405d1`. The script's own sha256 is `9804d127460735c7ba121d54e900a76180008e4c4af3f435d4d49cc502bec053`.

| Scenario | Before (issue #182, `ae55c97` packaged) | After (this tree) |
|---|---|---|
| `GG_CORE_MAX_TOTAL_MEMORY=268435456` only: Job Object `JobMemoryLimit` | raised to 1 GiB | **268435456**. Flags `0x2608` = JOB_MEMORY, ACTIVE_PROCESS (limit 1), KILL_ON_JOB_CLOSE, DIE_ON_UNHANDLED_EXCEPTION |
| Same: `models load` of a 491 MB model | rc 0, **loaded** (cap not honoured) | rc 1, `Insufficient memory budget: … used 491400032 bytes, limit 268435456 bytes` |
| Same: daemon after the refusal | – | alive; `live` 0, `status` 0, `models list` 0; Job limit unchanged |
| `config show` with total only | showed 1 GiB | `GG_CORE_MAX_TOTAL_MEMORY=268435456` exactly; fallback reservation 268435456 |
| `GG_CORE_MAX_TOTAL_MEMORY=2147483648`, model preloaded | – | Job limit **2147483648**; `ready` 0; real generation `"Paris. …"` |
| No memory variables (default) | host default | Job limit 12884344832 = 50% of RAM; fallback 1 GiB (unchanged default behaviour) |
| Invalid settings: T 256 MiB with P 512 MiB; T `0`; P `0`; T `abc`; T `18446744073709551616`; P `10`; P > host default | silently relaxed or defaulted | 7 of 7 refused. `serve` exits 1 within 10–60 ms after the FIPS self-test, with `serve: invalid configuration: …` and no Auth, Sandbox, preload or listener output. `config show` and `config validate` exit 1, and `show` prints no values. |

The "before" column comes from the packaged reproduction recorded on issue #182. It was not re-run in this batch.

## 3. Independent review

A fresh reviewer read the commit `bf836708` diff and returned APPROVE with no blockers. The two should-fix items and four of the five nits are fixed in `a10909dc`:
- docs now say the Job cap applies "unless `GG_CORE_SANDBOX=off`";
- poison-tolerant test locks;
- an exact `usize::MAX`+1 overflow case;
- a timed-out child process is reaped;
- `ok_or_else`;
- a test renamed.

One nit is open: on 32-bit targets, `8 << 30` and `8 * GIB` in the test constants overflow. This does not affect the x64 Windows/Linux targets.

## 4. Audit residual conditions

1. `run_serve` shrank from 65 to 63 lines and still exceeds 40, as it already did. `runtime_init.rs` shrank from 320 to 319 lines and was already over the file-size limit.
2. `catch_unwind` wraps only `load` and `per_call_ceiling_from_env`.
3. Line counts: `config.rs` 248, `config_memory.rs` 202, `config_memory_tests.rs` 199, `config_tests.rs` 224, `cli/config_cmd.rs` 246, `main.rs` 217 and `tests/config_memory_cli_test.rs` about 122, all within 250.
4. The readiness test clears the memory variables while holding its lock.
5. The packaged Windows Job Object evidence is §2 above.

## Not established

- Linux: in the default seccomp-only mode the cap is enforced by GG-CORE admission only, and in cgroup `enforce` mode it was not tested here.
- macOS: there is no OS cap.
- Clean installed product, other models, concurrency under the cap, owner release GO.
