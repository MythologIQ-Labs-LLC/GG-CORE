# ADR-007: TierSynergy Adaptive Speculative Decoding

**Status:** Proposed  
**Date:** 2026-07-08  
**Decision Makers:** MythologIQ Labs / GG-CORE maintainers  
**Consulted:** Runtime, Security, Performance, Governance

---

## Context

GG-CORE is a secure, Rust-native inference runtime built around Contained, Offline, Restricted, Execution. The runtime is intentionally scoped as a compute boundary. It performs model execution only, has no network authority, and does not operate as an agent, orchestrator, policy engine, connector layer, plugin host, or memory store.

GG-CORE already contains the core shape needed for tier-aware speculative inference:

1. `core-runtime/src/models/tier_synergy.rs` defines `TierSynergy`, `SynergyMode`, Light/Balanced/Quality model tiers, and speculative pairings such as Light as draft with Quality as target.
2. `core-runtime/src/engine/mod.rs` exposes advanced modules for speculative decoding, speculative v2, GPU kernels, quantization, SIMD, and multi-GPU execution behind feature gates.
3. `core-runtime/src/engine/gguf/speculative.rs` provides GGUF draft and target wrappers.
4. Existing benchmark documentation establishes a verified CPU baseline and performance matrix, which provides a practical starting point for measuring local inference gains.

A separate `GG-CORE-TierSynergy` repository also exists, but the functional TierSynergy concept is already present inside GG-CORE. The separate repository should be treated as historical, commercial packaging, or a source of design material until a formal consolidation decision is made.

Recent DSpark research introduces confidence-scheduled speculative decoding. The useful idea is not to copy DeepSeek's implementation. The useful idea is to generate draft token blocks, estimate the survival probability of the drafted prefix, choose a verification window dynamically, and avoid wasting verification compute on low-confidence draft tails.

This maps naturally onto GG-CORE's existing TierSynergy design. Light models can serve as draft models. Balanced models can serve either draft or target roles. Quality models can serve as verification targets. TierSynergy can become the resource-aware control layer that decides when speculation is helpful, how aggressive verification should be, and when to fall back to single-model decoding.

---

## Decision

GG-CORE will fully incorporate TierSynergy as a first-class runtime scheduling layer and extend it with DSpark-inspired adaptive speculative decoding.

The implementation will treat TierSynergy as the primary integration point for:

1. Model tier selection.
2. Draft and target model pairing.
3. Speculative decoding mode selection.
4. Verification budget scheduling.
5. Runtime degradation and fallback.
6. Telemetry for acceptance, throughput, latency, and resource pressure.

The DSpark-inspired behavior will be implemented as an adaptive speculative decoding strategy, not as a direct dependency on DeepSeek, DSpark, Python serving infrastructure, or external model-serving systems.

The runtime must preserve GG-CORE's security boundary. The speculative path must remain local, offline, IPC-bound, auditable, and safely reversible.

---

## Goals

1. Make TierSynergy the canonical GG-CORE layer for tiered inference coordination.
2. Add adaptive speculative decoding that scales from low-end CPU inference to high-end GPU and multi-tenant deployments.
3. Improve local inference throughput where the draft cost is lower than target generation cost.
4. Preserve output correctness by requiring target-model verification.
5. Preserve GG-CORE's security model and pure compute boundary.
6. Add runtime telemetry that proves whether speculation is beneficial in a given environment.
7. Support graceful degradation to ordinary single-model decoding when speculation is not beneficial.

---

## Non-Goals

1. Do not turn GG-CORE into a network model server.
2. Do not import SGLang, DeepSpec, DSpark Python code, or DeepSeek serving infrastructure.
3. Do not add autonomous agent behavior.
4. Do not move governance authority into GG-CORE.
5. Do not persist prompt text, output text, PII, or sensitive inference content as part of speculation telemetry.
6. Do not claim DSpark production speedups for local CPU inference without direct GG-CORE benchmarks.
7. Do not require GPU hardware for the first implementation.

---

## Existing TierSynergy Baseline

The current TierSynergy implementation already contains the correct conceptual spine:

```rust
pub enum SynergyMode {
    Single,
    SpeculativeLightQuality,
    SpeculativeLightBalanced,
    SpeculativeBalancedQuality,
}
```

The current implementation also maps task hints to model tiers, detects available tier pairings, and falls back to single-model mode when the needed tier combination is not available.

This ADR does not replace that logic. It formalizes it, extends it, and moves it from a useful local feature into the runtime's canonical adaptive inference scheduling model.

