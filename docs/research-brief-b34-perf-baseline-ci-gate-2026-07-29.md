# Research Brief — B-34: Perf Baseline + CI Regression Gate

**Date**: 2026-07-29
**Analyst**: The Qor-logic Analyst
**Target**: B-34 — run the default-feature benches in CI and establish a perf baseline, so
the optimization initiative (B-35..B-38) is measured. First cycle of the optimization pass.
**Scope**: which benches are CI-runnable, the regression-gate mechanism, and CI wiring.

---

## Executive Summary

Seven of the eleven benches are CI-safe (model-free, GPU-free, default-feature). A `bench`
CI job can run them on ubuntu today. But a **committed absolute-timing baseline is unsound**
— criterion measures nanoseconds that differ 2–10× between a CI runner and a dev machine, so
a hard threshold against a checked-in baseline would be pure noise. A real regression gate
therefore needs **run-over-run CI-persisted baselines** (GitHub Actions cache + `critcmp`)
with a threshold tuned to these benches' observed CI variance. Recommendation, measurement-
first: **B-34 = a smoke + baseline CI job** (run the 7 CI-safe benches; fail on compile
error/panic; upload criterion output as the baseline artifact) — sound, non-flaky, and the
prerequisite for thresholds. The **threshold regression gate** (cache + critcmp) is
sequenced as **B-34b** once CI noise is observed, so the fail threshold isn't set blind.

## Findings (verified)

### F1 — 7 benches are CI-safe (model-free, GPU-free, default-feature)
Verified by reading each bench's measured code:
- `ipc_throughput` (IPC encoding/protocol), `scheduler_throughput` + `concurrent_load`
  (`PriorityQueue` push/pop), `memory_overhead` (pool), `kv_cache_throughput`
  (`KvCacheManager` insert/lookup) — all default modules, no model.
- `generation_throughput` — despite the name, benches `generation_result_creation` /
  `streaming_output_creation` / `finish_reason_matching` (pure data-structure ops, no model).
- `inference_latency` — benches `input_validation` / `chat_validation` / `params_creation`
  (validation + struct creation, no model).

### F2 — 4 benches are excluded (model / GPU / advanced)
- `llama_cpp_comparison` (loads a real model), `gpu_allocation` (GPU allocator/pool — mock
  but GPU-modeling, conservatively excluded), `multi_gpu_scaling` (`#[cfg(feature="advanced")]`
  → compiles empty under default), `speculative_matrix` (advanced).

### F3 — the CI job MUST select specific benches
`[[bench]]` targets carry no `required-features`, so a bare `cargo bench` compiles ALL bench
targets under default features — and the advanced/gpu benches referencing feature-only APIs
would fail to compile (or run degenerate). The job must invoke each CI-safe bench explicitly:
`cargo bench --bench ipc_throughput --bench scheduler_throughput --bench concurrent_load
--bench memory_overhead --bench kv_cache_throughput --bench generation_throughput --bench
inference_latency`. (Optionally add `required-features = ["advanced"]` to the advanced bench
targets as a hardening follow-up.)

### F4 — committed absolute baseline is UNSOUND; regression needs run-over-run
- `harness = false` benches use criterion, which writes `target/criterion/<group>/<bench>/new/
  estimates.json` (mean/median in ns). These are **machine-absolute**; a baseline captured on
  the Windows dev host (or any single machine) is meaningless on the ubuntu CI runner. A
  threshold against a checked-in JSON would false-positive/negative by hardware, not by code.
- Sound options (CI-to-CI on the same runner class): (a) **`actions/cache` the
  `target/criterion` baseline keyed by runner OS + `critcmp` current-vs-cached with a generous
  median threshold**, updating the cache on `main` — self-contained, GitHub-native, no external
  service; (b) `benchmark-action/github-action-benchmark` — stores history in gh-pages/
  artifacts (heavier, external surface). Prefer (a).

### F5 — no regression tooling exists yet; CI runs fmt/clippy/test only
`.github/workflows/rust.yml` has no bench job; no `critcmp`/baseline in the repo. The bench
job is net-new. CI triggers only on PR/push to `main` (per the repo workflow), so the new
job is CI-gated: verified by pushing + a PR-to-main run.

## Blueprint Alignment

| Optimization-brief expectation | Finding | Status |
|---|---|---|
| Gate the default-feature hot benches | 7 CI-safe benches identified | MATCH |
| Lightweight committed-baseline threshold script | UNSOUND across hardware (F4) | DRIFT → run-over-run cache+critcmp |
| Non-flaky | Absolute-baseline threshold would be pure noise | RISK → smoke gate first, threshold tuned later |

## Recommendations (measurement-first; scope fork at cycle start)

1. **B-34 (recommended, bounded, sound)**: add a `bench` job to `rust.yml` (ubuntu, on
   PR/push to `main`) that runs the 7 CI-safe benches with a short measurement time
   (`--warm-up-time`/`--measurement-time` trimmed for CI budget), **fails on compile
   error/panic** (kills bench-rot — the real day-one risk that benches silently stop
   compiling), and uploads `target/criterion` as the run's baseline artifact. Documents the
   canonical CI-safe bench list. This IS the "baseline" (a reproducible CI baseline record)
   and the gate foundation.
2. **B-34b (follow-up, seeded)**: the threshold regression gate — `actions/cache` the
   criterion baseline keyed by runner OS + `critcmp` current-vs-cached, failing on a generous
   median regression (threshold set from B-34's observed CI variance, e.g. +30–50%), updating
   the cache on `main` merges. Deferred deliberately so the threshold is data-driven, not
   guessed.
3. **Fold B-06**: B-06 (backend perf benchmark harness) overlaps — the CI-safe bench list +
   the `bench` job satisfy its harness intent for default-feature paths; mark B-06 addressed
   by B-34 (model/GPU/advanced perf benches remain manual/local).

## Updated Knowledge (Shadow Genome)

**Perf-gate soundness**: a checked-in absolute-timing baseline cannot gate CI regressions —
timings are hardware-relative, so a baseline is only valid run-over-run on the same runner
class. Establish "benches run + observed in CI" (B-34) before "auto-fail on threshold"
(B-34b); set the threshold from measured CI noise, never guessed.

---

_Research complete. B-34 = CI bench smoke+baseline job (7 CI-safe benches); B-34b = the
run-over-run threshold gate, seeded. Findings advisory; the soundness constraint (F4) is
load-bearing._
