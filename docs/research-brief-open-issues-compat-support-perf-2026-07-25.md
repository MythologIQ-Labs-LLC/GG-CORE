# Research Brief

**Date**: 2026-07-25T12:24:23-04:00
**Analyst**: The Qor-logic Analyst
**Target**: Open GitHub issues + pending architectural intent for GG-CORE
**Scope**: Three independent lenses — (1) open compatibility, (2) wide range of
support, (3) performance optimization — investigated independently, then
cross-impact analyzed.

---

## Executive Summary

Of 12 open issues, 4 (#55, #56, #57, #69) are already fixed on `origin/main`
via merged PR #71 with green 3-OS Rust CI — they are stale-open and need only
operator verification + closure. The live architectural intent is the
backend-capability epic (#48–#53): codebase verification confirms its core
premise (no `RuntimeBackendCapabilities`, no capability-driven selection, no
degraded-mode policy), but issue #72's premise has partially drifted — the ONNX
embedder now performs real candle-onnx loading; only the classifier remains a
stub. ADR-007 speculative decoding is complete at the plan/heuristic/telemetry
level but is **not wired into the engine decode path**, and issue #52's
benchmark harness is the shared evidence gate that both the capability epic and
ADR-007 wiring depend on.

---

## Findings

### A. Issue-state pre-check (Step 2.5 — surfaced to operator)

**PR #71 — MERGED 2026-07-08T19:41:53Z** — "feat(adr007): ADR-007 TierSynergy
Adaptive Speculative Decoding epic + Section 4 Razor fix" merged the entire
`chore/hardening-cycle2-validate-clippy` branch into `origin/main` (merge
commit `11bf0ac`). Ancestor-check against `origin/main` confirms the fix
commits shipped:

| Issue | Fix commit | On origin/main | CI evidence | Status |
|-------|-----------|----------------|-------------|--------|
| #55 test failures (default features) | `43cc89c`, `461abaa` | YES | Rust CI green on main (run 28982209959, 3-OS) | Stale-open — operator close candidate |
| #56 13 residual clippy errors | `43cc89c`, `3e85763` | YES | clippy `-D warnings` green, 3 OSes | Stale-open — operator close candidate |
| #57 validate_path NUL bytes | `43cc89c` | YES | security_path_traversal_test in green `cargo test --workspace` | Stale-open — operator close candidate |
| #69 FinishReason::Cancelled match | `626f034` | YES | clippy `--all-targets` green | Stale-open — operator close candidate |

Caution observed during research: **local `main` (`354d41d`) is stale/diverged
from `origin/main` (`11bf0ac`)** — matches BACKLOG B-09. Ancestor checks
against local main gave the opposite (wrong) answer; all claims above were
re-verified against `origin/main` after `git fetch`.

Remaining genuinely-open issues: #48, #49, #50, #51, #52, #53 (backend
capability epic), #70 (Hologram research), #72 (ONNX). Open PRs: #59 (ADR-007
docs — companion to already-merged epic code), #47 (cfg-gate advanced tests —
BACKLOG B-08, "merge first" per 2026-07-08 research).

Minor: the only failing runs on `main` are Dependabot update jobs (pyo3, rand)
— dependency-bump PRs failing, not project CI.

### B. Lens 1 — Open compatibility (backend capability epic #48–#53, #72)

