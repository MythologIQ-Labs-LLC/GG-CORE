# N7 successor: deterministic cancellable prefill schedule (#184)

**Status:** plan for L2 audit. Not yet implemented.
**Base:** `fix/n7-bounded-cancel-20261110-no-ci@55d9fd0`, the C2 producer. That commit and the `d64b5662…` daemon stay unchanged as the rollback.
**Branch:** `fix/n7-deterministic-prefill-20261112-no-ci`.
**Owner decision (2026-11-12):** disposition (b) from the #184 integration review. Use a deterministic per-configuration schedule instead of timing-paced chunks.

## Problem (confirmed)
`PrefillPacer` (`engine/gguf/prefill_pacer.rs`) sets each prefill chunk's size from the measured wall time of the previous chunk. llama.cpp CPU logits depend on how the prompt is split into decode batches.

Review experiment `d184`: real Qwen2.5-0.5B Q4_K_M, 64 greedy tokens, 4 threads and 1 thread.

| Prompt | Fixed 512 vs fixed 37 | Fixed 512 vs 16-token floor |
|---|---|---|
| 2516 tokens | identical | identical |
| 568 tokens | identical | identical |
| 100 tokens | diverge at generated token 63 | diverge at generated token 39 |

The divergence is the same at both thread counts. A paced split changes with host load, so identical greedy requests can return different text.

There is a second, older inconsistency. `decode_prompt` uses a 512-token cap only when a stop check is present, and otherwise uses the native batch size. Embedded `Runtime::infer` without a stop and the daemon could therefore split the same prompt differently.

## Rejected alternative: llama.cpp abort callback
`llama_set_abort_callback` interrupts a decode between graph nodes, which would allow fixed 512 chunks to be stopped mid-chunk. The vendored safe wrapper `llama-cpp-2 0.1.161` does not expose it:
- `LlamaContext::context` is `pub(crate)`.
- `LlamaContextParams::context_params` is `pub(crate)`.

Reaching it would need a patched or forked crate (`[patch.crates-io]`) plus `unsafe` FFI on a raw context pointer. That is a supply-chain and maintenance decision of its own. It is recorded as a separate candidate (follow-up issue) and is out of scope here.

## Design
1. **New `engine/gguf/prefill_schedule.rs`.** It replaces `prefill_pacer.rs`.
   - It provides a pure function `chunk_len(cap, n_threads, model_bytes) -> usize` with no time input:
     - `estimate = REF_TOKENS_PER_THREAD_PER_SECOND × n_threads × REF_MODEL_BYTES / model_bytes × BUDGET_SECONDS`;
     - `chunk = clamp(pow2_floor(estimate), MIN_CHUNK = 16, cap)`.
   - Powers of two limit the number of distinct split shapes. The constants are calibrated from measurement (step 3).
   - The schedule is one fixed chunk size for the whole prompt, so the same configuration always gives the same split.
2. **One schedule for every plain prompt prefill.** It applies to `generate_cancellable`, `sample_loop` and `generate_stream` through `decode_prompt`, whether or not a stop check is supplied. `decode_prompt` takes the schedule's chunk instead of choosing a cap from the presence of a stop check.
   - `n_threads` is the backend's resolved value.
   - `model_bytes` is `model.size()`.
   - Unchanged: the all-position logits path, `generate_from_tokens`, verify, and the speculative session. They keep `decode_range` without a stop check, because their outputs belong to the separate speculative contract.
3. **Calibration (batch `n7d-cal`).**
   - **Setup:** Qwen2.5-0.5B Q4_K_M, plus at least one larger GGUF if one is available offline (see Open items).
   - **Matrix:** chunk sizes {16, 32, 64, 128, 256, 512} × threads {1, 2, 4, 8} × contexts {8192, 16384}.
   - **Prompt:** long enough to reach about 90% of the context, so that late, slow positions are measured.
   - **Recorded:** the worst chunk time for each cell.
   - **Choice:** the constants so that the worst chunk is at most about 1 s (the owner-accepted target) for the measured cells. If late positions exceed 2× the target, add a deterministic position step (halve the chunk past `n_ctx/2`). The step is a pure function of position, never of time.
   - The calibration table is recorded in the module documentation.
