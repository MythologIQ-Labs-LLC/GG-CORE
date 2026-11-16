# N7 stop-stage instrumentation: evidence (#184)

Development branch `fix/n7-stop-stage-trace-20261114-no-ci`. Nothing is merged, tagged, signed or published, and no CI was used: every run was on the offline Windows build VM. Plan, audit and review are in `N7_STOP_STAGE_TRACE_PLAN.md`.

## Answer
On the pinned Qwen2.5-0.5B, the stop tail that #184 reported is fixed in `main` by #195's deterministic prefill schedule. This branch adds the instrumentation that shows what the remaining tail consists of. In every traced stop, the time from answering the caller to the backend observing the stop was within the duration of the single native decode call that was in flight, plus 5 ms (54 of 54). After the backend observed the stop:
- the worker released in at most 11.5 ms;
- process CPU was quiet within one to two 100 ms sampler periods.

There is no measured post-observation work to remove, so this branch **makes no compute change**. The remaining tail is one in-flight native decode (up to 0.71 s here). A hard bound below that needs a native abort callback, which `llama-cpp-2` 0.1.161 does not expose. That is an owner decision; see Limits.

## Which source these results belong to
- **Every measurement outside the section "Integration head `d4bb8a3`" was made on source `fcfa3cc`** (base `1032d1b`) and its daemon `5632E1E5…80C1`. They are kept unchanged as the historical baseline, and the Answer above describes them.
- They are **not** results for the integration head that merges current `main`. That head was qualified separately, on a GG-CORE-only build VM, in "Integration head `d4bb8a3`" below.
- The `c1` comparison daemon in `N7_STOP_STAGE_RESULTS_n7stage.jsonl` is the historical pre-#195 producer binary (`3FA16661…3F40`, GG-CORE source `ae55c976`). The integration-head measurement uses only daemons built from GG-CORE source on the GG-CORE VM.

## Scope of the stage correlation (review N7-R2)
The stage joins (S1–S4 by `request_id`, then by `queue_id` and time) are qualified **only for the recorded harness**: one client, one stop at a time, non-streaming requests.
- The 54/54 stage observations do **not** establish behaviour with several clients stopping at once, where a `request_id` is unique only per connection.
- They do **not** establish packaged streaming stops either: streaming S2 has no `queue_id` and is joined by order, and the harness runs no streaming stop. Streaming events are covered by unit tests only.
- The evidence gate (`scripts/qualification/n7_stop_stage_report.py`) applies the same joins and carries the same limit.

## Integration head `d4bb8a3` (current `main` merged)
Requalification of the branch after merging `main` `00f3c81`. Results come only from this source and these binaries; nothing from `fcfa3cc` is reused.

**Identity**
| Item | Value |
|---|---|
| Source | `d4bb8a31f2b547b7fc4a8ea44045b9e1c5c0e6b1`, tree `306beae5199bdf3987e15a67fff020af00567e4f`. Merge `5d7b895` (no history rewritten); runtime code identical to the review head `ccd7fb4`. |
| New daemon (`--release --features gguf,ffi`) | `gg-core-cli.exe` 8,530,944 bytes, SHA-256 `BE6BA6917B818D0E022129C030A514333C94B47E5F00B40B13C11E4CDE3EC159`, unsigned |
| Baseline `main` daemon, built from `00f3c81` in the same environment | 8,315,392 bytes, `B90836A8D40EFEF9092FA8645CCB310EF095CAB5AD5BEC462911469AAEFF6059` |
| Model | Qwen2.5-0.5B-Instruct Q4_K_M, SHA-256 `74A4DA8C9FDBCD15BD1F6D01D621410D31C6FC00986F5EB687824E7B93D7A9DB`, 491,400,032 bytes; one resident model, `GG_CORE_MAX_CONCURRENT=2` |
| Host | GG-CORE-only offline Hyper-V VM `ggcore-build`: Windows Server 2025 (evaluation), 8 vCPU, 16 GB, no network adapter, CPU only |
| Toolchain | rustc/cargo 1.99.1, clippy 0.1.99, rustfmt 1.10.0; VS Build Tools 17.14 (MSVC 14.44.35207, Windows SDK 10.0.26100); LLVM/clang 23.1.4; CMake 4.4.5; Ninja 1.13.2; protoc 36.2; Python 3.11.9. All installers hash-verified before staging. |
| Dependencies | `cargo --locked --offline` against 344 crates, each verified against the `Cargo.lock` checksum. No network at build or run time. |

