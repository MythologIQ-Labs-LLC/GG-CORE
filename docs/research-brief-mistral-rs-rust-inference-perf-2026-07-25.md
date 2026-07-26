# Research Brief

**Date**: 2026-07-25T12:32:59-04:00
**Analyst**: The Qor-logic Analyst
**Target**: mistral.rs (EricLBuehler/mistral.rs v0.9.0) and the Rust-native
inference ecosystem (candle, llama-cpp-2, burn, ort, tract, candle-vllm,
ratchet, kalosm, rustformers/llm), benchmarked conceptually against GG-CORE's
actual backend integration.
**Scope**: Performance optimization opportunities that preserve GG-CORE's
security posture (offline, IPC-only, no reqwest/hyper/WebSocket, sandboxed).
Step 2.5 note: target is an external API surface, not a GH issue — pre-check
skipped.

---

## Executive Summary

mistral.rs is a **technique quarry, not a linkable dependency**: its core crate
unconditionally links `reqwest`, `tokio-tungstenite` (WebSocket), `hf-hub`, and
an MCP network client — three forbidden-dependency violations — and pins a git
candle 0.11 incompatible with our candle 0.8. The highest-leverage findings are
closer to home: GG-CORE's production GGUF path sets only 4 of llama-cpp-2's
performance parameters and single-sequences every batch, while flash-attention,
quantized-KV, and batch-sizing knobs **already exposed by our pinned 0.1.133**
sit unused. Two drift findings against our own blueprint: the in-house perf
kernels (paged KV, Q8 KV, flash-attn, SIMD matmul) are **not invoked** by the
production GGUF decode path, and the documented security interception chain
(PII redact, output sanitize, prompt-injection scan) is **not wired** into the
production inference flow — the latter must be fixed and measured before any
performance claims, or "no security sacrifice" is unfalsifiable.

---

## Findings

### A. GG-CORE baseline reality (local verification — authoritative)

1. **Only 4 llama-cpp-2 parameters are set.** `engine/gguf/backend.rs:229-232`
   applies `n_ctx` (default 2048, `gguf/mod.rs:39`), `n_threads` +
   `n_threads_batch` (auto, capped 16, backend.rs:319-330), and `n_gpu_layers`
   (default 0, backend.rs:37). **Unset**: `n_batch`, `n_ubatch`,
   flash-attention policy, `type_k`/`type_v` (KV quantization),
   mmap/mlock, `offload_kqv`, `defrag_thold`, RoPE/YaRN scaling. All of these
   exist at our pinned 0.1.133 (docs.rs `LlamaContextParams`,
   `with_flash_attention_policy`, `with_type_k/with_type_v`,
   `with_n_batch/with_n_ubatch`).
2. **No cross-request batching.** `scheduler/worker.rs:68-70` dequeues one
   request at a time; `scheduler/queue.rs:128-136` pops singly;
   `RequestBatch`/`BatchProcessor` (`scheduler/batch.rs:22-60`) and
   `DecodeExecutor`/`PrefillExecutor` are never invoked in the worker loop.
   Every `LlamaBatch` is constructed with `n_seq = 1`
   (backend.rs:100,139,177,246). llama.cpp's multi-sequence batching is
   unused.
3. **DRIFT — in-house perf kernels are dead weight for GGUF.**
   `engine/flash_attn.rs`, `engine/simd_matmul.rs`, `memory/paged.rs`,
   `memory/kv_quant.rs` are gated behind `advanced` (engine/mod.rs:40-58,
   Cargo.toml:131) and have **zero callers in `engine/gguf/`** (grep-verified).
   The production decode runs entirely inside llama.cpp's black box
   (`ctx.decode(batch)`, backend.rs:296) with llama.cpp's own KV cache and
   attention. Prior narrative (ledger #96 brief, FEATURE_INDEX) presented
   these as GG-CORE's active optimization surface — they are parallel
   infrastructure exercised only by tests/benches in default+gguf builds.
4. **DRIFT — security interception is not in the production flow.**
   `PromptInjectionFilter`, `PIIDetector`, `OutputSanitizer` have zero call
   sites in `engine/` (grep-verified; modules exist under `security/`).
   What actually runs: admission control / resource guards at request start
   (`scheduler/worker.rs:117-127`, `engine/inference.rs:156-164`).
   ARCHITECTURE_PLAN's data flow ("engine → security/ (output sanitize, PII
   redact) → ipc/") does not match production behavior. Consequence for this
   research: current latency baselines exclude governance costs; wiring
   security later will change perf numbers, so the two must be
   measured together (issue #52's policy_intercept/audit metrics).
5. **Streaming costs per token**: fresh `StreamingOutput` + JSON
   `serde_json::to_vec` per token through a bounded tokio mpsc
   (`engine/streaming.rs:49-54`, `ipc/protocol_codec.rs:12-20`,
   `worker_streaming.rs:64-66` runs the loop on `spawn_blocking` with an
   unsafe pointer cast). Optimization headroom, but second-order vs A.1/A.2.
6. **Sampling** is llama-cpp-2's sampler chain (penalties → top_k → top_p →
   temp → dist) with a **hardcoded seed 42** (backend.rs:300-316) — not
   per-request configurable; determinism may be intentional but should be a
   config decision, not an accident.
