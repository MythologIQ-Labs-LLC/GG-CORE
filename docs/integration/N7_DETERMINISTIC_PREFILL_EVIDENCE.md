# N7 successor: deterministic prefill schedule, implementation evidence (#184)

**Source under test:** branch `fix/n7-deterministic-prefill-20261112-no-ci`, commit `22829c18b96555d9ab706e06598e35da727573e3` (tree `01e21a75`). The base is the C2 producer `55d9fd0`, which is preserved unchanged as an ancestor; its `d64b5662…` daemon is not rebuilt.

**Plan and audit:** `N7_DETERMINISTIC_PREFILL_PLAN.md` (Revisions 1–3 and implementation notes) and `N7_DETERMINISTIC_PREFILL_AUDIT.md`. The audit went VETO, VETO, then PASS with four binding conditions.

**Host:** offline Windows Server 2025 build VM (12 vCPU), rustc 1.99.1. No GitHub Actions minutes were used.

**Scope of this evidence:** source tests, packaged daemon behaviour and measurement on one Windows CPU host. It is **not** clean installed-product qualification, Linux or macOS evidence, or release approval.

## 1. Source gate (batch `n7f`, exact tree above)
- **Checks:** `cargo fmt --check` passes. `clippy --all-targets -D warnings` passes for all 5 feature sets: default, advanced, ffi, onnx, gguf.
- **Tests:** `test` (default), `test --features advanced` and `test --features onnx,gguf,ffi` all pass with `GG_CORE_REQUIRE_REAL_MODEL=1`, so a missing fixture fails the run instead of skipping.
- **Release build:** `build --release --features gguf,ffi` passes.
  - `gg-core-cli.exe`: 8,212,992 bytes, sha256 `3565ab421f2d04b378ce028445b8e0a803df4804f54bc65b3eb9eb51e2a9a833`.
  - `gg_core.dll`: sha256 `011e9cf0e8fe30194ecd4eb2c3dc615ff6844eb225700c6c395dac794b1057d7`.
  - Both are unsigned development builds.
- **Earlier batches, for the record:**
  - `n7d` (commit `4b3d92c5`): one doctest failed with linker error `LNK1318` (PDB `LIMIT`) because VM C: had 0.1 GB free. After freeing space by removing this lane's obsolete snapshots, the code passed.
  - `n7e` (commit `87ea296d`): `clippy::await_holding_lock` failed under gguf. Fixed in `22829c18`.

## 2. Determinism (acceptance item 1)
**Measurement job** (`GG_N7_MEASURE=1`):
- The determinism matrix covers thread buckets 1, 2, 4 and 8 × the three d184 prompts (2516, 568 and 100 tokens). All 12 cells produced the identical split and identical greedy output with no stop, with a stop and under 8-thread CPU load.
- Every recorded split equals the planned schedule:

| Bucket | Splits observed |
|---|---|
| 1 | 2516 → 39×64 + 20 |
| 2 | 2516 → 19×128 + 84 |
| 4 | 2516 → 9×256, 128, 84 |
| 8 | 2516 → 9×256 + 212; 568 → 256, 256, 56; 100 → 100 |

**Always-on tests:**
- The streaming path (`generate_stream`) records the planned split.
- A test fails against the old C2 pacer: with no stop it decoded one 512 chunk, while the paced run started at 32.

## 3. Stop tail and throughput (acceptance items 2–3, packaged daemons)
- **Harness:** `scripts/qualification/n7_packaged_stop_tail.ps1` (sha256 `18a5a441…9611`).
- **Raw data:** `N7_PACKAGED_STOP_TAIL_g184stop.jsonl` (sha256 `6506c2d9…46d2`). No prompt text or token is recorded.
- **Setup:** real authenticated named-pipe IPC with Qwen2.5-0.5B Q4_K_M (`74a4da8c…`) and a 2516-token prompt. Each daemon was run at contexts 8192 and 16384, at threads 1, 2 and auto (6), with 3 repetitions per cell.
- **What was measured:**
  - **Throughput:** a prefill-dominated 24-token request.
  - **Cancel:** `cancel_request` at 1.5, 2.0 and 2.5 s into prefill.
  - **Deadline:** expiry at 2.0, 2.5 and 3.0 s.
  - **Tail:** from the stop until the daemon's CPU time is quiescent, sampled every 100 ms.
  - **Recovery:** a follow-up request.