**Source gate (batch `n7g1`).** All with `--locked --offline`; tests with `GG_CORE_REQUIRE_REAL_MODEL=1`.
| Step | Result |
|---|---|
| `cargo fmt --check` | pass |
| `clippy --all-targets -D warnings`: default, advanced, ffi, onnx, gguf | pass ×5 |
| Tests, default features | 1,502 passed, 0 failed, 0 ignored |
| Tests, `advanced` | 1,637 passed, 0 failed, 3 ignored |
| Tests, `onnx,gguf,ffi` (real model) | 1,609 passed, 0 failed, 1 ignored; includes the real-model N7, cancel-latency and stop-observation tests |
| `build --release --features gguf,ffi` | pass |

Ignored tests:
- Three doctests are marked `ignore` because they are illustrative and not compilable on their own: `engine::adaptive_speculative` (`mod.rs` and `telemetry.rs`) and `models::tier_synergy_speculative`.
- `working_memory::tests::real::flash_attention_off_peak_stays_within_estimate` needs a multi-model set (`GG_CORE_WM_MODELS`) that is not in the fixture set.
- None of them covers stop or trace code.

**Unchanged surfaces.** Relative to `main` `00f3c81`:
- The branch changes neither `Cargo.toml` nor `Cargo.lock`, and touches no file under `sandbox/` and no `ipc/auth*` file.
- Its IPC and scheduler edits only add stop-trace calls; their test files and the adversarial review cover them.
- The only runtime change that `main` brought in after `fcfa3cc`'s base is the `windows-sys` 0.61 handle-type migration in `sandbox/windows.rs`. It does not touch the stop path.

**Evidence gate on the committed files.** `n7_stop_stage_report.py` was run with `--labels main,newoff,newon` and the pre-stated thresholds. It has 44 synthetic tests, all passing: one negative per rule, malformed input, the review cases (including Codex P2: S1 required for explicit cancels, recoveries counted per label and configuration), and the committed `fcfa3cc` evidence passing. Both committed runs give the same verdict under the tightened checks.

**Packaged measurement, run 1 (batch `n7m1`): FAIL, recorded and not used.** The gate exits 1 on R1, R5, R6 and R7.
- **Cause.** For about 16 minutes the whole VM slowed. The untraced `main` daemon was hit too: its recovery requests took 12–20 s instead of about 1.4 s.
- **Where the failures sit.**
  - Every R1 and R5 failure is in the one traced cell measured during that window (`ctx16384-t2`). There, setup-phase stops were observed only after 22–31 s.
  - The R6 failure is `ctx16384-t0`, ratio 0.968. That cell's trace-off rep 0 ran as the window was ending.
  - R7 found a single output hash. It failed only because one `main` throughput row was not written. A file read made during the run held the file, and that read was a harness-operator error. No read was made during run 2.
- **Unexplained.** The guest logged no task or service activity in the window, and the cause is not identified. Results, trace and report: `*_n7m1.*`.

**Packaged measurement, run 2 (batch `n7m2`): PASS.** Identical harness, binaries and matrix: contexts 8k/16k, threads 1/2/auto, 3 repetitions, configurations interleaved. Gate exit 0:
- **R1:** 0 of 72 traced runs incomplete.
- **R2:** 0 queued cancels reached the backend.
- **R3:** full matrix present; 54 recoveries.
- **R4:** every stop and recovery correct.
- **R5:** 54 of 54 S2→S3 within the decode in flight + 5 ms. Worst observation 676 ms; worst in-flight decode 834 ms.
- **R6:** trace on/off throughput ratio 0.993–1.017.
- **R7:** one output hash (`57822580A271832B`) per configuration across `main`, trace-off and trace-on.

Descriptive results (not gated):
- **Throughput:** new trace-off against `main` is −2.1% to +1.6%.
- **Stop tails against `main`:** median post-stop tails of the trace-off daemon match `main`. Prefill cancel is 494 vs 556 ms, prefill deadline 381 vs 334 ms and decode cancel 112 vs 112 ms, with 18 stops each.
- **Tracing cost:**
  - With tracing on, prefill-deadline tails are higher in both runs: run 2 median 442 ms and mean 459 ms, against 381/391 trace-off.
  - S3→S4 (observed → worker released) has a median of 50 ms and a maximum of 177 ms; 36 of 54 stops exceed 20 ms. On `fcfa3cc` (12 vCPU) the maximum was 11.5 ms.
  - The stop-path code is unchanged, so this comes from the host (8 vCPU, with auto threads occupying all of them) or from trace cost. With the trace on 18 stops per kind, it is not separated further.
  - Tracing is off by default, and the default-off daemon shows no tail or throughput regression.