7. **ONNX path**: `candle_onnx::simple_eval` on CPU with a naive
   whitespace-hash tokenizer (`onnx/embedder.rs:66-92,120-130`); candle
   0.8.4 pinned (Cargo.toml:32-33, lock). No batching, no real tokenizer.
8. **Versions**: llama-cpp-2 0.1.133 (19 patch releases behind 0.1.152);
   candle 0.8.4 (three minors behind 0.11.0); tokio 1.49 lock;
   interprocess 2.2.3. No `[patch]`/pins; no MSRV.

### B. mistral.rs (v0.9.0, MIT, observed 2026-07-25, master)

1. **Linkability: BLOCKED.** `mistralrs-core/Cargo.toml` unconditionally
   depends on `hf-hub` (ureq/tokio/rustls), `reqwest` 0.13,
   `tokio-tungstenite` (WebSocket), `tokenizers`, and `mistralrs-mcp`
   (network client). No feature flag removes network. Additionally pins git
   candle 0.11.0 (commit `27f20fe…`) — type-incompatible with our candle 0.8.
   Two independent hard blockers under our forbidden-dependency rules.
   Runtime-offline is supported (`pipeline/hf.rs` serves local paths without
   hub contact; `HF_HUB_OFFLINE` honored) — but link-time posture is what our
   rules govern.
2. **Speculative decoding (maps to ADR-007).** v0.9 restructured into
   `mistralrs-core/src/speculative/{cache,config,driver,logging,proposer,staging,target,verifier}`.
   Key ideas to adopt: (a) **interleaved pipeline** — the target forward pass
   verifies step N's staged draft AND yields the hidden state from which the
   proposer immediately drafts step N+1; (b) tri-state batch handling
   (homogeneous staged / mixed / none) in
   `try_sample_speculative_causal_gen`; (c) **stochastic rejection sampling**
   (accept prob `min(1, p/q)`, resample residual `(p−q)⁺`) preserving target
   distribution for non-greedy sampling; (d) speculation is *coupled to paged
   KV* (load fails without PagedAttention). Their proposer/verifier/staging/
   driver split validates ADR-007's BlockDraftModel/VerificationScheduler/
   TargetVerifier decomposition almost 1:1.
3. **Scheduler**: continuous batching with FCFS admission at every
   `schedule()` call plus **length bucketing** (`BucketKey = (cache_len,
   imgs&prompt, offset)`; run shortest bucket so laggards catch up;
   urgency flag preserves FCFS) — `scheduler/default_scheduler.rs`.
   Directly reusable pattern for GG-CORE's currently-dead RequestBatch.
4. **Prefix caching**: `prefix_cacher.rs` `PrefixCacheManagerV2` — flat
   IndexMap keyed by tokens+adapter, longest-common-prefix scan, KV restore
   via `try_set_len()`, media-hash guards, eviction only counts GPU-resident
   entries (CPU residency free). Complements llama.cpp session persistence
   (see C.2).
5. **Other techniques**: ISQ in-situ quantization at load (21 IsqType
   variants incl. HQQ/AFQ/FP8/MXFP4, imatrix calibration,
   `mistralrs-quant/src/isq_executor.rs`); PagedAttention CUDA+Metal only
   (block sizes 8/16/32, chunked prefill 4096, FP8-E4M3 KV type,
   CUDA-graph replay on decode-only steps); device layer-range mapping with
   auto CPU offload (f32 fallback); MatMul dispatch per platform
   (cublaslt module; Accelerate forces F32; CPU uses F16 intermediates).
6. **Benchmarks** (self-published, Gemma 4 E4B): ~2× llama.cpp on prefill
   (e.g., H100: 26,220 vs 11,702 tok/s), decode within ~10–25%. Directional
   only — build flags/batch config unpublished.
7. **Maturity**: MIT, workspace 0.9.0, `rust-version` 1.94, ~7.5k stars,
   10 releases in 3 weeks (API churn risk if ever depended on).

