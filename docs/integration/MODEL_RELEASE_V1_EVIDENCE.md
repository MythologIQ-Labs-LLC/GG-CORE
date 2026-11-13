# Verified model release: qualification evidence

**Branch:** `feat/v1-unload-reclaim-20261109-no-ci`. The plan is `MODEL_RELEASE_V1_PLAN.md` revision 4, with the audit verdict (PASS) in `MODEL_RELEASE_V1_AUDIT.md`.
**Host:** the offline Hyper-V guest `coreforge-build`: Windows Server 2025, 12 vCPU, 24 GiB, no virtual switch (0 network adapters up), CPU only.
**Evidence levels:** source tests (§1), in-process real-model test (§2) and installed release binary (§3) are reported separately. No GitHub Actions minutes were used.

## 1. Source build and tests

Final runs (`fin197`, `fin199`) on head `531343c`:

| Toolchain | fmt | clippy `-D warnings` | default | advanced | `onnx,gguf,ffi` |
|---|---|---|---|---|---|
| Rust 1.97.0 | pass | pass on 5 feature sets (default, advanced, ffi, onnx, gguf) | 1456 passed, 0 failed | 1591 passed, 0 failed, 3 ignored | 1539 passed, 0 failed, 1 ignored |
| Rust 1.99.1 | pass | pass on the same 5 | 1456 passed, 0 failed | 1591 passed, 0 failed, 3 ignored | 1539 passed, 0 failed, 1 ignored |

`GG_CORE_REQUIRE_REAL_MODEL=1` was set for the full-feature suites, so real-model tests fail rather than skip if the GGUF fixture is missing.

**Failures seen on the way, all from timing-sensitive real-model tests contending for CPU.** None involved product code.

