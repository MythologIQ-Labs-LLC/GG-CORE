# Plan: B-07 — Degraded-Mode Policy for Constrained Local Inference

**change_class**: feature

**doc_tier**: standard

**terms_introduced**:
- term: DegradedModePolicy
  home: core-runtime/src/engine/degraded_mode.rs
- term: ResourcePressure
  home: core-runtime/src/engine/degraded_mode.rs
- term: DegradedDecision
  home: core-runtime/src/engine/degraded_mode.rs

**boundaries**:
- limitations:
  - The only executed mechanism is **context reduction** (truncate an over-budget prompt
    before hard-failing). Memory pressure and unsupported capability resolve to
    `Reject { reason }` in this cycle — an explained rejection, not a silent one.
- non_goals:
  - Model swapping / "prefer a smaller/BitNet model" — the `DegradedDecision::PreferModel`
    variant is documented as a future hook the BitNet backend (B-02..B-06) will consume,
    NOT implemented here.
  - Changing `ResourceLimits::try_acquire` or the memory accounting.
- exclusions:
  - No new env plumbing beyond a `DegradedModeConfig` with a `Default`; wiring it into
    `RuntimeConfig` env-loading is a follow-up (the engine takes the config directly).

## Open Questions

None. Research recommended a bounded L2 cycle: pure policy + `evaluate` + context-reduction
mechanism + explanation, with model-swap deferred. Adopted.

## Design Rationale (Simple Made Easy)

The **decision** (policy + pressure → action + explanation) is pure and total, decomplected
from the **mechanism** (truncate the prompt / return the reject error), mirroring B-29a /
B-29b-2:

- `evaluate(&DegradedModePolicy, ResourcePressure) -> DegradedDecision` — no IO, no engine
  state. `ResourcePressure` is a **neutral** input (not either `InferenceError` enum, per
  research F3). Every `DegradedDecision` arm carries a human-readable `reason` (research F4:
  explainability is the product thesis).
- The engine's run path calls a thin `apply_degraded_context(prompt) -> Result<Cow<str>,
  InferenceError>`: within budget → prompt unchanged; over budget → `evaluate(Context{..})`
  → `ReduceContextTo(n)` truncates to `n` tokens (byte-approx, logged) or `Reject{reason}`
  → `Err(ContextExceeded)`. Truncation is the effectful edge; the decision is pure.

## Phase 1: Degraded-mode policy + pure decision

### Affected Files

- `core-runtime/src/engine/degraded_mode.rs` (NEW, ~90 lines) — `DegradedModeConfig`
  (+`Default`), `DegradedModePolicy`, `ResourcePressure`, `DegradedDecision`, pure
  `DegradedModePolicy::evaluate`.
- `core-runtime/src/engine/mod.rs` — `pub mod degraded_mode;` +
  `pub use degraded_mode::{DegradedDecision, DegradedModeConfig, DegradedModePolicy, ResourcePressure};`.

### Changes

```rust
// core-runtime/src/engine/degraded_mode.rs
/// Policy knobs for degraded-mode behavior under resource pressure.
#[derive(Debug, Clone)]
pub struct DegradedModeConfig {
    /// Truncate an over-budget prompt to the context limit instead of failing.
    pub allow_context_reduction: bool,
    /// Never reduce below this many tokens; below it, reject instead of truncating.
    pub min_context_tokens: usize,
}

impl Default for DegradedModeConfig {
    fn default() -> Self {
        Self { allow_context_reduction: true, min_context_tokens: 16 }
    }
}

/// A neutral resource-pressure signal, independent of the InferenceError enums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourcePressure {
    Context { max: usize, got: usize },
    Memory { used: usize, limit: usize },
    Capability { name: String },
}

/// The intentional, explained action degraded mode chooses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradedDecision {
    /// Proceed, reducing the effective context to this many tokens.
    ReduceContextTo { tokens: usize, reason: String },
    /// Fail loud, but with an explanation of the tradeoff.
    Reject { reason: String },
    // Future (BitNet, B-02..B-06): PreferModel { model_id, reason } — not implemented.
}

