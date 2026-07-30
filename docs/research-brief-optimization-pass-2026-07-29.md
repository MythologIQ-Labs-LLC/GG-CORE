# Research Brief — Optimization Pass (opening)

**Date**: 2026-07-29
**Analyst**: The Qor-logic Analyst
**Target**: Open a governed optimization initiative on green `main` (@17a758d, ledger #148)
now that the runtime is complete + consumable. Catalog opportunities and seed a
prioritized, **measurement-first** optimization backlog.
**Scope**: the default (consumer-facing, non-`advanced`) runtime hot paths and the
benchmark/measurement infrastructure. `advanced`-feature perf machinery (TierSynergy) is
noted but out of the default-consumer scope.

---

## Executive Summary

`main` is green and consumable; the next phase is optimization. The governing constraint:
**GG-CORE has 9 criterion benches but no tracked baseline and no CI perf-regression gate**,
so any optimization today is unmeasured and unprotected against regression. This brief
opens the initiative measurement-first: (1) establish a baseline + a CI regression gate,
(2) profile the default `Runtime::infer` hot path (security-pipeline overhead, tokenize,
batching), (3) run targeted optimization cycles against profiled hotspots. It seeds
optimization backlog items **B-34..B-38** in that order. No optimization is executed here —
this is the opening (research + prioritized backlog).

## Findings (verified)

### F1 — benches exist, but there is no baseline or CI gate (measure first)
- `core-runtime/benches/` has 9 criterion targets: `inference_latency`, `generation_throughput`,
  `concurrent_load`, `scheduler_throughput`, `ipc_throughput`, `kv_cache_throughput`,
  `memory_overhead`, `gpu_allocation`, `llama_cpp_comparison` (+ multi-gpu). `criterion 0.5`
  is a dev-dep. But `.github/workflows/rust.yml` runs fmt/clippy/test only — **no bench job,
  no stored baseline, no regression threshold.** Optimization without this is blind.

### F2 — the security façade adds unmeasured per-call overhead (consumer-facing)
- Every `Runtime::infer` (now the sole path, B-33) runs `SecurityPipeline::scan_prompt`
  (ingress, regex injection patterns) + `sanitize_output` (egress, `\b`-anchored PII
  regexes). Cost is unmeasured. The streaming egress sanitizer (B-24b) **re-sanitizes the
  full accumulated buffer on every push** (holdback windowing) — potentially O(n²) over a
  long stream. High-value profiling + possible incrementalization candidate.

### F3 — the fast paths are `advanced`-gated; the default runtime is the consumer target
- `flash_attn_gpu`, `simd_matmul`, `simd_tokenizer_v2`, `speculative`/`_v2`, `quantize`,
  `adaptive_speculative`, multi-gpu are `#[cfg(feature="advanced")]` (proprietary
  TierSynergy). What COREFORGE and default consumers run is the **default** feature set
  (llama.cpp for GGUF, candle for ONNX, default `simd_tokenizer`, `cache.rs` KV). Optimization
  scope should target default paths unless `advanced` is explicitly in scope.

### F4 — candidate default hot paths (to be confirmed by profiling, not assumed)
- Tokenization (`engine/simd_tokenizer.rs`, `tokenizer.rs`); scheduler + continuous batching
  (`scheduler/continuous.rs`, `queue.rs`, `batch.rs`); memory (`memory/pool.rs`, `cache.rs`,
  `prompt_cache.rs`); the ONNX embed/classify path (candle `simple_eval`); IPC framing
  (`ipc/encoding.rs`). None is a confirmed bottleneck yet — that is what B-35 measures.

### F5 — a comparison reference already exists
- `benches/llama_cpp_comparison.rs` implies an intended perf baseline against raw llama.cpp —
  a natural yardstick for the generation path and for framing "acceptable overhead".

## Blueprint Alignment

| Expectation | Reality | Status |
|---|---|---|
| Perf is measured + regression-guarded | Benches exist; no baseline, no CI gate | GAP → B-34 |
| Consumer path (`Runtime::infer`) overhead known | Security scan/sanitize cost unmeasured | GAP → B-35 |
| Streaming egress sanitizer scales to long streams | Re-sanitizes full buffer per push (O(n²)?) | RISK → B-36 (profile-gated) |

## Recommendations — the optimization backlog (measurement-first)

Seeded as backlog items; each is its own governed cycle. **Order matters — do not optimize
before B-34.**

- **B-34 (P2)** — Perf baseline + CI regression gate: add a `bench` CI job (or a scripted
  criterion run) that records a baseline and fails on a threshold regression for the
  default-feature hot benches (`inference_latency`, `generation_throughput`,
  `scheduler_throughput`). Enables every subsequent optimization to be proven, not asserted.
- **B-35 (P2)** — Profile the default `Runtime::infer` hot path: quantify the
  `SecurityPipeline` scan+sanitize overhead per call and identify the top default-feature
  hotspots (tokenize / batching / memory). Output: a ranked, evidence-backed target list.
- **B-36 (P2, profile-gated)** — Streaming egress sanitizer: if B-35 confirms the
  full-buffer re-sanitize is superlinear, incrementalize it (sanitize only the newly
  released window) while preserving the multi-word-PII-split guarantee (B-24b tests).
- **B-37 (P3)** — Scheduler/batching throughput under concurrency (guided by
  `concurrent_load` + `scheduler_throughput` baselines from B-34).
- **B-38 (P3)** — Memory-overhead reduction (pool/prompt-cache) guided by `memory_overhead`.

Cross-refs: **B-06** (backend perf benchmark harness) overlaps B-34 — fold or sequence
together. **B-21** (ADR-007 DSpark adaptive speculative) is the `advanced`-tier optimization
track, separate from this default-runtime pass.

## Updated Knowledge (Shadow Genome)

Discipline (not a failure): **open an optimization initiative measurement-first.** With
benches present but no baseline/gate, the first cycle must be instrumentation (B-34), not a
code change — otherwise "optimizations" are unfalsifiable and regressions are invisible.

---

_Research complete. This opens the optimization initiative; B-34..B-38 are seeded in the
backlog for governed execution. Findings advisory; sequencing (baseline before edits) is
the load-bearing recommendation._