- **Host load:** a host-side sampler ran during run 2. Its per-process column shows no host process other than the VM above 6% of the machine. Its "total" column includes Idle and is not valid.

Files: `N7_STOP_STAGE_RESULTS_n7m2.jsonl`, `N7_STOP_STAGE_TRACE_n7m2.jsonl` and `N7_STOP_STAGE_REPORT_n7m2.txt`, plus the failed run 1 as `*_n7m1.*`. Raw daemon stderr is not committed.

**Not tested on this head:** Linux, macOS, GPU, models other than Qwen2.5-0.5B, more than one client, packaged streaming stops, installed products, deadlines longer than 3 s, and signing. The single-client, non-streaming limit (N7-R2) applies unchanged.

## Identity (`fcfa3cc`)
| Item | Value |
|---|---|
| Source tested | `fcfa3cc` (base `1032d1b` = qualified `main` before the 2026-11-14 dependency bumps) |
| New daemon (`--release --features gguf,ffi`, rustc 1.99.1) | `gg-core-cli.exe` 8,529,920 bytes, SHA-256 `5632E1E5E1F28C0E140E471DC4C2BF228FF5FF213292C068114C34CA20B980C1`, unsigned |
| Baseline `main` daemon, built from `1032d1b` in the same environment | 8,314,368 bytes, `BB5705FA4ADDDD12661D6EE1AD0C73A0CEAB24B74B896A1EAC230341CB1D68E1` |
| Defect reproduction daemon, C1 | `3FA16661605BBB4A9AA890BED4588D649039148B5084D78E2D48C6FA7CD03F40` |
| Model | Qwen2.5-0.5B-Instruct Q4_K_M, SHA-256 `74A4DA8C…A9DB` |
| Host | offline Hyper-V VM `coreforge-build`, Windows Server 2025, 12 vCPU, CPU only |

## Source gate (`fcfa3cc`, batch `n7s2`)
- **fmt and Clippy:** `fmt --check` passes. `clippy --all-targets -D warnings` passes for default, advanced, ffi, onnx and gguf.
- **Tests, all with `GG_CORE_REQUIRE_REAL_MODEL=1`:**

  | Suite | Passed | Failed | Ignored |
  |---|---|---|---|
  | default | 1502 | 0 | 0 |
  | advanced | 1637 | 0 | 3 |
  | full (`onnx,gguf,ffi`) | 1609 | 0 | 1 |

  The ignored counts are the same as on `main`.
- **New tests:** 19 in total, and all ran, including 2 real-model S3 tests that were not skipped.
- **Release build:** passes.

## Packaged measurement (batch `n7stage`)
**Setup:**
- Real authenticated named-pipe IPC, two workers (`GG_CORE_MAX_CONCURRENT=2`).
- Contexts 8192 and 16384; threads 1, 2 and auto (`t0`); 3 repetitions each.
- Four daemons interleaved within each configuration: `c1`, `main`, `newoff` (new daemon, trace off) and `newon` (same binary, `GG_CORE_STOP_TRACE=1`).
- Scenarios:
  - `cancel_request` during prefill (2,516-token prompt);
  - deadline during prefill;
  - `cancel_request` during token generation;
  - cancel of a queued request while both workers prefill;
  - a recovery request.
- **CPU quiet** is the harness rule: the last 100 ms sample using 15 ms or more of CPU before two calm samples.

**Correctness, all 4 daemons, 288 stops:**
- Every stop ended with the explicit `cancelled` or `deadline_exceeded`.
- Every cancel got exactly `cancel_response` then one `inference_response`, with no late frame.
- Every daemon quiesced.
- Every recovery request succeeded on the same, still-running process (72 of 72).
- Every queued cancel left both busy requests successful (72 of 72) and never reached the backend (0 of 18 traced).
- Uncancelled output was identical across all four daemons in every configuration (one hash, `57822580A271832B`).

**Post-stop CPU tail (`tailWallMs`), 18 stops per cell:**