#[derive(Debug, Clone, Default)]
pub struct DegradedModePolicy {
    config: DegradedModeConfig,
}

impl DegradedModePolicy {
    pub fn new(config: DegradedModeConfig) -> Self { Self { config } }

    /// Decide the degraded action for a pressure signal. Pure and total.
    pub fn evaluate(&self, pressure: ResourcePressure) -> DegradedDecision {
        match pressure {
            ResourcePressure::Context { max, got }
                if self.config.allow_context_reduction && max >= self.config.min_context_tokens =>
            {
                DegradedDecision::ReduceContextTo {
                    tokens: max,
                    reason: format!(
                        "context {got} tok over limit {max}; reduced to {max} (degraded mode)"
                    ),
                }
            }
            ResourcePressure::Context { max, got } => DegradedDecision::Reject {
                reason: format!("context {got} tok over limit {max}; reduction disabled"),
            },
            ResourcePressure::Memory { used, limit } => DegradedDecision::Reject {
                reason: format!("memory {used}B over limit {limit}B; no smaller model available"),
            },
            ResourcePressure::Capability { name } => DegradedDecision::Reject {
                reason: format!("capability '{name}' unsupported by the loaded backend"),
            },
        }
    }
}
```

### Unit Tests

- `core-runtime/src/engine/degraded_mode_tests.rs` (NEW; `#[cfg(test)] #[path]` sibling):
  - `context_over_budget_reduces_when_allowed` — `evaluate(Context{max:100,got:150})` with
    default policy → `ReduceContextTo { tokens: 100, .. }`; assert `tokens` and that
    `reason` is non-empty (explanation present).
  - `context_reduction_disabled_rejects` — policy `allow_context_reduction:false` +
    `Context{..}` → `Reject { reason }` naming "reduction disabled".
  - `context_below_min_tokens_rejects` — `min_context_tokens:200`, `Context{max:100,got:150}`
    → `Reject` (won't reduce below the floor).
  - `memory_pressure_rejects_with_reason` — `Memory{used,limit}` → `Reject` whose reason
    mentions "no smaller model" (the documented future-swap gap).
  - `capability_pressure_rejects_with_reason` — `Capability{name:"chat"}` → `Reject`
    naming the capability. Each asserts the returned decision + reason, not artifact presence.

## Phase 2: Wire context reduction into the engine run path

### Affected Files

- `core-runtime/src/engine/inference.rs` — add a `degraded: DegradedModePolicy` field to
  `InferenceEngine` (default via `DegradedModePolicy::default()` in `new`); add
  `pub fn with_degraded_policy(max_context_length: usize, degraded: DegradedModePolicy) -> Self`
  (constructor that overrides the policy — used by tests and any caller that customizes
  degraded-mode behavior); replace the hard `check_context` call in `run` with
  `apply_degraded_context`, which returns the effective prompt (`Cow<str>`) or an `Err`.
  `run` uses the returned prompt for inference. `check_context` remains for the other
  `run_*` paths (unchanged behavior there).

### Changes

```rust
// InferenceEngine::new — initialize `degraded: DegradedModePolicy::default()`.

/// Resolve the prompt under the degraded-mode policy: within the context
/// budget it is returned unchanged; over budget it is either truncated to the
/// limit (logged) or rejected with an explanation.
fn apply_degraded_context<'p>(
    &self,
    prompt: &'p str,
) -> Result<std::borrow::Cow<'p, str>, InferenceError> {
    let got = prompt.len() / Self::BYTES_PER_TOKEN;
    if got <= self.max_context_length {
        return Ok(std::borrow::Cow::Borrowed(prompt));
    }
    match self.degraded.evaluate(ResourcePressure::Context {
        max: self.max_context_length,
        got,
    }) {
        DegradedDecision::ReduceContextTo { tokens, reason } => {
            tracing::warn!(target: "gg_core::degraded", "{reason}");
            let byte_budget = tokens * Self::BYTES_PER_TOKEN;
            Ok(std::borrow::Cow::Owned(truncate_on_char_boundary(prompt, byte_budget)))
        }
        DegradedDecision::Reject { reason } => {
            tracing::warn!(target: "gg_core::degraded", "reject: {reason}");
            Err(InferenceError::ContextExceeded { max: self.max_context_length, got })
        }
    }
}
```

`run` becomes: `let prompt = self.apply_degraded_context(prompt)?; let model = self.get_model(..).await?; Self::infer_with_model(&model, &prompt, params).await`. The helper
`truncate_on_char_boundary(&str, usize) -> String` (truncates to the largest char boundary
≤ the byte budget, never splitting a UTF-8 codepoint) lives in **`degraded_mode.rs`**
(pub, it is degraded-mode logic) so `inference.rs` stays ≤250 (212 → ~240 with the field +
constructor + `apply_degraded_context`; the truncate helper adds no lines there).

### Unit Tests

- `core-runtime/src/engine/inference_tests.rs` (add):
  - `degraded_context_truncates_over_budget_prompt` — small-context engine (via
    `InferenceEngine::new(N)`) + a non-GGUF mock model registered; a prompt whose estimated
    tokens exceed `N` → `run` succeeds (the mock returns its output) rather than
    `ContextExceeded`, proving truncation occurred. (The `BudgetModel`/mock returns "ok"
    regardless of prompt; success ⇒ the over-budget prompt was reduced, not rejected.)
  - `degraded_context_rejects_when_reduction_disabled` — construct the engine via
    `InferenceEngine::with_degraded_policy(N, DegradedModePolicy::new(cfg))` where
    `cfg.allow_context_reduction:false`, same over-budget prompt → `Err(ContextExceeded)`.
- `core-runtime/src/engine/degraded_mode_tests.rs` (add):
  - `truncate_on_char_boundary_never_splits_utf8` — truncating a multibyte string to a byte
    budget that lands mid-codepoint returns a valid `String` prefix (assert it is valid
    UTF-8 by construction + byte-length ≤ budget + is a prefix of the original).

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| FX-degraded-mode | NEW | core-runtime/src/engine/degraded_mode_tests.rs | `DegradedModePolicy::evaluate` reduces context when allowed (with an explanation) and rejects-with-reason on memory/capability/reduction-disabled pressure |

## Definition of Done

### Deliverable: degraded-mode policy + decision

- **D1**: Resource pressure produces an intentional, explained action (reduce or
  reject-with-reason) rather than a bare hard failure.
- **D2**: `pub fn DegradedModePolicy::evaluate(ResourcePressure) -> DegradedDecision` in
  `core-runtime/src/engine/degraded_mode.rs` (+`DegradedModeConfig`/`ResourcePressure`/
  `DegradedDecision`), re-exported from `engine/mod.rs`.
- **D3**: META_LEDGER entry (canonical markup) records the policy + the context-reduction
  wiring; BACKLOG B-07 → done; FEATURE_INDEX gains the degraded-mode row.
- **D4**: `context_over_budget_reduces_when_allowed` + `memory_pressure_rejects_with_reason`
  pass, asserting the returned decision + explanation.

### Deliverable: context-reduction mechanism in the engine

- **D1**: An over-budget prompt is truncated to the context limit (with a logged
  explanation) instead of hard-failing, when the policy allows it.
- **D2**: `InferenceEngine::apply_degraded_context` + `truncate_on_char_boundary` in
  `inference.rs`; `run` uses the resolved prompt.
- **D3**: Covered by the same ledger entry; SYSTEM_STATE engine tree updated.
- **D4**: `degraded_context_truncates_over_budget_prompt` passes (over-budget prompt →
  success, not `ContextExceeded`); `truncate_on_char_boundary_never_splits_utf8` passes.

## CI Commands

```bash
cargo build -p gg-core --all-features                                   # full-feature compile
cargo clippy -p gg-core --all-targets --all-features -- -D warnings     # lint clean, warnings-as-errors
cargo test -p gg-core                                                   # default: evaluate + truncation + utf8 tests
cargo fmt --check                                                       # formatting
```
