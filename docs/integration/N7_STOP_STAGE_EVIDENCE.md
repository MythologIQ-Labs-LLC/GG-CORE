# N7 stop-stage instrumentation: evidence (#184)

Development branch `fix/n7-stop-stage-trace-20261114-no-ci`. Nothing is merged, tagged, signed or published, and no CI was used: every run was on the offline Windows build VM. Plan, audit and review are in `N7_STOP_STAGE_TRACE_PLAN.md`.

## Answer
On the pinned Qwen2.5-0.5B, the stop tail that #184 reported is fixed in `main` by #195's deterministic prefill schedule. This branch adds the instrumentation that shows what the remaining tail consists of. In every traced stop, the time from answering the caller to the backend observing the stop was within the duration of the single native decode call that was in flight, plus 5 ms (54 of 54). After the backend observed the stop:
- the worker released in at most 11.5 ms;
- process CPU was quiet within one to two 100 ms sampler periods.

There is no measured post-observation work to remove, so this branch **makes no compute change**. The remaining tail is one in-flight native decode (up to 0.71 s here). A hard bound below that needs a native abort callback, which `llama-cpp-2` 0.1.161 does not expose. That is an owner decision; see Limits.

## Identity
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
6. **Not on current `main`.** `main` moved to `12023a3` on 2026-11-14 with dependency bumps (candle, zip 9.5.1) that the offline VM registry lacks. This branch is based on `1032d1b` and changes neither `Cargo.toml` nor `Cargo.lock`. A merge with current `main` needs those crates staged on the VM before it can be requalified.
7. **Help text:** the `RUST_LOG` line in `print_usage` documented a setting the daemon never read. It is replaced by `GG_CORE_STOP_TRACE`.
