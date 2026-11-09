# HOL Phase 2: concurrency, thread oversubscription and multi-model measurements (v1.0.0)

**Build:** #177 head `b4adec2` (Phase 1 `4485cee` + Phase 2 `0e4f3de`), `gg-core-cli` release build, features `onnx,gguf`, Rust 1.97.
**Host:** offline Hyper-V guest `coreforge-build`, Windows Server 2025, **12 vCPU, 24 GiB RAM**, CPU only.
**Harness:** `scripts/hol_bench/` (stdlib Python over the authenticated named pipe; per-frame timestamps; daemon private bytes and CPU sampled every 100 ms).
**Models:** verified by hash against `PROVENANCE.json` and hard-linked into the confined `models/` tree:
- Qwen2.5-0.5B Q4_K_M
- SmolLM2-1.7B Q4_K_M
- Phi-3.5-mini Q4_K_M
- MiniLM (ONNX)
- DistilBERT-SST2 (ONNX)

`GG_CORE_N_CTX=4096`; all other limits at their defaults unless stated.

These figures come from one VM on one day. They are evidence for #177 and #161, not a qualified performance claim. Linux and macOS are out of scope.

## 1. Concurrency sweep (Qwen2.5-0.5B, one resident model)
**Workload:**
- 8 fixed prompts in ChatML.
- Greedy and seeded: `temperature 0`, `top_k 1`, `max_tokens 128`.
- Per daemon: a warm-up, then a serial reference pass, then a closed loop with `max_concurrent` clients and max(16, 3N) streaming requests.
- Token counts come from a deterministic non-streaming pass: 128 tokens on 6 prompts, 124 and 12 on the other two.

**Columns:**
- *TTFT*: request sent → first sanitized text frame.
- *Total*: request sent → final frame.
- *Decode tok/s*: tokens ÷ (total − TTFT), per request (median).
- *Identical*: concatenated stream text byte-identical to the serial baseline taken at `max_concurrent=1`.

| `GG_CORE_MAX_CONCURRENT` | `N_THREADS` per context | Threads on 12 vCPU | Aggregate tok/s | Decode tok/s per request | TTFT p50 / p95 ms | Total p50 / p95 ms | Identical | Peak private | Cores busy |
|---|---|---|---|---|---|---|---|---|---|
| 1 | auto (12) | 12 | 82.5 | 116.7 | 383 / 482 | 1 522 / 1 710 | 16/16 | 0.49 GiB | 11.9 |
| 2 | auto (12) | 24 | 85.0 | 62.0 | 706 / 911 | 2 879 / 3 454 | 16/16 | 0.90 GiB | 11.1 |
| 4 | auto (12) | 48 | 94.9 | 35.7 | 1 232 / 1 484 | 4 795 / 5 667 | 16/16 | 1.72 GiB | 11.2 |
| 8 | auto (12) | 96 | 99.9 | 19.1 | 2 348 / 3 998 | 9 051 / 11 124 | 24/24 | 3.36 GiB | 11.3 |
| 2 | 6 | 12 | **106.3** | 76.8 | 623 / 794 | 2 351 / 2 565 | 16/16 | 0.90 GiB | 11.9 |
| 4 | 3 | 12 | **149.4** | 55.2 | 874 / 1 037 | 3 196 / 3 424 | 16/16 | 1.70 GiB | 11.1 |
| 8 | 1 | 8 | **160.2** | 31.4 | 1 703 / 1 970 | 5 860 / 6 084 | 24/24 | 3.31 GiB | 7.3 |

**Findings:**
- **Correctness under concurrency.** Every stream in every configuration was byte-identical to the serial baseline: 128 of 128 concurrent streams, plus 56 of 56 per-daemon serial passes. That includes different thread counts. No request failed.
- **Memory.** Peak private bytes grow linearly with workers, at about 0.42 GiB per concurrent Qwen context at `n_ctx` 4096. That matches the admitted working-memory estimate, and all of it stays far below the 12 GiB default total.
- **Thread oversubscription.** `n_threads` is resolved once per model to all logical cores (12, capped at 16), and every concurrent context uses that same count. So N workers run 12·N compute threads on 12 vCPUs.
  - With the default thread count, adding workers barely raises aggregate throughput: 82.5 → 99.9 tok/s from 1 to 8 workers.
  - Each request slows roughly in proportion, so TTFT and total latency grow almost linearly with N.
  - Matching threads to cores (12/N per context) raises aggregate throughput by **+25 % (N=2), +57 % (N=4) and +60 % (N=8)** over the default at the same N, and cuts p95 TTFT by 13–51 %.
  - Single-request decode on this 0.5B model does not scale to 12 threads: 1 worker at 12 threads reaches 82.5 tok/s, while 8 workers at 1 thread each reach 160 tok/s using only 7.3 cores.