---

## Proposed Architecture

```text
Authenticated IPC
  -> request validation
  -> prompt injection filtering
  -> PII detection and redaction
  -> tier and model selection
  -> prefill
  -> TierSynergy adaptive speculative scheduler
       -> choose runtime mode
       -> choose draft model
       -> choose target model
       -> generate draft block
       -> estimate prefix survival
       -> select verification window
       -> verify with target model
       -> commit accepted prefix
       -> discard or resample rejected suffix
       -> update aggregate telemetry
  -> output sanitization
  -> audit logging
  -> metrics emission
```

TierSynergy will own the decision of whether the request should run in single-model mode or speculative mode.

The speculative decoder will own the low-level mechanics of draft generation, confidence estimation, target verification, accepted-prefix commit, and fallback.

The engine module will own the model execution primitives.

The security module will continue to own input filtering, PII handling, output sanitization, authentication, and audit behavior.

---

## Core Interfaces

The first implementation should introduce interfaces equivalent to the following.

```rust
pub enum AdaptiveSpeculativeMode {
    Disabled,
    Conservative,
    Adaptive,
    Aggressive,
    TierAware,
}

pub struct AdaptiveSpeculativeConfig {
    pub enabled: bool,
    pub mode: AdaptiveSpeculativeMode,
    pub max_draft_tokens: usize,
    pub min_verify_tokens: usize,
    pub max_verify_tokens: usize,
    pub confidence_floor: f32,
    pub acceptance_floor: f32,
    pub auto_disable_on_low_acceptance: bool,
    pub collect_stats: bool,
    pub use_runtime_cost_profile: bool,
    pub tier_aware: bool,
}
```

```rust
pub trait BlockDraftModel {
    fn draft_block(
        &self,
        context: &[u32],
        max_draft_tokens: usize,
        config: &AdaptiveSpeculativeConfig,
    ) -> Result<DraftBlock, InferenceError>;
}

pub trait ConfidenceEstimator {
    fn estimate_survival(
        &self,
        context: &[u32],
        draft: &[u32],
        profile: &RuntimeCostProfile,
    ) -> Result<SurvivalProfile, InferenceError>;
}

pub trait VerificationScheduler {
    fn choose_verification_window(
        &self,
        survival: &SurvivalProfile,
        tier: ModelTier,
        load: &SystemLoadSnapshot,
    ) -> VerificationPlan;
}

pub trait TargetVerifier {
    fn verify_draft(
        &self,
        context: &[u32],
        draft: &[u32],
        plan: &VerificationPlan,
    ) -> Result<VerificationResult, InferenceError>;
}
```

The first version may use heuristic confidence estimation. A later version may add learned confidence heads or model-family-specific drafters.

---

## Hardware and Model Scope

### Tier 0: Ultra-Low-End CPU

Target environment:

- 2 to 4 CPU cores.
- 8 GB RAM.
- No GPU.
- 0.5B to 1.5B Q4 models.

Behavior:

- Speculation disabled by default or enabled only in conservative mode.
- Draft length: 1 to 2 tokens.
- Verification window: 1 to 2 tokens.
- Auto-disable when acceptance falls below floor.
- Auto-disable when draft overhead exceeds savings.

Expected result:

- Low to modest gain.
- Useful mainly for predictable completions and structured output.
- Diminishing returns are expected because target generation is already cheap.

### Tier 1: Low-End Local CPU

Target environment:

- 4 to 8 CPU cores.
- 16 GB RAM.
- No GPU.
- 1.5B to 3B Q4 models.

Behavior:

- Conservative adaptive speculation available.
- Draft length: 2 to 4 tokens.
- Verification window: 2 to 4 tokens.
- Heuristic confidence scheduling.
- Aggregate acceptance tracking by model pair.

Expected result:

- Modest to meaningful gain.
- Strongest for low-temperature text, structured responses, boilerplate, and code continuation.

### Tier 2: Mainstream Local CPU

Target environment:

- 8 to 16 CPU cores.
- 32 GB RAM.
- 3B to 8B Q4 models.

Behavior:

- Adaptive speculation available.
- Draft length: 4 to 8 tokens.
- Verification window: 2 to 8 tokens.
- Same-family draft-target pairings supported where tokenization is compatible.
- Runtime cost profile used to avoid negative speedups.

Expected result:

- Meaningful gain where target-model generation is expensive enough to amortize draft cost.

### Tier 3: Consumer Local GPU

Target environment:

- 8 GB to 16 GB VRAM.
- 32 GB to 64 GB system RAM.
- 7B to 14B models.