| Daemon | Prefill cancel p50 / max | Prefill deadline p50 / max | Decode cancel p50 / max |
|---|---|---|---|
| C1 (defect) | 1213 / **3756** ms | 1001 / **3332** ms | 113 / 223 ms |
| `main` | 330 / 673 ms | 342 / 759 ms | 114 / 217 ms |
| new, trace off | 329 / 768 ms | 331 / 766 ms | 110 / 212 ms |
| new, trace on | 227 / 768 ms | 383 / 774 ms | 118 / 226 ms |

C1's worst cases are at 1 thread: 3.2–3.8 s, reproducing #184. `main` and the new daemon differ by no more than one sampler period, which is the expected outcome, since this branch has no compute change.

**Stage deltas, trace on, 72 traced stops, none excluded for dropped lines or missing stages:**

| Stage | p50 | p95 | max |
|---|---|---|---|
| S1→S2, cancel received → caller answered | 0.05 ms | 0.07 ms | 0.08 ms |
| S2→S3, answered → backend observed | 117 ms | 459 ms | 583 ms |
| `last_decode_ms`, the native call in flight | 621 ms | 684 ms | 706 ms |
| S3→S4, observed → worker released | 7.1 ms | 10.6 ms | 11.5 ms |
| S4→CPU quiet, on the unix clock | 105 ms | 200 ms | 207 ms |

- **S2→S3 ≤ `last_decode_ms` + 5 ms in 54 of 54 timed stops.** The plan's rule was at least 95%. The in-flight call is a 64-, 128- or 256-token prefill chunk, or a single generated token (up to 24 ms).
- **Queued cancels have no S3 or S4** (18 of 18), as expected.
- **S4→quiet is bounded by the sampler.** CPU quiet needs two calm 100 ms samples, so the value cannot be resolved below about 100–200 ms.

**Trace overhead (rule F7):** uncancelled throughput medians, trace on / off, ranged from 0.987 to 1.018 across the six configurations, within the stated ±3%. Outputs were identical, and the worst tails with the trace on and off agree within 14 ms.

**Throughput against baselines** (median uncancelled long prompt, new trace-off): `main` −1.5% to +1.1%; C1 +1.8% to +7.0%. The C1 difference is #195's known schedule cost, not this branch.

## Files
| File | Content |
|---|---|
| `N7_STOP_STAGE_RESULTS_n7stage.jsonl` | Harness results; no prompt text |
| `N7_STOP_STAGE_TRACE_n7stage.jsonl` | Trace-on stage events, filtered to `gg_core::stop` JSON lines |
| `N7_STOP_STAGE_REPORT_n7stage.txt` | Output of `scripts/qualification/n7_stop_stage_report.py` |

Raw daemon stderr is not committed, because it contains llama.cpp native logs with the model path.

## Limits, recorded individually
1. **Not a hard bound.** One native decode call cannot be interrupted. On this host and model the worst in-flight call was 706 ms. #195 records about 4.6 s for Phi-3.5 at 1 thread. A native `llama_set_abort_callback` exists in llama.cpp, but `llama-cpp-2` 0.1.161 keeps the context pointer private (`pub(crate)`), so using it means patching or replacing the wrapper. That needs owner approval and an architecture review.
2. **Joins are single-client.** `request_id` is unique only per connection, and streaming S2→S3/S4 is joined by order. The harness runs one stop at a time and no streaming stop. Streaming events are covered by unit tests only.
3. **Untraced stops:** client disconnect (no S1), stream receiver disconnect, the speculative path (only through the embedder API; `serve` never enables it), and stops caught by the engine pre-check (S2 and S4 without S3).
4. **The stderr pipe is still a risk for existing output.** The trace itself never blocks. The daemon's existing `eprintln!` lines and llama.cpp native logging can still block if a launcher stops draining stderr. Unchanged.
5. **Measured cells only:** Windows Server 2025 on CPU, Qwen2.5-0.5B, contexts 8k/16k, threads 1/2/auto, two workers, 3 repetitions. Not measured: Linux, macOS, GPU, other models, installed products, and the 60 s deadline (deadlines of 2–3 s were used).
6. **Superseded by the integration-head section for `d4bb8a3`.** Original note for `fcfa3cc`: **Not on current `main`.** `main` moved to `12023a3` on 2026-11-14 with dependency bumps (candle, zip 9.5.1) that the offline VM registry lacks. This branch is based on `1032d1b` and changes neither `Cargo.toml` nor `Cargo.lock`. A merge with current `main` needs those crates staged on the VM before it can be requalified.
7. **Help text:** the `RUST_LOG` line in `print_usage` documented a setting the daemon never read. It is replaced by `GG_CORE_STOP_TRACE`.
