# Model release-wait review remediation: evidence (#183)

**Source under test:** branch `fix/model-release-review-remediation-20261112-no-ci`, commit `aaa22cd`, based on draft #183 `cddb8bbd`, which is preserved as an ancestor.

**Plan and governance:** `MODEL_RELEASE_WAIT_REVIEW_REMEDIATION_PLAN.md`, which covers:
- the owner dispositions of 2026-11-13;
- Revision 2;
- audit L2-R2-1, PASS with conditions C1–C6, all met;
- the independent review, APPROVE WITH CONDITIONS, with every finding fixed.

**Host:** offline Windows Server 2025 build VM (12 vCPU), rustc 1.99.1. No GitHub Actions minutes were used.

**Scope:** source tests and packaged daemon CLI behaviour on one Windows host. This is **not** a clean installed-product qualification, Linux or macOS evidence, combined-stack (#195/#193) integration, or release approval.

## 1. Source gate (batch `r183f`, exact commit above)
- **Checks:** `cargo fmt --check` passes. `clippy --all-targets -D warnings` passes for default, advanced, ffi, onnx and gguf.
- **Tests:** all three runs use `GG_CORE_REQUIRE_REAL_MODEL=1`.

| Suite | Passed | Ignored | Failed |
|---|---|---|---|
| `test` | 1465 | 0 | 0 |
| `test --features advanced` | 1600 | 3 | 0 |
| `test --features onnx,gguf,ffi` | 1548 | 1 | 0 |

- **Release build:** `build --release --features gguf,ffi` passes. `gg-core-cli.exe` sha256 `d11b3b6aff0f1475afd1fc6f13355345cb07f3cfc7b3a9b609b4667903e73f73`, an unsigned development build.
- **Earlier batches on intermediate commits, for the record:**
  - `r183b` and `r183c`: test-only compile errors in the new stall test.
  - `r183e`: a stall-test hold check that passed before request 7 had started, and Clippy `unneeded_struct_pattern` in the new guard.
  - All three are fixed in the commits up to `aaa22cd`.

## 2. Remediation tests (in the suites above)
- **R1, reply order** (`server_release_order_tests`): with the connection writer held, the second wait is refused (409) without spawning. The first reply is never misattributed, and the gate reopens for a third wait once the first reply has been fully written.
- **R1a, stall bound** (`server_release_stall_tests`):
  - **Setup:** real `handle_connection`, a 256-byte duplex and a 300 ms stall. The client stops reading and floods duplicate waits, then sends `cancel_request`.
  - **Result:** the connection ended at **308–316 ms** across batches `r183d`/`r183f`, within one stall. Teardown released the held model, `releasing_models()` was empty, and `LIVE_WAITS` was 0. The client received 1–3 whole 409 frames and no `cancel_response`.
- **`#[non_exhaustive]`** (`ipc::protocol_compat`):
  - The wildcard-match doctest compiles.
  - The exhaustive external match fails with **E0004**, as `compile_fail,E0004`.
  - The in-crate exhaustive guard keeps the doctest's variant list current (28 variants).

## 3. Packaged CLI exit codes (owner disposition 3)
- **Harness:** `scripts/qualification/model_release_wait_cli_exit.ps1` (sha256 prefix `adbd9a01`).
- **Raw data:** `MODEL_RELEASE_WAIT_CLI_EXIT_r183cli.jsonl` (sha256 prefix `655775b9`). No prompt or token is recorded.
- **Setup:** the release binary above with real Qwen2.5-0.5B Q4_K_M (`74a4da8c…`), authenticated named pipe, Windows Job Object enforced, `GG_CORE_N_CTX=4096`, and `GG_CORE_STREAM_STALL_MS=120000`.
- **The hold:** a raw IPC stream whose reader stops after the first frame.

| Step | Expected | Observed |
|---|---|---|
| `models unload qwen --wait-ms 2000` while held | exit 4 | **exit 4**, stderr `release pending: 1 instance(s) still held by in-flight requests` |
| `models list --json` during release | one `unloading` entry | `state:"unloading"`, `total_memory_bytes` 485,452,288 |
| Resume reading the held stream | clean final | 860 frames, final frame with no error |
| `models load` with a wrong token | exit 1 | exit 1, `Invalid handshake token` |
| `models load` | exit 0 | exit 0 (handle 2) |
| plain `models unload` (unchanged contract) | exit 0 | exit 0 |
| `models load` again | exit 0 | exit 0 (handle 3) |
| `models unload qwen --wait-ms 30000` with nothing held | exit 0 | **exit 0**, `released` |
| Daemon at end | alive | alive |

**Harness defects on the way, none of them in the product:**
- The socket path lost a backslash during script generation.
- `cli` is a built-in PowerShell alias for `Clear-Item`, so the helper function never ran.
- PowerShell 5.1 wraps native stderr in error records.
- The first hold prompt ended its stream early.

## Not established
- Linux and macOS.
- The combined #195 + #193 + #183 tree.
- The default-5 s stall bound by measurement; it is documented, and the 300 ms bound is the one measured.
- Clean installed product.
- Owner merge authorization and release GO.