### C. Ecosystem survey (versions observed 2026-07-25)

1. **candle 0.8.4 → 0.11.0** (MIT/Apache-2.0; released 2026-06-26; no
   default features; `candle-transformers` has **no hf-hub/tokenizers
   deps** — clean offline). Buys ~14 months of kernel/op fixes; flash-attn
   feature is CUDA-only; candle-transformers can run quantized GGUF but has
   no paged attention/continuous batching — weaker than llama.cpp as a
   second GGUF backend. Upgrade is low-risk; no releases page/changelog
   (commit archaeology needed at bump time).
2. **llama-cpp-2 0.1.133 → 0.1.152** (MIT/Apache-2.0; 2026-07-21; vendored
   llama.cpp source, no build-time downloads). New since 0.1.133: **MTP
   speculative decoding**, `state_seq_save/load_file` (0.1.136 — offline
   prefix/session persistence across restarts), `fit_params` auto GPU/CPU
   layer split, mmap control (0.1.140), `llguidance` grammar-constrained
   sampling (0.1.134, pure compute), Windows CRT linking fix, Intel
   MKL/OpenCL backends. Also note `llama-cpp-sys-2` exposes a **vulkan**
   feature — the cheapest path to non-NVIDIA GPU support on Windows without
   new dependencies.
3. **tract-onnx 0.23.4** (sonos/tract; MIT/Apache-2.0; 2026-07-08; very
   active): **pure Rust, self-contained, zero network, no C++** — the best
   supply-chain match in the survey. Broader ONNX op coverage than
   candle-onnx, mature SIMD micro-kernels (x86/ARM/SVE), NNEF
   translate-once runtime. Candidate replacement/alternative for the
   candle-onnx classify/embed path; head-to-head perf UNVERIFIED — needs a
   bench.
4. **ort 2.0.0-rc.12** (ONNX Runtime bindings): highest ONNX perf ceiling
   (graph fusion, DirectML/CUDA/TensorRT EPs) but **`download-binaries` is a
   default feature** (build-time binary fetch) and `tls-native` signals
   network-facing EP code — usable only with `default-features = false` +
   operator-provisioned runtime; large C++ audit surface. Hold unless
   profiling proves pure-Rust ONNX CPU-bound.
5. **burn 0.21** (wgpu/CubeCL cross-vendor GPU): training-first, thin LLM
   inference story; watch, don't adopt — llama.cpp's vulkan feature covers
   the same goal cheaper. **candle-vllm** (same author as mistral.rs,
   active): PagedAttention/continuous-batching/CUDA-graphs reference to
   mine, not to link. **ratchet**: browser/WGSL niche. **kalosm**: wrong
   layer, cloud integrations. **rustformers/llm**: archived 2024. **No
   TensorRT-LLM Rust bindings exist.**

### D. Security-compatibility triage

| Move | Network/dep delta | Sandbox impact | Verdict |
|------|-------------------|----------------|---------|
| Turn llama-cpp-2 knobs (flash-attn, Q8_0 KV via type_k/v, n_batch/n_ubatch) | none | none — same black box, config only | SAFE, do first |
| Multi-sequence LlamaBatch + scheduler continuous batching (mistral.rs bucketing pattern) | none | none — code we own | SAFE (L2 logic) |
| Bump llama-cpp-2 → 0.1.152 | vendored C++ delta (llama.cpp submodule sync) | none | SAFE with vendoring review |
| llama.cpp `state_seq_*` prefix/session persistence | none | writes must stay in `cache/` (contract) | SAFE with path discipline |
| Bump candle → 0.11 | none (no default features, no hf-hub) | none | SAFE |
| tract-onnx as ONNX alt | +1 pure-Rust dep tree, zero network | none | SAFE, bench first |
| ADR-007 wiring using mistral.rs driver/verifier design | none (reimplement) | none | SAFE (L2) |
| Link any mistralrs-* crate | reqwest + WebSocket + hf-hub (core); candle-0.11-git conflict | violates forbidden deps | **BLOCKED** |
| ort with defaults | build-time binary download + TLS code | supply-chain | **BLOCKED as-is**; conditional later |
| llguidance grammars (llama-cpp-2 feature) | +1 dep, pure compute | none | SAFE, optional |

---

## Blueprint Alignment