| Daemon | Worst tail, cancel / deadline, all cells | Throughput median, auto threads (8k / 16k) | 1 thread (8k / 16k) |
|---|---|---|---|
| C1 `3fa16661…` (fixed 512) | 3.66 s / 3.20 s | 6.97 / 6.87 s | 28.36 / 28.90 s |
| C2 `d64b5662…` (time-paced) | 1.14 s / 1.11 s | 6.88 / 7.39 s | 32.07 / 31.69 s |
| **N7D `3565ab42…` (this branch)** | **0.78 s / 0.66 s** | **7.41 / 7.33 s** | **30.31 / 29.85 s** |

- **Gates:**
  - Worst tail at most 1.2 s at 1 thread, 2 threads and default threads: **PASS** (0.78 s at most).
  - Throughput no worse than C2 + 10% and fixed 512 + 10%: **PASS** (at most +7.7% against C2 and +6.7% against C1 at auto threads).
- **Correctness:** all 36 N7D stop runs (6 cells × 3 repetitions × cancel and deadline) and all 72 baseline stop runs quiesced and ended with `cancelled` or `deadline_exceeded`, with zero tokens and successful recovery. 0 problems.
- **Output:** the output hash `57822580A271832B` was identical for all three daemons and every thread count on this prompt.
- **Confounder:** C2 was built with rustc 1.97, while C1 and N7D used rustc 1.99.

## 4. Long-prompt and cross-model (reported, not gated)
- **Reference model** (`n7f-measure` characterization, 2516 tokens): the worst scheduled chunk was 815–1012 ms. Prefill against fixed 512 ranged from −2% to +6% across 1, 2 and 4 threads at 8k and 16k.
- **Larger models:** experiment `x184c`, which is not part of the branch, used provenance-verified SmolLM2-1.7B Q4_K_M (`77665ea4…`, Apache-2.0) and Phi-3.5-mini Q4_K_M (`e4165e3a…`, MIT).

| Model | Threads | Worst chunk | Prefill vs fixed 512 | Split and output under load |
|---|---|---|---|---|
| SmolLM2 | 4 | 0.91 s | +4.5% | identical |
| SmolLM2 | 1 | 2.25 s | −22% (noisy) | identical |
| Phi-3.5 | 4 | 2.44 s | +10% | identical |
| Phi-3.5 | 1 | **11.9 s** | +4% | identical |

- **Determinism holds on every model.**
- **The model-bytes scaling term underestimates the larger models' chunk time.** At 4 threads, Phi-3.5 is about 50% over its prediction. At 1 thread the 64-token floor produced an 11.9 s chunk.
- Under acceptance rule 5 and audit condition 3 this is **reported, not retuned**. Scaling beyond the 0.49 GB calibration model was declared unqualified. Re-deriving it needs a fresh multi-model calibration batch and a re-audit, which is an owner decision.
- **Superseded by Revision 4 (§6).** The owner approved a recalibration. It found that the overruns were the final sub-64 remainder, not the model-bytes scaling.

## 5. Independent review
The independent review of `4b3d92c5` returned APPROVE WITH CONDITIONS with no blockers. All 7 findings are fixed in `87ea296d`, and the Clippy fix is in `22829c18`.

## Not established
- Linux and macOS.
- GPU offload.
- Model-size scaling outside 0.49–2.4 GB Q4_K_M, and the sub-64 bound away from positions 2300–2600 (§6).
- Contexts above 16k.
- Clean installed product.
- Owner release GO and merge authorization.

## 6. Revision 4: sub-64 remainder pieces (plan Revision 4, audit R4-2 PASS with conditions)
**Source:** commit `3e0f522` (implementation) and `2f55c88` (review fixes; tests and comments only, so production code is identical).

**Data:**
- Calibration: `N7_PREFILL_CALIBRATION_mcal.txt` and `N7_PREFILL_CALIBRATION_shape.txt`.
- Packaged run: `N7_PACKAGED_STOP_TAIL_g184stop-n7g.jsonl` (sha256 `317dce9d…15c8`).

### Source gate
- **Batch `n7g` (`3e0f522`):** fmt, clippy `-D warnings` for 5 feature sets, the 3 test suites with `GG_CORE_REQUIRE_REAL_MODEL=1`, and the release build all pass.
- **Batch `n7h` (`2f55c88`):**
  - fmt, clippy for 5 feature sets, `test --features onnx,gguf,ffi` (808 passed, 1 ignored), the release build and the measurement job all pass.
  - The default and advanced suites failed one doctest each with `LNK1318` (PDB `LIMIT`) because VM C: had 0.01 GB free. Their unit tests passed (757 and 847). After this lane's obsolete snapshots were removed, they were re-run as batch `n7i`. Both pass with 0 failures, doctests included.