4. **Determinism contract, documented.** Plain prompt prefill is reproducible when all of these are the same: model file, `n_threads`, `n_ctx`/`n_batch`, prompt and sampling settings. It does not depend on host load, cancellation support or concurrency. Output may differ across thread counts, because the split differs.
5. **Tests, always on.**
   - Unit:
     - schedule monotonicity, bounds, pow2;
     - the floor for huge models and the cap for tiny ones;
     - no overflow at extreme inputs;
     - the position-step boundaries, if a step is added.
   - Real model:
     - **Load-independence:** prefill the same prompt three ways: no stop; a stop callback that sleeps 50 ms per check (simulated load); and with a concurrent request. Assert identical greedy tokens (64 tokens, the 100-token prompt from `d184` plus a long prompt). The test fails, rather than skips, when `GG_CORE_REQUIRE_REAL_MODEL=1`.
     - **Tail:**
       - The existing bounded-tail test asserts `tokens_generated == 0`, so the stop must land in prefill.
       - A host-independent check counts stop-check calls and asserts that the count equals `ceil(tokens / chunk)`.
       - The chunk at 1 thread must be at most 64.
     - **Concurrency:** `backend_cancel_tests` reference and survivor equality is now valid by construction. Keep it.
6. **Razor.**
   - Move `decode_prompt`, `decode_range`, `decode_range_checked`, `decode_paced` → `decode_chunked`, `decode_batch_cap` and `decode_chunk` into `engine/gguf/prefill.rs` (about 130 lines). `backend.rs` goes from 739 to about 610 lines; it was already over 250 before this change, and that is noted.
   - `decode_chunked` becomes private, behind a `#[cfg(test)]` wrapper that keeps `decode_batch_cap` validation (review nit).
   - All new functions are at most 40 lines with nesting of at most 3. New files are at most 250 lines.
7. **No changes to:**
   - IPC, protocol or public API (`StopControl::overdue` stays);
   - trace events, except that `cancel_flag` stays as is;
   - dependencies, CI or release.

## Qualification
1. **Source gate:** fmt, 5× clippy `-D warnings`, the 3 test suites, and a release gguf build, on the offline VM with rustc 1.99.1.
2. **Determinism:** rerun the `d184` experiment against the new schedule, plus the always-on test.
3. **Stop tail and throughput.** Run the N7 packaged stop harness at 1/2/4 threads and 8192/16384 context. Compare three arms:
   - C2 (`d64b5662…`, pacer);
   - fixed 512 (C1-equivalent behaviour);
   - this branch.

   Report the worst post-stop CPU tail and prefill throughput. **Acceptance:** worst tail at most 1.5 s on the qualification VM at 1 thread, and prefill time no worse than C2 + 10% at default threads. Otherwise report and return the decision to the owner.
4. Independent review of the exact diff. Then push and post on #184. No merge without owner authorization.

## Open items for the audit
- Only Qwen2.5-0.5B is currently staged on the VM. The model-size term of the formula needs a second, larger licensed GGUF to be checked rather than extrapolated. If none can be staged offline, the size term is calibrated on one model, and that is reported as unqualified for larger models.

---

## Revision 2: calibration results, fixes for audit findings V1–V4, and dropping the thread dependency (2026-11-12)

Where this revision conflicts with Revision 1's Design steps 1–3 or step 5, this revision wins.

### Calibration (batch `c184`, raw data in `N7_PREFILL_CALIBRATION_c184.txt`)
Setup: Qwen2.5-0.5B Q4_K_M (`model.size()` = 485 452 288) on a 12 vCPU VM. Each cell times one decode chunk, at position 0 and at 85% of `n_ctx`, for threads {1, 2, 4, 8} × contexts {8192, 16384}.

Time for one chunk, in ms, by chunk size:

| Position | Threads | 16 | 32 | 64 | 128 | 256 | 512 |
|---|---|---|---|---|---|---|---|
| 0 | 1 | 283–299 | 355–361 | 646–676 | 1217–1452 | 2385–2522 | 4863–5043 |
| 0 | 4 | 126–128 | 121–126 | 201–203 | 373–375 | 759–780 | 1483–1547 |
| 6963 (8k) | 1 | 957 | 1899 | 1274 | 2564 | 5103 | 10491 |
| 6963 (8k) | 4 | 276 | 550 | 547 | 878 | 1482 | 2737 |
| 13926 (16k) | 1 | 1748 | 3480 | 1959 | 3998 | 8399 | 18647 |
| 13926 (16k) | 2 | 904 | 1787 | 1217 | 2123 | 3915 | 8192 |
| 13926 (16k) | 4 | 474 | 964 | 851 | 1292 | 2282 | 4935 |
| 13926 (16k) | 8 | 305 | 600 | 729 | 901 | 1597 | 3009 |

