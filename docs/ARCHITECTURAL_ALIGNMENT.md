# GG-CORE Architectural Alignment

## Purpose

This document defines the architectural role GG-CORE must play inside the broader COREFORGE ecosystem.

GG-CORE is not an assistant, agent, planner, governance authority, memory system, or tool executor. GG-CORE is the contained local inference runtime. Its job is to execute model workloads safely, efficiently, and offline while exposing enough model lifecycle controls for higher-level systems to route work intelligently.

The corrected architectural framing is simple:

> GG-CORE provides local model execution, model lifecycle control, model residency primitives, and inference safety boundaries. COREFORGE, Axis, Arbiter, Vault, CodeGenome, and TierSynergy decide why, when, and under what authority those models are used.

This distinction matters because agentic systems collapse when inference runtimes accidentally become policy engines, business logic layers, memory stores, or action executors. GG-CORE should remain powerful, but deliberately narrow.

## Current Architectural Position

GG-CORE already contains primitives that support adaptive local inference:

- Offline runtime execution with no HTTP server dependency.
- IPC-oriented request handling.
- Model registry and lifecycle management.
- Inference engine model registration and unregistration.
- Model pool support for fast switching between resident models.
- Swap manager support for preload, drain, atomic route swap, and old model cleanup.
- Memory reporting and context limit enforcement.
- Scheduler priority support.
- Request interception extension points for TierSynergy or other policy layers.

These features make GG-CORE more than a single-model inference wrapper. It is evolving into a model residency runtime, meaning it can support multiple model classes and switch between them based on task need, available memory, and latency constraints.

## Correct System Boundary

GG-CORE owns these responsibilities:

| Responsibility | GG-CORE Role |
|---|---|
| Model loading | Load model artifacts from approved local paths. |
| Model registration | Register loaded models into the runtime registry and inference engine. |
| Model unregistration | Remove inactive or evicted models from the active runtime. |
| Inference execution | Execute prompt or embedding work against an already selected model. |
| Model pool state | Track resident models, active model, memory use, and switch latency. |
| Hot swap | Preload, drain in-flight requests, atomically swap routes, and clean up old models. |
| Runtime safety | Enforce containment, context limits, cancellation, and memory limits. |
| Runtime telemetry | Report model state, request metrics, latency, memory, and health. |

GG-CORE does not own these responsibilities:

| Responsibility | Owning Layer |
|---|---|
| User intent interpretation | COREFORGE Axis. |
| Agent identity and persona behavior | COREFORGE. |
| Governance policy and action authority | Arbiter, FailSafe, or external governance layers. |
| Durable user memory | Vault, EvolveAI, or future memory subsystem. |
| Codebase semantic truth | CodeGenome. |
| Tool execution authority | Synapse and Arbiter-gated tool systems. |
| Product tier licensing | COREFORGE licensing layer. |
| Business workflows | COREFORGE application layer. |

## Strategic Alignment With COREFORGE

COREFORGE needs local agentic behavior across machines with different capabilities. A static single-model runtime is not enough. COREFORGE should be able to ask GG-CORE to run the smallest sufficient model for the current task, then free memory when the active cognitive need changes.

The target behavior is not merely "run a model." The target behavior is:

1. Classify the cognitive operation.
2. Select the minimum viable model tier.
3. Check whether the required model is already active.
4. If active, reuse it.
5. If resident but inactive, switch to it.
6. If not resident, preload it.
7. If memory is constrained, evict the least useful inactive model.
8. Execute inference.
9. Report telemetry and memory impact.
10. Allow higher-level policy to refine future selections.

This is the core of adaptive local cognition.

## Model Residency Lifecycle

GG-CORE should standardize model residency around the following lifecycle.

```text
DISCOVER
  model manifest is found and validated

REGISTER
  model metadata and handle are recorded

PRELOAD
  model is loaded into memory but not necessarily active

WARM
  optional warmup inference reduces first-use latency

ACTIVATE
  model becomes the active route for a persona or operation class

EXECUTE
  inference runs under cancellation, context, and memory limits

OBSERVE
  latency, memory, throughput, failure, and quality metadata are recorded

EVICT
  inactive model is dropped from memory when pressure requires it
```

This lifecycle should remain runtime-level. COREFORGE and TierSynergy may decide which state transition is needed, but GG-CORE should execute the transition safely.

## Recommended Runtime Contract

GG-CORE should expose a stable model residency contract to COREFORGE and TierSynergy.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelResidencyAction {
    UseActive,
    SwitchResident,
    PreloadAndSwitch,
    EvictThenLoad,
    DegradeToSmallerModel,
    RejectInsufficientResources,
}

#[derive(Debug, Clone)]
pub struct ModelResidencyRequest {
    pub operation_class: String,
    pub persona_id: Option<String>,
    pub preferred_model_id: Option<String>,
    pub required_model_tier: ModelTier,
    pub max_latency_ms: Option<u64>,
    pub max_memory_bytes: Option<usize>,
    pub context_tokens_required: usize,
    pub risk_level: String,
}

#[derive(Debug, Clone)]
pub struct ModelResidencyDecision {
    pub action: ModelResidencyAction,
    pub selected_model_id: Option<String>,
    pub evict_model_id: Option<String>,
    pub reason: String,
}
```

This type does not need to decide policy by itself. It gives GG-CORE a clear way to receive a decision from TierSynergy or COREFORGE and execute the model movement safely.

## Suggested Runtime Facade

A thin runtime facade should compose the existing `ModelPool`, `SwapManager`, `SmartLoader`, and `InferenceEngine` into one safe operational surface.

```rust
pub struct ModelResidencyManager {
    pool: Arc<ModelPool>,
    swap_manager: Arc<SwapManager>,
    smart_loader: Arc<SmartLoader>,
    inference_engine: Arc<InferenceEngine>,
}