### Determinism matrix (C3; batches `n7g` and `n7h`)
- All 12 cells record exactly the planned split, with identical output with no stop, with a stop and under 8-thread load.
- The expected split changes:

| Cell | Old split | New split |
|---|---|---|
| 568@1 | ends `56` | ends `19, 19, 18` |
| 100@1 | `64, 36` | `64, 18, 18` |
| 568@2 | ends `56` | ends `28, 28` |

- Their output differs from the old schedule only as expected, since a different split is a different llama.cpp batch sequence.
- The other 9 cells, including every 2516-token split, are unchanged.

### Cross-model acceptance (item 2 and C2; `GG_N7_MEASURE=1`)
Every remainder piece is under `(p+6)/24 × T64` (the run's last 64-token chunk). No piece exceeded its bound in either batch.

| Model, threads | Remainder pieces (ms), `n7g` / `n7h` | Worst chunk (`n7g` / `n7h`) | Worst ≤ 1.2 × max(1 s, worst 64)? |
|---|---|---|---|
| Phi-3.5, 1 | 16, 15, 15, 15 → 2545–2684 / 2825–3714 | 4609 / 11 336 ms, both 64-chunks | yes |
| Phi-3.5, 4 | 16, 15, 15, 15 → 1106–1221 / 740–784 | 2461 / 1405 ms, both 64-chunks | yes |
| SmolLM2, 1 | 11, 11 → 1037–1071 / 1161–1179 | 3542 / 2254 ms, both 64-chunks | yes |
| SmolLM2, 4 | 22 → 506 / 624 | 978 / 1036 ms | yes |

- **Split and output:** identical with no load and under load in all 8 runs.
- **Prompt length:** the measured prompt is 2877 tokens on Phi-3.5 and 2518 on SmolLM2; unit tests pin both splits.
- **Old remainder, for comparison:** on Phi-3.5, the old 61-token final chunk took 2.4 s at 4 threads and 8.4 s at 1 thread.
- **Host noise:**
  - Absolute times vary up to 2.5× between batches. This is the shared VM: in `n7h` a Phi 1-thread 64-chunk took 11.3 s, against about 4.6 s in `n7g` and 4.4 s in `shape2`.
  - `n7g` had no untimed warm-up; `n7h` does, after review finding 2.
  - The pieces' ratio to the same run's 64-chunk is the host-independent check.

### Packaged stop tail and throughput (batch `g184stop-n7g`; daemon built from `3e0f522`)
- **Daemon:** `gg-core-cli.exe` sha256 `dec0decf20cecf171643fd2dc7b437caaae79f1cd62d5b0812c6ca1d6d63002e`, an unsigned development build.
- **Method:** same harness and model as §3. C1 and C2 were re-run in the same batch.

| Daemon | Worst tail, cancel / deadline | Throughput median, auto threads (8k / 16k) | 1 thread (8k / 16k) |
|---|---|---|---|
| C1 `3fa16661…` | 3.78 s / 3.19 s | 7.30 / 7.42 s | 29.78 / 29.26 s |
| C2 `d64b5662…` | 1.12 s / 1.21 s | 7.27 / 7.83 s | 36.93 / 32.72 s |
| **N7 Rev 4 `dec0decf…`** | **0.67 s / 0.79 s** | **7.92 / 7.83 s** | **30.44 / 32.01 s** |

- **Gates:**
  - Worst tail ≤1.2 s: **PASS** (0.79 s at most).
  - Throughput at auto threads no worse than C2 + 10% and C1 + 10%: **PASS**, with a narrow margin (at most +9.0% against C2 and +8.5% against C1, on a noisy shared host).
- **Correctness:** 0 problems. Every stop quiesced with `cancelled` or `deadline_exceeded`, no tokens and a successful recovery.
- **Output:** the hash `57822580A271832B` is identical for all daemons.

### Independent review
- **Verdict:** APPROVE WITH CONDITIONS, with no correctness defects.
- **Fixed in `2f55c88`:**
  - nesting over the Razor limit in the sweep test;
  - warm-up before timing;
  - C2 guards so the check cannot pass vacuously;
  - doc and comment accuracy.