- **No default was changed.** A per-context thread policy, such as `n_threads = max(1, logical / max_concurrent)`, would need a plan addendum and `/ql-audit` (plan revision 2, V4). These numbers are the evidence for that decision; making it is the owner's call.

## 2. Multi-model residency and mixed concurrent load (default total)
**Setup:**
- Default total = max(2 GiB, 50 % RAM) = **12.0 GiB**, which is also the Windows Job Object cap. No per-call ceiling.
- Qwen is preloaded, then SmolLM2, Phi-3.5, MiniLM and DistilBERT are loaded over authenticated IPC.
- Three rounds of 8 simultaneous requests:
  - 2 × Qwen, 2 × SmolLM2 and 2 × Phi-3.5 generations (`max_tokens 64`);
  - 1 MiniLM embedding;
  - 1 DistilBERT classification.
- Then 2 cycles of: unload Phi-3.5 → reload → infer.
- Run twice, at `GG_CORE_MAX_CONCURRENT` 2 and 4.

| | `max_concurrent=2` | `max_concurrent=4` |
|---|---|---|
| All 5 models loaded | yes (5/5) | yes (5/5) |
| Admission refusals (load or request) | **0** | **0** |
| Failed requests | 0 / 24 | 0 / 24 |
| Peak private bytes (mixed load) | 5.68 GiB (47 % of 12 GiB) | 7.51 GiB (63 % of 12 GiB) |
| Allocation failures (daemon log) | 0 | 0 |
| Idle private: one model / all five resident | 86 MiB / 2 442 MiB | 87 MiB / 2 442 MiB |
| Idle private after mixed load | 2 469 MiB (+27 MiB) | 2 472 MiB (+30 MiB) |
| Phi-3.5 unloaded (×2 cycles) | 1 206 / 1 210 MiB | 1 207 / 1 210 MiB |
| Idle at end (after reload cycles) | 2 465 MiB | 2 467 MiB |
| Phi-3.5 generation latency | 8.1–16.2 s | 7.8–9.4 s |
| DistilBERT classify latency | 0.8–8.5 s | 0.1–1.6 s |
| Daemon alive at end | yes | yes |

**Findings:**
- **The earlier result changes.** Previously, Phi-3.5 was refused with all five models resident on the 12 GiB-RAM VM, where the default total was 6 GiB. At 24 GiB RAM (default total 12 GiB), all five load, and Phi-3.5 is admitted and answers in every round with 2 or 4 workers. The refusal was budget-bound, not a defect.
- **No leak observed.**
  - Idle private bytes after the mixed load are within +27–30 MiB of idle-after-loads, and that value stays flat (−4 MiB) across two Phi-3.5 unload/reload cycles.
  - Unloading Phi-3.5 returns about 1.23 GiB each time.
- **No admission pressure was reached.** Zero refusals means the 12 GiB total was never approached (peak 63 %). Refusal behaviour at the limit is covered by the #174/#176 qualification, not by this run.
- **Typed requests queue behind generations with 2 workers.** DistilBERT waits up to 8.5 s because both workers are busy with multi-second generations. With 4 workers it waits at most 1.6 s. That is expected for a bounded pool. Queue rotation keeps typed work from being starved, but it cannot make a request start before a worker is free.