impl ModelResidencyManager {
    pub async fn prepare_model(
        &self,
        decision: ModelResidencyDecision,
    ) -> Result<String, ResidencyError> {
        match decision.action {
            ModelResidencyAction::UseActive => {
                self.pool.active().await.ok_or(ResidencyError::NoActiveModel)
            }
            ModelResidencyAction::SwitchResident => {
                let model_id = decision.selected_model_id.ok_or(ResidencyError::MissingModel)?;
                self.pool.switch_to(&model_id).await?;
                Ok(model_id)
            }
            ModelResidencyAction::PreloadAndSwitch => {
                let model_id = decision.selected_model_id.ok_or(ResidencyError::MissingModel)?;
                self.smart_loader.load_hint(&model_id, LoadHint::Interactive).await?;
                self.pool.switch_to(&model_id).await?;
                Ok(model_id)
            }
            ModelResidencyAction::EvictThenLoad => {
                if let Some(evict) = decision.evict_model_id {
                    self.pool.remove(&evict).await;
                }
                let model_id = decision.selected_model_id.ok_or(ResidencyError::MissingModel)?;
                self.smart_loader.load_hint(&model_id, LoadHint::Interactive).await?;
                self.pool.switch_to(&model_id).await?;
                Ok(model_id)
            }
            ModelResidencyAction::DegradeToSmallerModel => {
                let model_id = decision.selected_model_id.ok_or(ResidencyError::MissingModel)?;
                self.pool.switch_to(&model_id).await?;
                Ok(model_id)
            }
            ModelResidencyAction::RejectInsufficientResources => {
                Err(ResidencyError::InsufficientResources(decision.reason))
            }
        }
    }
}
```

The names can change. The architectural need should not.

## Model Tier Semantics

GG-CORE should support stable model tier metadata. Tier names should describe runtime role, not customer account level.

| Model Tier | Intended Role | Typical Use |
|---|---|---|
| Light | Fast, low memory, low latency | intent extraction, routing, health summaries, simple classification |
| Balanced | General reasoning under local constraints | assistant responses, task planning, memory synthesis |
| Quality | Larger context or higher quality reasoning | code reasoning, deep synthesis, complex planning, high-risk explanation |
| Specialist | Domain-specific model | embeddings, policy classification, code review, voice, OCR, security checks |

The runtime should avoid hardcoded assumptions that model size equals quality. The manifest should carry observed performance and capability metadata.

## Manifest Requirements

Each model manifest should eventually include:

```toml
model_id = "qwen2.5-1.5b-balanced"
display_name = "Qwen 2.5 1.5B Balanced"
tier = "balanced"
format = "gguf"
capabilities = ["chat", "summarization", "routing"]
context_tokens = 8192
estimated_memory_bytes = 1400000000
cold_load_ms_estimate = 2500
warm_switch_ms_target = 50
license = "apache-2.0"
recommended_personas = ["alden", "axis"]
```

## Alignment With TierSynergy

TierSynergy should not be reduced to multi-user account tiers. Its stronger role is inference triage.

TierSynergy should decide which model tier is appropriate. GG-CORE should execute the model residency change.

```text
COREFORGE Axis
  classifies operation

TierSynergy
  chooses model tier and residency action

GG-CORE
  loads, switches, evicts, and runs the selected model

Arbiter
  validates policy and logs decisions

Vault
  persists result context and memory
```

## Required Implementation Work

### Phase 1: Document the current runtime primitives

Confirm and document the exact current behavior of:

- `ModelPool::preload`
- `ModelPool::switch_to`
- `ModelPool::remove`
- `SwapManager::execute_swap`
- `InferenceEngine::register_model`
- `InferenceEngine::unregister_model`
- `InferenceEngine::model_memory_usage`

### Phase 2: Add runtime model residency status

Expose one runtime status method that reports:

- Active model.
- Loaded models.
- Resident memory use.
- Available memory estimate.
- Switch latency metrics.
- Eviction count.
- Warmup status.

### Phase 3: Add a stable model residency facade

Create a single API surface that higher-level systems can call without knowing whether the action uses `ModelPool`, `SwapManager`, `SmartLoader`, or direct inference engine calls.

### Phase 4: Bind COREFORGE GG-CORE integration to residency

COREFORGE should stop treating model switching as a simple persona-to-model lookup. It should ask the residency layer to prepare the right model before inference.

### Phase 5: Add integration tests

The minimum test suite should prove:

1. A light model can be loaded and activated.
2. A balanced model can be preloaded and switched to.
3. An inactive model can be evicted under memory pressure.
4. In-flight requests are drained before hot swap.
5. A rejected residency decision does not unload the active model.
6. Telemetry records model switch latency and evictions.

## Acceptance Criteria

GG-CORE is architecturally aligned when the following are true:

- The runtime can report which model is active and which models are resident.
- COREFORGE can request a model tier without hardcoding a specific model ID.
- GG-CORE can switch to a resident model without cold loading it.
- GG-CORE can evict inactive models under memory pressure.
- GG-CORE can reject model activation safely when resources are insufficient.
- All model lifecycle transitions are observable.
- GG-CORE never makes product, user, tool, or governance decisions.

## Non-Goals

GG-CORE should not become:

- A chat product.
- A multi-agent orchestrator.
- A memory system.
- A policy authority.
- A workflow engine.
- A cloud service dependency.
- A general tool execution framework.

GG-CORE wins by staying narrow, fast, local, observable, and safe.

## Strategic Summary

GG-CORE is the compute heart of COREFORGE, but it should not become the brain, memory, judge, or hands.

Its strategic value is adaptive local inference: run the right model, at the right time, under the right resource envelope, without leaking authority outside the runtime boundary.

That is the alignment target.