What the data shows:
1. **Small chunks cost little throughput at low positions.** Chunks of 64–512 tokens decode at the same rate there, 0.32–0.34 tokens/ms at 4 threads. Rates drop below 64. A 128-token chunk therefore loses about 0% prefill throughput against 512. At late positions, sizes 128–512 are within 10% of each other, and 64 is 24–32% slower.
2. **Position dominates the tail.** The cost of a chunk rises up to 4× with position, because attention grows with it. No single size meets about 1 s everywhere, so the schedule must depend on position.
3. **32 is anomalous.** At late positions, a 32-token chunk is slower in absolute time than a 64-token chunk at every thread count. Schedule sizes are therefore drawn from {16, 64, 128, 256, 512}, and 32 is never chosen.

### Design change: the chunk no longer depends on thread count
Revision 1 keyed the chunk on `n_threads`. The audit pointed out, and run d184 confirms, two facts:
- thread count alone does not change logits;
- the daemon's automatic thread count varies with the host's core count and the worker count.

Revision 2 therefore removes `n_threads` from the schedule. The schedule is a pure function with no time or thread input:

```
prefill_chunk(pos, model_bytes, cap) -> usize
  base  = 128 if pos < 4096 else 64          // position step (calibrated)
  scale = REF_MODEL_BYTES (485_452_288) / model_bytes as an integer ratio, at least 1
  chunk = snap_down(base / scale) to {16, 64, 128}, then min(cap)   // never 32
```

Each chunk is sized from the position where it starts. The split therefore depends only on the prompt length and the model.

**Contract.** Given the same model file, prompt and sampling, plain prompt prefill uses the same split under any of these conditions:
- any host;
- any thread count;
- any worker count;
- any load;
- with or without cancellation.

The output is then identical, to the extent that llama.cpp's CPU kernels are deterministic for a fixed split. That held across 1 and 4 threads in d184. It is not guaranteed across CPU instruction sets.

**Expected stop tail.**

| Threads | Position | Chunk | Expected tail |
|---|---|---|---|
| 6, the daemon default on this VM (12 logical ÷ 2 workers) | below 4096 | 128 | about 0.32–0.37 s (interpolated between 4 and 8 threads) |
| 6 | 16k late | 64 | about 0.73–0.85 s |
| 1 | below 4096 | 128 | 1.2–1.45 s; C2 measured about 1.2 s with 2.5k-token prompts |
| 1 | 8k late | 64 | 1.27 s |
| 1 | 16k late | 64 | 1.96 s |

The C2 pacer could not do better at 1 thread and 16k late: its 16-token floor costs 1.75 s. This limit is reported, not hidden.

**Throughput cost against fixed 512.**
- About 0% below position 4096.
- About 24–32% for the part of a prompt beyond position 4096.
- §Qualification restates the acceptance per prompt length.

**Larger models and GPU.**
- The size term is calibrated on one 0.49 GB model. No second licensed GGUF is staged offline, and downloading one needs owner approval. Larger models get proportionally smaller chunks, down to 16, by an extrapolation that is unqualified, and the docs say so.
- GPU offload (`n_gpu_layers > 0`) uses the same schedule. Its chunks are smaller than needed, but determinism is kept, and GPU is outside RC scope. This is documented.

### V1: a structural determinism test replaces "sleep in callback"
- A `#[cfg(test)] fn decode_chunked_recorded(...) -> Vec<usize>` wraps the private `decode_chunked`. It returns the sequence of chunk lengths after `decode_batch_cap` validation.
- **Unit test, no model needed.** The planned sequence from `prefill_chunk` must equal hand-computed expectations for lengths {1, 63, 64, 100, 4095, 4096, 4097, 9000, 16000}. It must never contain 32, and every chunk must be at most `cap`.
- **Real-model test.** The same prompt runs three ways:
  - with no stop callback;
  - with a stop callback that never fires;
  - with that callback while 8 spinner threads saturate the CPU *during* decode.

  The test asserts that all three record the same chunk sequence, equal to the planned one. It also asserts identical 64-token greedy output, for the d184 100-token prompt and a 2516-token prompt. This test **fails against the C2 pacer**, whose sequence starts at 32 and then varies with load.