1. **ONNX backend (issue #72) — premise partially DRIFTED.**
   `load_onnx_model()` at `core-runtime/src/engine/onnx/mod.rs:71-80` now calls
   `candle_onnx::read_file(path)` and returns a real `OnnxEmbedder` — commit
   `b048869` ("implement Candle ONNX inference for OnnxEmbedder") is on this
   branch and on origin/main. `OnnxEmbedder` holds
   `Option<candle_onnx::onnx::ModelProto>` (`engine/onnx/embedder.rs:13-20`),
   populated via `with_model()` (embedder.rs:36-47). **However**
   `OnnxClassifier` still holds `_model: Option<()>` placeholder
   (`engine/onnx/classifier.rs:13-20`) and `classify_text()` always errors
   (classifier.rs:35-42). Issue #72's scope-1 acceptance is therefore ~half
   done: embedder path real, classifier path stub.
2. **No capability contract exists (issues #48/#49 premise CONFIRMED).** The
   only capability structures are the parallel enums `InferenceCapability`
   (`engine/mod.rs:158-165`) and `ModelCapability` (`models/manifest.rs:31-39`)
   plus per-model `capabilities()` on the `OnnxModel`/`GgufModel` traits
   (`engine/onnx/mod.rs:42-55`, `engine/gguf/mod.rs:46-77`). Nothing resembles
   `RuntimeBackendCapabilities` (streaming/embeddings/tool-calling/KV-cache
   flags, perf estimates, governance hooks) and no IPC exposure of backend
   capabilities exists.
3. **Backend dispatch is format-based and decentralized.** The manifest's
   `ModelArchitecture` enum (Gguf/Onnx/SafeTensors,
   `models/manifest.rs:43-50`) drives dispatch, but the decision is made at
   call sites — callers pick `load_gguf_model()` (`engine/gguf/mod.rs:85-98`)
   or `load_onnx_model()`; `ModelRouter.resolve()` is a stateless id→handle
   map (`models/router.rs:34-36`); `SmartLoader` selects by tier, not
   capability (`models/smart_loader.rs:104-111`). No central capability
   resolver exists — exactly the gap #48's mermaid architecture proposes.
4. **Hardware profile (issue #50) — partial precursor exists.**
   `k8s/profiles.rs:8-15` defines `DeploymentProfile`
   (CpuOnly/SingleGpu/MultiGpu/HighMemory) with K8s resource specs
   (profiles.rs:111-188). Missing vs. #50's schema: CPU arch/feature matrix
   (no AVX detection), thread planning (GGUF `n_threads` hardcoded 0=auto,
   `engine/gguf/mod.rs:28`), pre-declared VRAM budgets (GPU memory only
   tracked at runtime, `engine/gpu_allocator.rs`), `preferredBackends`, and
   `degradePolicy`. A second, distinct `HardwareProfile` enum already exists
   in the speculative layer (`models/tier_synergy_speculative.rs:101-155`) —
   #50 work should unify these two rather than add a third.
5. **BitNet (issue #51) — pure greenfield.** Zero matches for "bitnet"
   anywhere in the repo, no `src/backends/` module. Adapter would be new code.
6. **Degraded mode (issue #53) — no policy layer.** What exists: health
   tri-state (`health.rs:14-18`, reported at health.rs:104-120), canary
   regression detection (`deployment/metrics.rs:86-90`), and
   speculative→single-model fallback (`tier_synergy_speculative.rs:101-155`).
   None of these performs backend-level degradation (context reduction,
   capability shedding, fail-closed-with-explanation). #53's premise holds.

### C. Lens 2 — Wide range of support (platforms, bindings, consumers)

1. **Sandbox is real on both platforms.** Windows: Job Objects
   (`sandbox/windows.rs:102-171`, memory/CPU limits via `windows_sys`).
   Linux: cgroups v2 writes (`sandbox/unix.rs:159-216`) + full seccomp-bpf
   x86_64 whitelist (unix.rs:234-393) with GPU-syscall allowances
   (unix.rs:220-230). Platform fallback is `NoopSandbox`
   (`sandbox/mod.rs:82-110`). F-38 remains **unverified** in
   FEATURE_INDEX, but the flip condition — green Linux/macOS clippy/test CI —
   is now satisfied (Rust run 28982209959 on main, 3 OSes). Evidence exists;
   the index flip is pending.
2. **CI matrix is 3-OS but default-features-only.**
   `.github/workflows/rust.yml:16-56`: lint job (fmt + clippy `-D warnings`)
   and test job (`cargo test --workspace`) on ubuntu/macos/windows — but with
   `default = []` (`Cargo.toml:125`), **no inference backend, binding, or GPU
   feature is ever compiled in CI**: gguf, onnx, cuda, metal, python, ffi all
   untested. The working ONNX embedder (B.1) and any #72 completion ship
   CI-unverified today.
3. **FFI surface is real**: 7 `#[no_mangle]` modules (`ffi/mod.rs:1-25`),
   lifecycle (`ffi/runtime.rs:25-100`), streaming (`ffi/streaming.rs:66-162`),
   cbindgen header generated at `include/gg_core.h` (build.rs:8-49).
   **Python bindings are complete scaffolds but F-40 unverified** — PyO3
   module exposes Runtime/Session/streaming (`python/mod.rs:1-27`, gated
   lib.rs:58); `tests/python_binding_test.rs` exists but never runs in CI.
   **Veritas shim F-45 unverified** (no standalone test binding).
4. **GPU breadth**: CUDA real behind `cuda` feature (`engine/cuda.rs:53-92`;
   stub fallback cuda.rs:466-482), Metal real behind `metal`+macOS
   (`engine/metal.rs:43-79`; stub metal.rs:483-505), CPU always available
   (`engine/gpu.rs:16-35`). CPU-only is the only CI-exercised path.
5. **Consumer contract**: COREFORGE builds gg-core with
   `features = ["gguf", "onnx"]` (issue #72 body, COREFORGE
   `src-tauri/Cargo.toml`) and routes only to `load_gguf_model` today; its
   llmfit setup wizard filters recommendations to GGUF-only until GG-CORE can
   deliver ONNX per capability. `docs/COREFORGE Integration Notes.txt:67-91`
   carries the outstanding builder obligations (real generation path,
   non-stub tokenizer, streaming completion, honest metrics).
6. **No MSRV pinned** — no `rust-version` in Cargo.toml, no
   rust-toolchain.toml; CI floats on `dtolnay/rust-toolchain@stable`
   (rust.yml:28). Downstream consumers (COREFORGE, Python, C FFI) have no
   declared toolchain floor.

### D. Lens 3 — Performance optimization (#52, ADR-007, existing surface)

1. **Bench inventory (11 Criterion targets, Cargo.toml `[[bench]]`)**:
   ipc_throughput, scheduler_throughput, inference_latency,
   generation_throughput, memory_overhead, concurrent_load, gpu_allocation,
   multi_gpu_scaling, kv_cache_throughput, speculative_matrix,
   llama_cpp_comparison. Against issue #52's 15-metric harness:
   **covered** — ipc_overhead_ms (`benches/ipc_throughput.rs:41-161`),
   speculative control-path overhead (`benches/speculative_matrix.rs:32-181`,
   5 scenario groups, all sub-microsecond, feature-gated on `advanced`);
   **missing** — cold_start_ms, warm_start_ms, end-to-end first-token latency,
   real tokens_per_second, policy_intercept_overhead_ms,
   audit_write_latency_ms, JSON result export, and the raw-backend-vs-wrapped
   comparison mode (no backend abstraction to compare through).
   `benches/llama_cpp_comparison.rs:1-211` is the closest precursor to
   raw-vs-wrapped.
2. **ADR-007 speculative decoding is plan-complete, execution-unwired.**
   Complete: `AdaptiveSpeculativeConfig` (`models/speculative_config.rs:1-177`,
   off by default), `TierSpeculativePlan::select()` with hardware gating +
   acceptance-floor fallback (`models/tier_synergy_speculative.rs:101-155`),
   trait suite BlockDraftModel/ConfidenceEstimator/VerificationScheduler/
   TargetVerifier (`engine/adaptive_speculative/mod.rs:158-215`), heuristic
   estimator + auto-disable scheduler
   (`engine/adaptive_speculative/heuristic/mod.rs:50-234`), telemetry
   accumulator (`engine/adaptive_speculative/telemetry.rs:116-214`).
   **Missing: no production `impl BlockDraftModel`/`impl TargetVerifier`
   exists (mocks only, in tests); `engine/decode.rs` imports the config but
   has no draft/verify execution path.** Claimed 1.3–2.0× speedup remains
   unmeasured — consistent with docs/BENCHMARKS.md:214-265 framing estimates
   as control-path only.
3. **Real optimization inventory (all non-stub)**: AVX2 SIMD tokenizer
   (`engine/simd_tokenizer.rs:1-24`, plus simd_neon.rs, simd_matmul.rs),
   vLLM-style paged KV (`memory/paged.rs:1-50`, 16 tokens/page), Q8 KV
   quantization (`memory/kv_quant.rs:9-57`), tiled flash attention
   (`engine/flash_attn.rs:44-50`, GPU variant flash_attn_gpu.rs), memory pool
   (benched in `benches/memory_overhead.rs:9-52`), scheduler batching
   (`scheduler/batch.rs:23-60`).
4. **Telemetry partially covers #52 needs**: histograms
   `core_inference_latency_ms` (`telemetry/metrics.rs:18-21`),
   `core_tokenization_latency_ms` (metrics.rs:22-25),
   `core_model_switch_latency_seconds` (metrics.rs:55-58), speculative
   counters (metrics.rs:41-46), Prometheus text export
   (`telemetry/prometheus.rs:71-94`). **Absent**: cold/warm start, governance
   intercept, and audit-write latency metrics — #52 needs new instrumentation,
   not just a harness.

---

## Blueprint Alignment

| Blueprint/Issue Claim | Actual Finding | Status |
|----------------------|---------------|--------|
| ARCHITECTURE_PLAN: `onnx/` = "candle: embedder, classifier" | Embedder real (candle-onnx load, onnx/mod.rs:71-80); classifier stub (classifier.rs:35-42) | DRIFT (partial) |
| Issue #72: "load_onnx_model() returns Err(...) even with onnx feature" | Superseded by `b048869` on origin/main — embedder loads for real | DRIFT (issue text stale) |
| Issues #48/#49: no backend capability contract exists | Confirmed — enums only, no schema, no IPC exposure | MATCH |
| Issue #50: hardware-aware selection missing | Confirmed, but two partial precursors exist (k8s/profiles.rs:8-15; tier_synergy_speculative HardwareProfile) that must be unified | MATCH (nuance) |
| Issue #53: no degraded-mode policy | Confirmed — health/canary/speculative fallbacks exist but no backend degradation ladder | MATCH |
| ARCHITECTURE_PLAN:199 "sandbox unverified on Linux/macOS pending #54" | Fix merged (PR #71); Rust CI green on 3 OSes → flip evidence exists, F-38 still marked unverified | DRIFT (doc lags reality) |
| BACKLOG B-17/B-18/B-19/B-22 marked open (issues #55/#56/#57/#69) | All fixed on origin/main with green CI | DRIFT (pointer layer stale) |
| CLAUDE.md cites `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` | File does not exist (known: BACKLOG B-13) | DRIFT (pre-existing) |
| ADR-007 commits imply speculative decoding delivered | Plan/heuristics/telemetry delivered; decode-path wiring absent | MATCH (scoped — ADR staged it this way) but flag for expectation management |

---

## Cross-Impact Analysis (lenses considered together)

The three lenses were investigated independently; their interactions define
the sequencing:

1. **#49 (capability schema) is the keystone of all three lenses.**
   Compatibility: #50 selection, #53 degrade policy, and #51 BitNet all
   consume it. Support: #72's downstream payoff — COREFORGE lifting its
   GGUF-only wizard filter — is specified "per capability", i.e. it needs the
   schema exposed over IPC, not just a working loader. Performance: #52's
   raw-vs-wrapped mode needs a backend abstraction boundary to instrument.
   Sequence: #48 ADR → #49 → {#50, #53, #72-integration} → #51.
2. **#52 (benchmark harness) is the shared evidence gate.** #51's acceptance
   criteria forbid BitNet default-status without benchmark evidence; ADR-007
   decode-wiring needs net_speedup proof before enabling; #49/#50 add a
   capability-resolver hop to the hot path whose overhead should be baselined
   *before* it lands (extend `ipc_throughput`/`llama_cpp_comparison` first).
   Building #52 early de-risks every other item.
3. **CI feature-coverage gap undercuts both compatibility and support work.**
   Any ONNX (#72), capability (#49), or BitNet (#51) code ships CI-unverified
   because the matrix builds default features only. A `--features full`
   (gguf+onnx) CI leg — and a `--features python` leg to flip F-40 — is a
   prerequisite-quality investment for the whole epic. This is the one new
   P1-shaped gap this research surfaces.
4. **Performance vs. compatibility tension.** Capability-driven dispatch
   (#49/#50) inserts decision logic where dispatch is currently a direct
   function call; GG-CORE's own thesis (issue #52 body) demands governance/
   wrapper overhead be measured separately. Mitigation: capability resolution
   at model-load/registration time (cold path), never per-token.
5. **#72 scope decision couples to #49.** Whether ONNX text-generation is in
   scope should be expressed as a capability declaration
   (`supportsChatCompletion: false` for the ONNX backend initially) rather
   than a new hard-coded path — otherwise #72 scope-2 recreates the implicit
   dispatch the epic is eliminating.
6. **#70 (Hologram) stays research-only** and touches the other lenses at
   two points: content-addressed model identity could serve #49's
   `modelFormats`/artifact-integrity story, and its no-classical-KV-cache
   posture conflicts with the ADR-007 verification path. Its own issue text
   already gates any action behind an evidence threshold — no sequencing
   impact now.
7. **Stale-state hygiene (lens-independent).** Closing #55/#56/#57/#69,
   updating BACKLOG rows B-17/B-18/B-19/B-22/B-23, flipping F-38, and
   refreshing ARCHITECTURE_PLAN:199 are cheap and remove false "red" signals
   that would otherwise distort future planning (this session's own research
   nearly mis-reported them from stale local main).

---

## Recommendations

1. **P1 — Operator: verify + close stale issues** #55, #56, #57, #69 citing
   PR #71 / merge `11bf0ac` and green Rust run 28982209959; update BACKLOG
   rows B-17, B-18, B-19, B-22 (and B-23 seal status); flip FEATURE_INDEX
   F-38 with the green 3-OS CI run as evidence; refresh ARCHITECTURE_PLAN:199.
   (Closing issues is reserved for Kevin per the lane contract.)
2. **P1 — Add CI feature legs** before new backend work: one job with
   `--features full` (linux+windows at minimum) and one with
   `--features python` (flips F-40). Keep cuda/metal out (no GPU runners).
3. **P2 — Author ADR #48 now** (it is a docs deliverable) and scope #49 as
   the next governed implementation cycle (L2 — new business logic, API
   surface). Fold the "unify the two HardwareProfile precursors" finding into
   #50's plan.
4. **P2 — Re-scope issue #72** with a comment: embedder half done
   (`b048869`), remaining = classifier implementation + registry/IPC wiring +
   capability declaration; couple its COREFORGE filter-lift to #49's IPC
   capability exposure.
5. **P2 — Build #52 harness before wiring ADR-007 into decode**: extend
   `llama_cpp_comparison.rs` into the raw-vs-wrapped mode, add cold/warm
   start + governance/audit-latency instrumentation to `telemetry/`, JSON
   export per #52's schema. It gates both #51 and speculative enablement.
6. **P3 — Pin an MSRV** (`rust-version` in Cargo.toml) — cheap insurance for
   the three consumer surfaces (COREFORGE, C FFI, Python).
7. **P3 — #70 Hologram**: keep parked; when picked up, deliver only the
   fit/non-fit research note its acceptance criteria describe.

## Updated Knowledge

- Shadow Genome Entry #4 added: stale-local-main issue-state drift —
  always ancestor-check against `origin/main` after `git fetch` during
  Step 2.5 pre-checks (this session's near-miss).
- Corrected: ONNX embedder is functional on origin/main (`b048869`);
  issue #72 text predates it. Classifier remains the stub.
- Corrected: ADR-007 speculative decoding is interface/plan-complete but not
  execution-wired; treat performance claims as unmeasured until #52 harness
  exists.

---

_Research complete. Findings are advisory — implementation decisions remain
with the Governor._