## Not covered
- Linux and macOS.
- GPU.
- Admission at the memory limit under concurrency (see the #174/#176 qualification).
- Prompts longer than the fixtures, or contexts other than 4096.
- Repeated runs for variance: this is a single run per configuration.

## 3. Per-context thread policy (Addendum A), re-measured
**Build:** `feat/v1-thread-policy-20261109-no-ci` at `cd5f550` (stacked on `51d0fa4`), `gg-core-cli` release build, `onnx,gguf`. Same VM (12 vCPU, 24 GiB), harness (`run.py … policy-all`), models and workload as sections 1 and 2.
**VM gate before measuring:** fmt and clippy (`onnx,gguf,ffi`, `-D warnings`) clean; `cargo test --features onnx,gguf,ffi` 1516 passed, 0 failed, 1 ignored (pre-existing); default `--lib` 753 passed.
`GG_CORE_N_THREADS` is unset unless stated, and each row's resolved count was read from `gg-core-cli config show` under the same environment.

| `GG_CORE_MAX_CONCURRENT` | Resolved threads per context (`config show`) | Aggregate tok/s | Before: default threads (§1) | Before: explicit 12/N (§1) | Decode tok/s per request | TTFT p50 / p95 ms | Total p50 / p95 ms | Identical | Peak private |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 12 (auto) | 93.4 | 82.5 | — | 136.0 | 358 / 460 | 1 322 / 1 463 | 16/16 | 0.49 GiB |
| 2 | 6 (auto) | **102.2** | 85.0 | 106.3 | 81.1 | 670 / 1 023 | 2 349 / 2 956 | 16/16 | 0.90 GiB |
| 4 | 3 (auto) | **160.2** | 94.9 | 149.4 | 64.8 | 845 / 1 011 | 2 935 / 3 172 | 16/16 | 1.70 GiB |
| 8 | 1 (auto) | **163.3** | 99.9 | 160.2 | 31.5 | 1 625 / 1 858 | 5 729 / 6 025 | 24/24 | 3.31 GiB |
| 4 | 12 (explicit override) | 109.7 | 94.9 | — | 39.7 | 1 126 / 1 336 | 4 327 / 4 593 | 16/16 | 1.72 GiB |

**Findings:**
- **Acceptance gate (plan Addendum A): met.**
  - Every stream in every row is byte-identical to the serial baseline (88 of 88 concurrent streams, plus 40 of 40 per-daemon serial passes). No request failed.
  - Aggregate throughput at N ≥ 2 is above the earlier default-thread rows: **+20 % (N=2), +69 % (N=4), +63 % (N=8)**. It is within run-to-run noise of the earlier explicit 12/N rows, which the policy reproduces by construction.
- **Run-to-run variance is about 13 %.** N = 1 runs the identical configuration as in §1 (12 threads), yet measured 93.4 tok/s against 82.5. Treat differences of that size as noise. Each row is still a single run.
- **Latency.**
  - N = 4: p95 TTFT fell from 1 484 ms to 1 011 ms, and p95 total from 5 667 ms to 3 172 ms.
  - N = 8: p95 TTFT fell from 3 998 ms to 1 858 ms, and p95 total from 11 124 ms to 6 025 ms.
  - N = 2: p95 total fell from 3 454 ms to 2 956 ms. p95 TTFT rose from 911 ms to 1 023 ms; with 16 samples, p95 is the second-highest value, so this is within noise but is reported as measured.
- **Override honoured.** `GG_CORE_N_THREADS=12` at N = 4 resolves to 12 per context, as `config show` reports. It reproduces the oversubscribed behaviour: 109.7 tok/s and p95 TTFT 1 336 ms, against 160.2 tok/s and 1 011 ms for the auto value.
- **Memory unchanged.** Peak private bytes match §1 within 1 % at every N. The thread count does not change admitted working memory, as the plan's security review states.
- **N = 8 uses 8 of 12 cores** (1 thread × 8 contexts, 7.3 cores busy), because integer division leaves 4 cores idle. It is still the highest-throughput row. A remainder-distributing split is a possible later refinement and is not part of this change.

### Multi-model under the policy (default total 12 GiB)
| | `max_concurrent=2` (6 threads per GGUF context) | `max_concurrent=4` (3 threads per GGUF context) |
|---|---|---|
| All 5 models loaded / admission refusals | 5/5 / **0** | 5/5 / **0** |
| Failed requests | 0 / 24 | 0 / 24 |
| Peak private bytes (mixed load) | 5.67 GiB (47 %) | 7.49 GiB (62 %) |
| Allocation failures (daemon log) | 0 | 0 |
| Idle: all five resident → after mixed load → end | 2 442 → 2 469 → 2 465 MiB | 2 442 → 2 470 → 2 468 MiB |
| Phi-3.5 unloaded (×2 cycles); reload and infer | 1 206 / 1 208 MiB; ok | 1 207 / 1 210 MiB; ok |
| Phi-3.5 generation latency (before, §2) | 6.3–10.4 s (8.1–16.2 s) | 9.6–10.9 s (7.8–9.4 s) |
| DistilBERT classify latency (before, §2) | 1.1–3.1 s (0.8–8.5 s) | 0.09–0.9 s (0.1–1.6 s) |

- **No change to admission, leaks or memory.** There were zero refusals, zero allocation failures, flat idle memory, and the same unload return as §2.
- **Larger models at 3 threads.** At N = 4 each Phi-3.5 (3.8B) request is somewhat slower than under the old 12-thread contexts (up to 10.9 s against 9.4 s), while typed requests and N = 2 generations got faster. Splitting cores trades per-request speed for fairness and aggregate throughput, as intended. Operators who prefer per-request speed for large models can set `GG_CORE_N_THREADS`.