### V2: the tail test, corrected
**Correction.** `backend_cancel_latency_tests.rs::real_model_single_thread_prefill_stop_returns_within_bounded_tail` does **not** assert `tokens_generated`. That check exists only at `backend_cancel_tests.rs:101`.

**Changes:**
- Add `assert_eq!(r.tokens_generated, 0)`.
- Under the require gate (V3), both skip branches become failures: a missing fixture, and a request that is not `Cancelled`.
- Set the stop to fire at 1.0 s, so that it lands inside prefill. At 1 thread, the 2516-token prompt prefills for about 25 s.
- The limit stays at 5 s.

**Host-independent check.** The V1 recorded chunk sequence for that prompt must have a maximum of at most 128. That proves pacing happened, independent of wall-clock time.

### V3: a shared real-model gate
- New `engine/gguf/test_fixture.rs` (`#[cfg(test)]`) provides `real_model() -> Option<&'static Path>`.
- If the fixture is missing it returns `None` (skip), unless `GG_CORE_REQUIRE_REAL_MODEL=1`, in which case it panics.
- Every real-model test touched here uses it: `backend_cancel_tests` (cancel and concurrency), the latency and tail tests, and the new determinism test.
- The VM qualification jobs set `GG_CORE_REQUIRE_REAL_MODEL=1`.

### V4: exact count, at the right level
- The stop-check count is asserted at the `decode_chunked` level: the number of chunk lengths recorded equals `planned.len()`, with one check before each chunk.
- No count is asserted for `generate_cancellable`. It also checks before prefill and once per generated token.

### Orphans: test rewrites
- **`backend_cancel_latency_tests.rs`:**
  - Drop `PrefillPacer` and `with_budget`.
  - The opt-in characterization (`GG_N7_MEASURE=1`) sweeps fixed chunks {16, 64, 128, 256, 512} and the schedule, at an early and a late position with KV rollback, as in c184.
  - `PREFILL_CHECKPOINT_TOKENS` is renamed `MAX_PREFILL_CHUNK` (512, the cap).
- **`mod.rs`:** `mod prefill_pacer` is replaced by `mod prefill` and `mod prefill_schedule`. `prefill_pacer.rs` is deleted.
- **`backend_cancel_tests.rs`:** the concurrency reference runs with no stop callback, which also covers the unified schedule with and without stop. The "time-paced" comment is fixed.

### Wiring and file-size limits
- **New method:** `LlamaBackendInner::prefill(&self, ctx, tokens, should_stop) -> Result<bool>` reads `self.model.size()` and calls the schedule, so `generate_stream` and `sample_loop` do not grow.
- **File split:** the decode functions move to `engine/gguf/prefill.rs`, about 170 lines. `backend.rs` shrinks from 739 to about 575 lines. It was already over the 250-line file limit before this change, and `generate_stream` (48 lines) and `verify_tokens` (66 lines) were already over the 40-line function limit; both are left unchanged.
- **Limits:** new files are at most 250 lines, new functions at most 40 lines, and nesting at most 3.

### Compatibility note
- Embedded callers without a stop callback move from the native 512 cap to the schedule. For prompts longer than one chunk, greedy text may differ from C1 or RC1, as seen in d184.
- This is a one-time change made for reproducibility. It is recorded in the release notes next to the RC rollback text.
- Speculative, verify and all-position-logits paths are unchanged.