Behavior:

- CPU-draft with GPU-target supported where beneficial.
- GPU-draft with GPU-target supported where memory allows.
- Draft length: 4 to 12 tokens.
- Verification window: 2 to 12 tokens.
- Scheduler must consider GPU memory pressure and KV cache pressure.

Expected result:

- Meaningful to high gain when draft model is cheap and target model is large enough.

### Tier 4: High-End Single GPU

Target environment:

- 24 GB to 48 GB VRAM.
- 64 GB or more system RAM.
- 14B to 34B models.

Behavior:

- Aggressive adaptive speculation available.
- Draft length: 8 to 16 tokens.
- Verification window: 4 to 16 tokens.
- Runtime cost profiles should influence scheduling.
- Learned confidence heads may be evaluated.

Expected result:

- High gain when model pair compatibility is strong.

### Tier 5: Multi-GPU or Server-Class Deployment

Target environment:

- Multi-GPU host.
- 34B to 70B+ models.
- Concurrent users or batch inference.

Behavior:

- Tier-aware speculation available.
- Draft length: 8 to 32 tokens.
- Verification window: 4 to 24 tokens.
- Service tier and system load may influence verification budgets.
- Scheduler should avoid wasting high-value batch capacity on low-confidence draft tails.

Expected result:

- Highest gain potential.
- Closest match to the DSpark production serving environment.

---

## TierSynergy Runtime Modes

### Single

Use one model only. No speculative decoding.

### Conservative Speculative

Use short draft blocks and short verification windows. Suitable for low-end CPU inference and unknown model pairs.

### Adaptive Speculative

Use acceptance history, confidence estimates, model tier, prompt class, temperature, runtime cost, and load to select verification depth.

### Aggressive Speculative

Use larger draft blocks and verification windows when model compatibility and confidence are high.

### Tier-Aware Speculative

Use service tier, queue depth, latency target, memory pressure, and GPU pressure to choose verification budgets.

---

## Telemetry Requirements

GG-CORE should expose aggregate speculative telemetry without storing sensitive content.

Required metrics:

- `speculative.enabled`
- `speculative.mode`
- `speculative.synergy_mode`
- `speculative.draft_model_tier`
- `speculative.target_model_tier`
- `speculative.draft_tokens_requested`
- `speculative.draft_tokens_generated`
- `speculative.verify_tokens_requested`
- `speculative.verify_tokens_used`
- `speculative.tokens_accepted`
- `speculative.tokens_rejected`
- `speculative.acceptance_rate`
- `speculative.mean_accepted_length`
- `speculative.confidence_mean`
- `speculative.confidence_min`
- `speculative.decode_latency_ms`
- `speculative.first_token_latency_ms`
- `speculative.tokens_per_second`
- `speculative.draft_overhead_ms`
- `speculative.verify_overhead_ms`
- `speculative.net_speedup_ratio`
- `speculative.auto_disabled_reason`

Auto-disable reasons should include:

- `draft_overhead_exceeded_savings`
- `acceptance_rate_below_floor`
- `memory_pressure`
- `gpu_memory_pressure`
- `unsupported_model_pair`
- `tokenizer_mismatch`
- `unsupported_sampling_mode`
- `security_policy`
- `operator_config`

---

## Correctness Requirements

Speculative decoding must preserve target-model output correctness.

For deterministic decoding, speculative decoding must produce output equivalent to non-speculative target decoding under the same configuration.

For probabilistic decoding, the verification algorithm must preserve the intended target sampling behavior. If the runtime cannot guarantee sampling correctness for a configuration, it must fall back to single-model decoding.

Rejected draft suffixes must never be committed.

Accepted prefixes must only be committed after target verification.

---

## Security Requirements

The speculative path must preserve the GG-CORE threat model.

1. No network access.
2. IPC-only request flow.
3. No prompt or output persistence in speculation telemetry.
4. Draft models loaded through the same model validation path as target models.
5. Encrypted model support preserved.
6. Path validation preserved.
7. Prompt injection filtering occurs before speculative decoding.
8. Output sanitization occurs after speculative decoding.
9. Audit logging records mode, model tier, and aggregate performance events without sensitive content.
10. Speculative failure falls back safely to single-model decoding.

---

## Benchmark Plan

Benchmarks must compare GG-CORE speculative mode against GG-CORE non-speculative mode.

Benchmarks must distinguish verified results from estimates.

Initial benchmark matrix:

| Scope | Models | Hardware | Required result |
| --- | --- | --- | --- |
| Tier 0 | 0.5B Q4 | 2 to 4 core CPU | Prove safe fallback and measure overhead |
| Tier 1 | 1.5B Q4 | 4 to 8 core CPU | Measure conservative speculation |
| Tier 2 | 3B to 8B Q4 | 8 to 16 core CPU | Measure adaptive speculation |
| Tier 3 | 7B to 14B | Consumer GPU | Measure CPU-draft and GPU-target behavior |
| Tier 4 | 14B to 34B | High-end GPU | Measure aggressive speculation |
| Tier 5 | 34B+ | Multi-GPU/server | Measure tier-aware scheduling |

Prompt classes:

1. Short factual answers.
2. Long-form explanation.
3. Code completion.
4. Structured JSON.
5. Repetitive boilerplate.
6. Creative writing.
7. High-temperature generation.
8. Prompt injection attempts.
9. Long-context continuation.

Required benchmark metrics:

1. Tokens per second.
2. First-token latency.
3. End-to-end latency.
4. Draft overhead.
5. Verification overhead.
6. Acceptance rate.
7. Mean accepted length.
8. Memory overhead.
9. CPU utilization.
10. GPU utilization where applicable.
11. Net speedup ratio.
12. Auto-disable frequency.
13. Correctness or sampling-equivalence status.

---

## Consolidation Plan

TierSynergy should be treated as a GG-CORE first-class subsystem.

The separate `GG-CORE-TierSynergy` repository should be audited and classified as one of the following:

1. Historical prototype, then archived.
2. Commercial packaging layer, with core logic mirrored or migrated into GG-CORE.
3. Extension crate, with GG-CORE owning the public interfaces and TierSynergy owning enterprise implementations.

Until this audit is complete, GG-CORE's internal TierSynergy module is the canonical implementation for runtime behavior.

---

## Rollout Plan

### Phase 1: Documentation and Scope

- Add this ADR.
- Open implementation issues.
- Document TierSynergy as a first-class runtime subsystem.

### Phase 2: Interface Cleanup

- Extend `SpeculativeConfig` or introduce `AdaptiveSpeculativeConfig`.
- Define adaptive speculative traits.
- Define safe fallback semantics.

### Phase 3: TierSynergy Integration

- Make TierSynergy select speculative mode based on model tiers, load hints, hardware profile, and observed performance.
- Add model compatibility checks.
- Add tokenizer compatibility checks.

### Phase 4: Heuristic CPU Implementation

- Implement conservative adaptive scheduling for CPU.
- Add auto-disable behavior.
- Add aggregate metrics.

### Phase 5: Benchmark Matrix

- Benchmark low-end CPU through mainstream CPU.
- Establish verified local results before promoting claims.

### Phase 6: GPU-Aware Scheduling

- Add GPU memory pressure, KV cache pressure, and resident-draft-model awareness.

### Phase 7: Advanced DSpark-Inspired Scheduling

- Evaluate learned confidence heads.
- Evaluate semi-autoregressive drafters.
- Evaluate cost-table based scheduling.
- Add high-concurrency server-class benchmarks.

---

## Consequences

### Positive

1. GG-CORE gets a coherent adaptive inference story.
2. TierSynergy becomes a real scheduling subsystem rather than a side concept.
3. Local CPU inference can gain performance where draft cost is low enough.
4. GPU and server deployments get a path toward larger speculative decoding gains.
5. The runtime can prove when speculation helps instead of assuming it helps.
6. The design preserves GG-CORE's security posture.

### Negative

1. Runtime complexity increases.
2. Bad draft-target pairings may reduce performance.
3. Tiny models may see little gain.
4. Learned confidence heads introduce additional model-loading and validation work.
5. GPU scheduling will require backend-specific tuning.

### Risks

1. Overclaiming performance gains before GG-CORE-specific benchmarks exist.
2. Turning TierSynergy into a policy engine instead of an inference scheduler.
3. Allowing telemetry to leak sensitive information.
4. Creating incompatible behavior between the internal TierSynergy module and the separate TierSynergy repository.
5. Making the decode path too complex without sufficient fallback testing.

---

## Decision Outcome

TierSynergy will be incorporated as the canonical GG-CORE tier-aware inference scheduling layer.

DSpark-inspired adaptive speculative decoding will fold into TierSynergy as a runtime optimization strategy.

The initial implementation will be conservative, CPU-safe, measurable, and reversible.

Future phases may add learned confidence heads, GPU-aware scheduling, multi-GPU support, and server-class cost profiling.