| Blueprint Claim | Actual Finding | Status |
|----------------|---------------|--------|
| ARCHITECTURE_PLAN data flow: engine → `security/` (output sanitize, PII redact) → ipc | Sanitizer/PII/prompt-injection have zero call sites in engine path; only admission control runs (worker.rs:117-127) | **DRIFT (L3-relevant)** |
| ARCHITECTURE_PLAN: `engine/` = "Inference, tokenizer, streaming, GPU, SIMD" | SIMD/flash-attn/paged-KV modules exist but are `advanced`-gated with no GGUF-path callers; production GGUF perf = llama.cpp defaults | DRIFT (overstates active surface) |
| Ledger #96 brief: "Real optimization inventory (all non-stub)" | Non-stub but **non-invoked** in production GGUF builds — corrected this brief | DRIFT (self-correction) |
| CLAUDE.md/PLAN: "Recommended crates: candle or llama-cpp-rs" | Confirmed as the right horses; ecosystem survey found no compliant replacement runtime (mistral.rs blocked on deps) | MATCH |
| Scheduler contract: "priority queueing, batching" | Priority queueing real; batching = data structures only, worker loop is one-request-at-a-time (worker.rs:68-70) | DRIFT (partial) |
| Security Considerations: "Error messages don't leak info (sanitizer rules in security/)" | Sanitizer module exists; not exercised on the inference output path | DRIFT (same root as row 1) |

---

## Recommendations

1. **P1 — Wire the security chain and baseline it (prerequisite for the
   whole goal).** Route production inference output through
   OutputSanitizer/PIIDetector (and prompt-injection scan on ingress) as
   ARCHITECTURE_PLAN already claims, behind `/qor-audit` (L3 — `security/`
   surface). Simultaneously add the issue #52 governance-overhead metrics
   (policy_intercept_ms, audit_write_ms). Without this, "no security
   sacrifice" cannot be demonstrated and every perf number measured now is
   an overstatement of post-wiring reality.
2. **P1 — Knob-turning cycle on llama-cpp-2 0.1.133 (zero dep delta).**
   Make flash-attention policy, `type_k/type_v = Q8_0`, `n_batch`,
   `n_ubatch`, `offload_kqv`, mmap explicit `GgufConfig` fields; A/B with
   the existing Criterion benches + `llama_cpp_comparison`. Also lift the
   hardcoded sampler seed 42 into config. L2, small surface
   (backend.rs:226-316).
3. **P2 — Bump llama-cpp-2 → 0.1.152** after vendored-C++ review; adopt
   `state_seq_save/load_file` for prefix/session persistence under `cache/`
   (offline warm restarts), evaluate `fit_params`, and pick up the Windows
   CRT fix. Bump candle 0.8 → 0.11 in the same or adjacent cycle (low risk,
   no default features).
4. **P2 — Real batching**: implement multi-sequence `LlamaBatch` +
   continuous batching in the scheduler using mistral.rs's
   every-step-FCFS-admission + length-bucketing pattern
   (`default_scheduler.rs` as reference). This finally activates the
   scheduler's dormant RequestBatch design. L2.
5. **P2 — ADR-007 wiring blueprint**: adopt mistral.rs's
   proposer/verifier/staging/driver structure — interleaved
   verify-then-draft, tri-state staged batches, stochastic rejection
   sampling — as the implementation shape for our dormant
   BlockDraftModel/TargetVerifier traits; llama-cpp-2 0.1.152's MTP support
   may provide the backend hooks. Gate on the #52 harness for net-speedup
   evidence.
6. **P3 — Bench tract-onnx 0.23 vs candle-onnx** behind the existing
   OnnxModel trait for classify/embed; adopt only on measured win. Replace
   the whitespace-hash tokenizer with a real tokenizer regardless (accuracy,
   not just perf). Keep ort blocked unless CPU-bound evidence emerges.
7. **P3 — Decide the fate of the `advanced` kernel modules** for the GGUF
   path: either wire them to a candle-based decode path where they can
   actually execute, or re-scope them explicitly as the ONNX/custom-backend
   toolkit so FEATURE_INDEX stops implying they accelerate GGUF. Route via
   `/qor-organize`-adjacent decision — do not restructure from research.

## Updated Knowledge

- Shadow Genome Entry #5 added: "exists+tested ≠ wired" — production-path
  call-site verification is mandatory before citing a module as active
  (perf kernels and security chain both failed this check; pairs with
  BACKLOG B-14 / SG-035 deep-verify obligation).
- Corrected ledger-#96 knowledge: paged-KV/flash-attn/Q8-KV/SIMD are
  bench-only in default+gguf builds; llama.cpp internals own the hot path.
- mistral.rs classified BLOCKED for linking (forbidden deps, candle-git
  pin); recorded as pattern-donor for speculative decoding, scheduler
  bucketing, prefix caching, ISQ.

---

_Research complete. Findings are advisory — implementation decisions remain
with the Governor._