### Qualification acceptance (restated)
1. **Determinism.** The V1 tests pass. The d184 experiment, rerun on the new schedule, shows identical output across the 3 prompts × {no stop, stop} × {1 thread, 4 threads}.
2. **Stop tail** (N7 packaged harness; contexts 8192 and 16384; 1, 2 and 4 threads; the harness's 2.5k-token prompts). The worst post-stop CPU tail is at most 1.5 s at 1 thread and at most 0.6 s at the default thread count.
3. **Throughput.** Prefill of the 2516-token prompt at the default thread count is no worse than C2 + 10%.
4. **Long prompt.** A prompt at about 85% of a 16k context is measured and reported against fixed 512 and C2. The expected cost is about +25%. This is reported as a trade-off, not a gate.
5. **Misses.** Any miss is reported to the owner. Nothing is tuned around it.

---

## Revision 3: remediation of re-audit R1–R4 (2026-11-12)

This revision supersedes Revision 2's schedule pseudocode, tail table, throughput claims and acceptance figures. Revision 2's V1–V4 test remediation, orphan list, gate, wiring, Razor and compatibility sections still apply, with the residual conditions listed at the end.

### Design decision: per-configuration determinism, thread bucket included
Revision 2 made the chunk independent of thread count, which the audit did not require. The re-audit's R2 and R3 show the cost of doing that. A single thread-blind schedule cannot keep the 1-thread tail and the high-thread late-position throughput both acceptable. On one host, 512-token chunks run from 1.25 s (8 threads, position 0) to 18.6 s (1 thread, 16k late).

Revision 3 therefore returns to the owner's chosen disposition (b) as written: **a deterministic schedule per configuration, chosen from thread count and context**.

The contract becomes:
- Plain prompt prefill uses the same split for the same model file, the same **thread bucket** ({1}, {2, 3}, {4 to 7}, {8 and up}), the same prompt and the same sampling, on any load, with or without cancellation, and at any concurrency.
- Output may differ between thread buckets, because the split differs.
- In d184, the thread count alone did not change output.

### Cost model (the calibration data used directly; replaces the R1 formula)
From c184, for each candidate size `s` in {64, 128, 256, 512} and each thread bucket `t` in {1, 2, 4, 8}:
- `T0[s][t]` is the mean of the two position-0 measurements;
- `T16[s][t]` is the measurement at position 13 926.

Predicted chunk time at start position `p`:

`ms(s, t, p) = (T0 + (T16 − T0) × p / 13926) × model_bytes / REF_MODEL_BYTES`

- `REF_MODEL_BYTES` = 485 452 288.
- All arithmetic is u128 integer.
- Positions beyond 13 926 extrapolate along the same line.

The line is a check against measurement, not an assumption. At position 6963, 1 thread, s = 128, it predicts 2666 ms against 2564 ms measured; at 4 threads, 848 ms against 878 ms measured.

Thread buckets map to the table row 1, 2, 4 or 8 by rounding **down**. Rounding down uses the slower row, so predictions are conservative.

### Schedule (pure function: no time input)
```
chunk(p, t_bucket, model_bytes, cap):
  for s in [512, 256, 128, 64]:            // largest first
      if s <= cap and ms(s, t_bucket, p) <= 1000: return s
  return min(64, cap)                       // floor; if cap < 64, cap itself
```

**Why sizes below 64 are excluded (corrects R4).** Per token, 16 and 32 are less efficient at every measured cell:
- At 16k-late, 16 tokens cost 109 ms/token at 1 thread against 30.6 for 64. At 8 threads the figures are 19.1 against 11.4 ms/token.
- 32 is faster than 64 in **absolute** time at 8 threads (600 against 729 ms at 16k, 344 against 440 at 8k), but slower per token (18.8 against 11.4 ms/token). At 1, 2 and 4 threads it is slower in both.

Dropping below 64 would trade at most about 15% of the tail for 1.6 to 3.6 times the prefill time of that part of the prompt.

**Remainders.** The last chunk is `min(chunk, remaining)`. Each chunk is sized by its own start position, and decoding always starts at 0, so the sequence is fully determined by `(prompt length, t_bucket, model_bytes, cap)`.

**Model scaling.** A larger model raises `ms` linearly with model bytes, so it gets smaller chunks, down to the 64 floor. A smaller model gets larger chunks, up to `cap`. This fixes R1: the direction is now correct and there is no truncation cliff. Scaling is calibrated on one 0.49 GB model only. It is **unqualified** for other sizes and quantizations, and is documented that way.

### Predicted behaviour, Qwen2.5-0.5B on the c184 host (corrects R2 and R3)
These figures were computed by `scripts/qualification/n7_prefill_schedule_model.py` from `N7_PREFILL_CALIBRATION_c184.txt`. The worst-chunk value includes the partial last chunk. Fixed 512 is modelled with the same lines.

**Line fit against measurement at position 6963.** For 64/128/256: within -6% to +5% of the measured value at every thread count. For 512: +12% at 1 thread and +18% at 4 threads, which overestimates the time and is therefore conservative.

| Bucket | Schedule (start position, chunk) | 2516-token prompt: worst chunk / prefill time vs fixed 512 | 13 926-token prompt | 16 300-token prompt |
|---|---|---|---|---|
| 1 | 64 everywhere | **0.89 s / +4%** | 1.95 s / -9% | **2.17 s / -11%** |
| 2 | 128 to p 3120, then 64 | **0.92 s / +5%** | 1.21 s / +17% | 1.36 s / +18% |
| 4 (default, see below) | 256 to 2128, 128 to 9504, then 64 | **0.99 s / +2%** | 1.00 s / +19% | 1.00 s / +23% |
| 8 | 256 to 5648, 128 to 16 304, then 64 | **0.74 s / +1%** | 1.00 s / +14% | 1.00 s / +15% |

**What this means:**
- **Short prompts:** the schedule costs at most +5% throughput, with a worst stop tail of at most 1 s.
- **Long prompts at 2 or more threads:** prefill costs +14% to +23% in exchange for a stop tail of at most about 1.4 s (at most 1 s at 4 or more threads).
- **1 thread:** the 64-token floor gives a tail of about 2.2 s at 16k. That is the floor's cost. Going to 16-token chunks would take it to about 1.75 s at 3.6 times the per-token cost.

**Default threads.** The configuration documents the daemon auto rule as `max(1, min(logical, 16) / GG_CORE_MAX_CONCURRENT)` (`config.rs` table), with `GG_CORE_MAX_CONCURRENT` defaulting to 2. That gives 6 threads, bucket 4, on the 12-vCPU VM. The implementation step must verify this against `backend_dispatch.rs:140-143` and record the resolved value in the qualification log, because the audit did not verify it.

### Acceptance (replaces Revision 2's Qualification acceptance)
1. **Determinism**
   - The V1 structural test passes. Its recorder must observe the production path (residual 1).
   - Rerunning d184 on this branch gives identical output across the 3 prompts, with and without a stop, for every bucket, under both no load and contention.
2. **Stop tail.** Run the N7 packaged harness at contexts 8192 and 16384, at 1, 2 and 4 threads, with its 2.5k-token prompts. The worst post-stop CPU tail must be:
   - ≤ **1.2 s** at 1 thread (predicted 0.89 s);
   - ≤ **1.2 s** at 2 threads and at the default threads (predicted ≤ 1.0 s).
3. **Throughput.** Prefill of the 2516-token prompt at default threads must be no worse than C2 + 10% and no worse than fixed 512 + 10%.
4. **Long prompts (reported, not gated).** A prompt at about 85% of 16k is measured at buckets 1 and 4. The tail and the prefill time are compared against fixed 512 and C2, alongside the predicted values above.
5. **Prediction error.** Any measured value more than 25% worse than predicted is reported, and the constants are then re-derived from a fresh calibration. They are not hand-tuned.

### Unit tests added for R1
- Model bytes at 0.5×, 1×, 1.9×, 2×, 3×, 9× and 100× of the reference: chunk sizes are monotonically non-increasing, never below `min(64, cap)`, and never 32 or 16.
- Cap values 16, 48, 64, 100 and 512: the result is at most cap, and a cap below 64 yields cap.
- Every bucket at positions 0, 2047, 2048, 4095, 4096, 9500, 13 926 and 16 383 matches the hand-computed table above.
- Extreme inputs (`usize::MAX` positions and bytes) saturate without overflow or panic.

### Residual conditions carried from re-audit 2
1. The recorder is a `#[cfg(test)]` hook inside `decode_chunked`. It is reached through `LlamaBackendInner::prefill` by real `generate_cancellable` and `generate_stream` calls, so the production wiring is exercised.
2. The `GG_CORE_REQUIRE_REAL_MODEL` gate also covers `real_model_long_prompt_beyond_batch` (`backend.rs:617-628`).
3. The default-worker assumption is verified in code and in the logged run (see above).
