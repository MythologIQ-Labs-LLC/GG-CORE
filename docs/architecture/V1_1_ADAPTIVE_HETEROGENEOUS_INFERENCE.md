# Adaptive heterogeneous inference (v1.1.0 proposal)

**Status:** Proposed architecture, requested by the owner on 2026-11-27 for consideration in v1.1.0. Documentation only; no implementation, qualification, or release approval.

**Scope lock:** v1.0.0 remains the immediate priority. This incremental enhancement must not change its approved completeness criteria, delay its qualification, or defer any already-required CPU, GPU, switching, security or standalone capability. Do not begin this enhancement on the v1 critical path. Reassess the design against the actual released v1 baseline before implementation.

**Reviewed baseline:** main at `f53bb329237ecbd4789227001135cfb62388af70`; open GPU offload PR [#224](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/224) and cold-switch PR [#211](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/211) are not assumed merged or qualified.

## Why

Useful local inference should not require purchasing a GPU large enough for every model. A computer often has substantial system RAM and CPU capacity alongside a much smaller, faster GPU memory pool. Treat both as first-class compute resources. Optimize the total cost of a useful answer, subject to quality, latency, resource, fairness and security constraints, not peak accelerator utilization or model size alone.

## What

Extend the existing adaptive model-selection policy to select a qualified (model, execution plan) pair. Do not introduce a competing scheduler or let backend selection independently change a model pin.

| Execution plan | Purpose |
| --- | --- |
| CPU-only | Intentional selection for memory capacity, suitable latency, or accelerator-independent work; not merely error fallback. |
| GPU-only | Low latency or throughput when the complete working set fits within the device budget. |
| Hybrid layer placement | One model spans CPU and GPU via qualified native backend placement. |
| Concurrent CPU/GPU workloads | Independently admitted requests use different devices while respecting shared host resources. |
| Hybrid tensor/expert placement | Research extension: keep attention and frequently needed weights on the accelerator while executing eligible expert operations on CPU or caching selected experts in VRAM. |

These are distinct plans, not interchangeable claims. Hybrid placement does not imply simultaneous execution. Concurrent workloads do not imply shared model weights or pooled RAM/VRAM. A device-detected backend is not a qualified serving path.

## Existing foundations and gaps

- [Backend capability contract](../integration/V1_BACKEND_CAPABILITY_CONTRACT.md) and [model selection policy](../../core-runtime/src/models/model_selection_policy.rs): advisory, quality-floor-first selection and local configured resource headroom; not an execution authorization.
- [Backend dispatch](../../core-runtime/src/models/backend_dispatch.rs) and [GGUF loader](../../core-runtime/src/engine/gguf/backend.rs): manifest-driven dispatch and a native GPU-layer configuration seam. The production dispatch at the reviewed baseline uses the default GPU-layer setting; #224 is separate active work.
- [Working memory estimator](../../core-runtime/src/engine/gguf/working_memory.rs): conservative FP16 KV and compute-buffer accounting; MoE and recurrent/hybrid architecture shapes are explicitly unsupported. Retain fail-closed admission until architecture-specific bounds are validated.
- Existing scheduler, model lifecycle, cancellation, secure inference façade and local IPC remain the only execution and authority boundaries. Library-only MoE or GPU modules must not be promoted by documentation.

## How

### Decision and ownership

1. Enumerate locally installed, authorized, exact-artifact-qualified model/execution-plan candidates. Preserve model/backend pins, task requirements, quality floor, offline rules, and host consent for transitions. Reject unknown or contradictory evidence.
2. Estimate feasible resident weights, KV state, temporary buffers, transfer staging, host commit, device allocation, CPU threads and transition peaks separately. Include OS/display headroom and other admitted workloads. A point-in-time profile is not an allocation reservation.
3. Rank feasible candidates using the existing selection objective. Compare measured end-to-end latency, quality, throughput, energy where available, residency and switching cost. No invented thresholds or p95 arithmetic presented as measured end-to-end p95. Deterministic tie/refusal behavior and bounded hysteresis prevent oscillation.
4. Return an advisory plan and rationale. The canonical scheduler/lifecycle owns atomic multi-resource reservation, revalidation and execution. A stale or infeasible plan is refused or replanned only within the same owner-granted policy.
5. Reassess at request boundaries by default. Never migrate live KV state or release allocations still owned by a native worker. Mid-request changes require separate correctness, cancellation, state-transfer and lifetime qualification.

Proposed internal plan identity: model hash, backend/build identity, device identities, placement policy, quantization, context/batch limits, execution phase configuration, resource bounds and evidence ID. Use additive internal types first; no breaking public Rust, C, Python or IPC V1 shape changes. Any consumer-visible negotiation needs separate review under [#112](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/112).

### Resource management and adaptation

Track host RAM/commit and each device VRAM separately; charge pinned buffers, duplicate copies, GPU staging and concurrent reservations to their actual domains. Never add the two capacities and call the result unified memory. Reserve transition peaks before loading; release only after worker quiescence and observed reclamation. Enforce CPU thread budgets and memory-bandwidth contention as well as memory size.

Maintain local performance profiles keyed by exact model, quantization, backend/binary, driver, hardware, placement, context/batch range and contention conditions. Record provenance, sample count, age and invalidation rules. Cold-start or missing evidence allows only explicitly authorized qualified baseline plans, never speculative escalation. Calibration is opt-in, bounded and offline; no prompt/content logging or automatic model downloads.

### Research directions (not dependencies of the first increment)

- Reuse llama.cpp native layer/tensor placement APIs before creating another execution engine. Examine upstream [llama.cpp](https://github.com/ggml-org/llama.cpp) and [KTransformers](https://github.com/kvcache-ai/ktransformers) for CPU expert kernels, tensor placement and transfers. Pin and verify source versions and binding availability in the research work; these links do not establish supported functionality.
- Compare static versus adaptive expert placement, sparse CPU execution, bounded expert caching and prefetch, and independent CPU/GPU work overlap. Avoid PCIe weight thrashing; measure bytes transferred and synchronization cost.
- Tune prefill and decode separately (thread counts, microbatching, cache placement, supported quantized KV). Consider speculative decoding only with verified model/tokenizer compatibility and real wall-clock benefit. Do not rebuild existing ADR-007 work.
- Assess low-bit CPU kernels and BitNet via existing [#51](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/51), not a new mandatory backend. Preserve equivalent treatment of GGUF, ONNX and Hologram under [#196](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/196).

## Security and failure boundaries

All paths retain authenticated local IPC and secure ingress/egress; no raw token or text bypass, external routing, network listener, cloud fallback, or new tool/data authority. Reuse model integrity and path confinement. Qualify native driver interactions against actual platform containment; do not claim universal sandbox isolation. On cancellation, timeout, OOM, device loss or load failure, drain and release exactly once after native exit. If safe recovery cannot be demonstrated, fail closed. Do not weaken limits or automatically downshift below a quality floor.

## Evidence and completion criteria

Use existing benchmark/receipt governance [#136](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/136), [#156](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/156) and [#196](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/196). Initial reference class: consumer x86-64 AVX-512 desktop, 64 GiB system RAM, 16 GiB CUDA GPU. No hardware purchase required; actual available memory, driver and sustained bandwidth must be measured.

Compare matched CPU-only, GPU-only where feasible, static layer offload, adaptive placement and concurrent execution. Start with a small dense fixture, then a larger dense quantized model exceeding VRAM, then a sparse MoE candidate only after safe memory admission is implemented. Record unsupported configurations rather than forcing them. Pin model hashes, sampling, context, workload, OS, drivers, binary and security configuration. Measure prompt/decode throughput, time to first native token and first sanitized output separately, end-to-end latency distributions, quality, host/device peaks, transfer costs, cancellation tail, power/energy where instrumented and reclamation. Repeat under idle and competing workloads; compare with native upstream baseline and v1 regression baseline.

Acceptance requires:

- Real packaged/installed inference on each claimed platform and execution plan, not mocks, with reproducible useful-answer quality and a measured benefit for at least one relevant workload; numerical targets require owner disposition after baseline collection.
- CPU-only remains an intentionally selectable, qualified path even with GPU present; hybrid execution demonstrably uses both domains; concurrent execution proves overlap and fairness.
- Negative tests cover stale profiles, denied pins, absent driver, CPU/GPU exhaustion, simultaneous admission races, unsupported model shape, cancellation, worker lifetime, rollback and no unauthorized fallback.
- Existing v1 behavior, security, compatibility and performance remain within agreed regression gates; new claims are exact-artifact-bound and independently reviewed.

## Proposed v1.1 sequencing

1. **After v1.0.0 qualification/release:** reconcile this note with the shipped baseline and approved capability inventory. Reuse [#149](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/149), [#50](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/50), [#154](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/154), and [#289](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/289) as historical and research inputs; no duplicate active implementation epic.
2. Qualify explicit CPU/GPU/static hybrid plan representation, dual-domain accounting and measurements; audit L2/L3 changes before code.
3. Integrate evidence-driven plan selection and independent workload concurrency through existing lifecycle and scheduler.
4. Prototype expert placement, phase-aware tuning and adaptive caches only where baseline data warrants complexity. Promote each independently or retain as research with explicit disposition.
5. Qualify the v1.1 candidate against owner-approved scope, including downstream compatibility. No automatic CoreForge pin update.

Open decisions: platform coverage, exact model corpus, latency/quality/energy targets, profile persistence policy, expert-placement strategy, and v1.1 scope commitment. GG-CORE owns local execution; CoreForge owns user intent and external orchestration. This proposal changes neither.