| Run | Test | What happened | Fix |
|---|---|---|---|
| `rel197` | `ipc::server::real_model_tests::short_request_is_not_serialized_behind_a_long_stream` (#177) | The suite now runs three real-model daemon tests in parallel | `fd66677`: real-model daemon tests share one lock |
| `rel199` | `tests/stream_stall_real_model_test.rs` (#177) | 244 frames delivered before the 200 ms stall bound fired | None needed: passed 6 of 6 in isolation (3 per toolchain, `fix197`/`fix199`, 33 frames each), and passed in both final runs |
| `fix197` | `engine::gguf::backend::cancel_tests::real_model_nonstream_deadline_and_cancel_fail_explicitly` | Under parallel load a full prefill took 17.5 s, so a 1.75 s deadline returned at 11.8 s | `531343c`: the new release test was lightened (2 threads, 120-token stream) |

These timing tests remain sensitive to host load. A loaded runner can make them flake regardless of this change.

**New tests** (16, all passing):
- `models::lifecycle::release_tests` (8): idle release; held instance listed and charged; short wait; no lock held while waiting; reloaded id tracked separately; unlimited lifecycle; the no-runtime case fails closed; the 1,000-listing unload race.
- `ipc::protocol_release_types::tests` (3): wire round trip; wait clamp; existing unload messages unchanged.
- `ipc::server::release_tests` (3):
  - an unauthenticated wait gets 401 and the connection stays open;
  - a second pending wait gets 409;
  - cancel is served during a wait, which then reports released, and the listing shows the instance once;
  - a wait ends when its connection does.
- `ipc::server::release_real_model_tests` (1): see §2.
- `public_api_compat_test::unload_api_and_wire_structs_are_unchanged` (1).

`tests/model_ipc_test.rs`, which uses external-crate struct literals of `ModelUnloadRequest`, compiles and passes unchanged.

## 2. In-process real model (A10)

Fixture: `models/qwen2.5-0.5b-instruct-q4_k_m.gguf`, at `n_ctx` 512.
- R = memory_usage = 485,452,288 B.
- W = working_memory_estimate = 439,058,432 B.
- Budget = R + W + (R − W)/2 = 947,707,648 B.

| Step | Result |
|---|---|
| Stream on `a`, reader stalled (back-pressure) | First frame received |
| Unload `a` | success; 1 releasing instance |
| Load second instance `b` while `a` held | refused `ResourceExhausted` |
| `model_release_wait_request{wait_ms:2000}` | `released:false, pending_instances:1` |
| Resume reading | Stream ends with one final frame, no error |
| Wait again | `released:true, pending_instances:0` |
| Request for unloaded `a`, nothing resident | `model_not_loaded` |
| Load `b`, infer | Admitted; inference succeeds |

## 3. Installed release binary (A11)

The binary is `gg-core-cli.exe` from batch `rel197` (product source identical to the final head; later commits change tests only), built with `cargo build --release --locked --features onnx,gguf,ffi` on Rust 1.97.0.
- SHA-256 `909624536933906bc1116cfec0958d2cc36204d4e36334ba614cf2baf1586254`.
- `version --json` reports features `gguf, onnx, ffi`. Its commit field reads `unknown`, because the build ran from a staged copy without `.git`.

The run used:
- a one-shot SYSTEM task, so the daemon is not nested in the WinRM job;
- default production controls: token auth and the Windows Job Object memory cap;
- the app-local Microsoft VC++/OpenMP DLLs;
- a Python stdlib harness that drove both the CLI and a raw named-pipe IPC client;
- models copied into the run's own base path. Per-file `*.manifest.json` files (SHA-256, size, licence, labels) were written for the two ONNX models, as #170 requires.

### 3.1 Idle cycles at default limits

10 cycles × 4 models: Qwen2.5-0.5B and SmolLM2-1.7B (GGUF), MiniLM embed and DistilBERT-SST2 classify (ONNX). Each step was load, then one real use, then `models unload --wait-ms 30000`, then 2 s settle and a median of 5 samples.

| Criterion | Result |
|---|---|
| `released` on every idle unload | **40 / 40** |
| Load / use / unload exit codes | all 0 |
| Handle count | 86 before the first load; **93 constant** after every unload in all 10 cycles (within ±16) |
| Idle private bytes, cycle 2 to cycle 10 | 28.7–29.2 MB to 29.8–30.7 MB (+3.8% to +6.9%): no monotonic growth |
| Idle private bytes, cycle 1 | 12.8 MB (qwen) and 14.3 MB (smollm2), before any ONNX model had loaded; 28.9 / 28.0 MB after the first ONNX load |
| Daemon stderr allocation errors | none |

**One-time step, not a leak:** the first ONNX load adds about 15 MB of idle private memory (candle/tokenizer runtime initialisation), and it never grows after that. Read strictly, the plan's "cycle 10 within 10% of cycle 1" fails for the two GGUF models (12.8 MB to 30.7 MB) only because of this step. The like-for-like ONNX comparison passes: MiniLM 28.9 to 30.2 MB (+4.5%), DistilBERT 28.0 to 30.2 MB (+7.9%). So does every cycle-2 to cycle-10 comparison.

### 3.2 Held instance under the Job Object cap

The daemon ran with `GG_CORE_MAX_TOTAL_MEMORY=1342177280` (1.25 GiB, which is also the Job Object cap), `GG_CORE_N_CTX=2048` and `GG_CORE_STREAM_STALL_MS=120000`.
- A = Qwen2.5-0.5B, B = SmolLM2-1.7B.
- The hold: a raw IPC stream (a counting continuation, 1,500 tokens) whose reader stopped after the first frame.

| Step | Result |
|---|---|
| Load A | handle 1; process private 88.5 MB |
| Stream started, reader stalled | private 497 MB while generating |
| `models unload qwen` | success; private still 497 MB (held) |
| `models list --json` during release | one entry: `handle_id:1`, metadata name `qwen2.5-0.5b-instruct-q4_k_m`, `state:"unloading"`; `total_memory_bytes` still 485,452,288 |
| Load B while A held | **refused**: `used 2100444640 bytes, limit 1342177280` (pre-load estimate, no backend I/O) |
| `model_release_wait_request{wait_ms:2000}` | `released:false, pending_instances:1` |
| Resume reading | 1,225 frames; final frame with no error |
| Wait again | `released:true`; private back to 14.9 MB; listing empty |
| Load B | **admitted** (handle 2) |
| Unload B with `--wait-ms` | `released` |
| `unload --wait-ms 120000` issued 1 s into a stream that is being read | returned `released` after 1.37 s, after the stream's final frame |
| Daemon alive at end; allocation errors in stderr | alive; none |

Headroom under the cap: R_A + W_A(2048) + idle U ≈ 0.49 + 0.47 + 0.03 ≈ 1.0 GB, against 1.342 GB. The peak private memory observed was 497 MB with A streaming, and 797 MB with B resident.

### 3.3 Harness notes

Several earlier attempts produced no evidence because of defects in the harness, not the product:
- PowerShell treats a function named `Cli` as its built-in `cli` alias (Clear-Item).
- The ONNX models had no per-file manifests in the shared model folder.
- Two prompts ended (end of sequence) before filling the 64 KiB pipe buffer, so the reader stall never held the model.

Those runs are not counted.

## 4. Not covered

- Linux and macOS installed behaviour; GPU/VRAM reclaim.
- OS-level reclaim after a failed native allocation (llama-cpp-rs#1194). `released` means destructors ran and the budget returned, not that the OS reclaimed every page.
- Release time when the client sends no deadline. Non-streaming and typed requests without `timeout_ms` can hold an instance indefinitely.
- Switch policy: atomic switch, warm A+B, rollback, readiness. That is the owner decision in #153.
- A packaged archive with the build commit recorded. This run used a release binary built from a staged copy.
