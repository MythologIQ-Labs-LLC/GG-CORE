# Plan: B-21f — KV-Cache Reuse + Pluggable Draft (the real speedup)

**change_class**: feature

**doc_tier**: system

**terms_introduced**:
- term: GgufSpeculativeSession
  home: core-runtime/src/engine/gguf/speculative_session.rs
- term: PromptLookupDraft
  home: core-runtime/src/engine/adaptive_speculative/prompt_lookup.rs

**boundaries**:
- limitations:
  - Makes the wired GGUF speculative path actually faster by reusing the llama.cpp KV cache across
    steps: the prompt context is decoded ONCE into a persistent `LlamaContext`; each step decodes only
    the committed delta + the draft block, and rolls back the draft positions after verify. Adds a
    model-free `PromptLookupDraft` (n-gram) so a real >1x is demonstrable on the single local qwen-0.5b,
    and keeps the model-pair path (`register_draft_pair`) wired for a downloaded draft/target pair.
- non_goals:
  - No second model is downloaded in this cycle (the model-pair path is wired + gated-tested but
    activates only when a 2nd GGUF is provisioned). No change to the security path (scan/sanitize wrap
    `run` in the façade). No change to the ONNX path. No draft-model KV session (only the expensive
    TARGET gets the persistent session this cycle; a model-based draft still uses stateless calls).
- exclusions:
  - No CI change — all new code is `all(gguf, advanced)`-gated and CI has no model, so the real-model
    tests early-return in CI (verified locally).

## Open Questions

None. Research #195 confirmed the API (`clear_kv_cache_seq`/incremental `decode`), the executor's
committed-prefix invariant, the self-referential-context obstacle (→ `self_cell`), and local-model
verifiability. Operator chose the superset (enabler + prompt-lookup draft + model-pair path).

## Design Rationale (Simple Made Easy)

The penalty is a **borrow**, not an algorithm: `create_context(&self)` returns a `LlamaContext` that
borrows the model, so today's code throws it away each step and re-decodes the whole context. Un-
complect *owning the context* from *borrowing the model* with `self_cell` — a `GgufSpeculativeSession`
owns `Arc<LlamaBackendInner>` and, self-referentially, the `LlamaContext` that borrows it, plus a
`committed_pos` cursor. The executor and the `BlockDraftModel`/`TargetVerifier` traits are unchanged;
all statefulness lives inside the target verifier's session (interior mutability). The draft is
separated from the enabler: `PromptLookupDraft` is a pure, model-free `BlockDraftModel` (n-gram copy
from context) that composes with the same session, so a speedup is demonstrable now; the model-pair
draft is the same trait wired to a second generator when one exists.

## Phase 1: Persistent KV session (the enabler)

### Affected Files

- `core-runtime/Cargo.toml` — add `self_cell = { version = "1", optional = true }`; include it in the
  `advanced` feature list (so it is pulled only for the gated path).
- `core-runtime/src/engine/gguf/speculative_session.rs` (NEW, `all(gguf, advanced)`-gated) —
  `GgufSpeculativeSession` built with the `self_cell!` macro: owner `Arc<LlamaBackendInner>`, dependent
  `LlamaContext<'_>`. State: `committed_pos: usize`, a persistent greedy `LlamaSampler`, a reusable
  `LlamaBatch`. Methods:
  - `pub fn new(inner: Arc<LlamaBackendInner>, prompt: &[u32]) -> Result<Self, InferenceError>` —
    build the context, decode the prompt once (positions `0..P`), `committed_pos = P`.
  - `fn ensure_committed(&mut self, context: &[u32]) -> Result<(), InferenceError>` — decode
    `context[committed_pos..]` at positions `committed_pos..context.len()`; advance `committed_pos`.
    (The executor only ever passes a growing committed prefix — research F3 — so this is a pure delta.)
  - `pub fn verify(&mut self, context: &[u32], draft: &[u32]) -> Result<VerifyResult, InferenceError>`
    — `ensure_committed(context)`; decode `draft` at positions `committed_pos..committed_pos+k` with
    logits; greedily sample each draft position; on first mismatch return `diverge_at(i, correction)`,
    else `accept_all(k)`; then **roll back the draft positions**:
    `clear_kv_cache_seq(Some(0), Some(committed_pos as u32), None)` so the KV holds only the committed
    prefix (the executor re-supplies the actually-committed tokens next step via `ensure_committed`).
  - `pub fn generate_one(&mut self, context: &[u32]) -> Result<u32, InferenceError>` —
    `ensure_committed(context)`; sample one token at the last committed logit (do NOT commit it to KV —
    the executor will pass it back in `context` next step, keeping KV/`committed_pos` the single source
    of truth).
- `core-runtime/src/engine/gguf/backend.rs` — expose the pieces the session needs without duplicating
  them: make `LlamaBackendInner::create_context` reachable to the session module (same crate) and add a
  small `decode_batch(ctx, tokens, start_pos, logits_for)` helper reused by session + existing methods
  (keeps `add_seq`/`add_one`/`decode` DRY; no behavior change to existing callers).

### Unit Tests (`speculative_session_tests.rs`, `#[path]`, `all(gguf, advanced)` + model-gated)

- `session_output_equals_fresh_context` — with the local qwen model (early-return if absent): a full
  greedy generation driven through the session (ensure_committed + generate_one) is **token-identical**
  to `generate_from_tokens` (fresh context). Proves the persistent-KV path is behavior-preserving.
- `verify_rollback_leaves_committed_prefix` — after a `verify` with a partially-wrong draft,
  `kv_cache_seq_pos_max(0)` equals `committed_pos - 1` (draft positions removed; committed prefix
  intact), and a subsequent `generate_one` yields the same token as if the draft had never been tried.

## Phase 2: Prompt-lookup draft (model-free) + session-backed verifier

### Affected Files

- `core-runtime/src/engine/adaptive_speculative/prompt_lookup.rs` (NEW, `advanced`-gated) —
  `PromptLookupDraft { ngram: usize, max_draft: usize }` implementing `BlockDraftModel`: find the most
  recent earlier occurrence of the last `ngram` tokens of `context`; propose up to `max_draft`
  following tokens as the draft block (`DraftBlock::from_tokens`). No match ⇒ empty block (the executor
  falls back to a single target token). Pure function of `context` — no model, no I/O.
- `core-runtime/src/engine/gguf/adaptive_speculative.rs` — `GgufTargetVerifier` gains a
  `Mutex<Option<GgufSpeculativeSession>>`; `verify`/`generate_one` lazily create the session from the
  target `Arc<LlamaBackendInner>` + the incoming prompt context on first call, then route through it
  (replacing the stateless `verify_draft_tokens`/`generate_tokens` calls). `eos_token` unchanged.

### Unit Tests (`prompt_lookup_tests.rs`, `#[path]`, `advanced`-gated, no model)

- `proposes_continuation_after_ngram_match` — context `[a b c X Y a b c]` with `ngram=3` proposes
  `[X Y ...]` (the tokens that followed the earlier `a b c`).
- `empty_block_when_no_match` — a context with no repeated n-gram yields an empty draft block.
- `respects_max_draft` — proposed block length is capped at `max_draft`.

## Phase 3: Engine wiring + real-model speculative test

### Affected Files

- `core-runtime/src/engine/inference_speculative.rs` — when the speculative branch builds the GGUF
  path (`try_speculative`): construct the session-backed `GgufTargetVerifier`, and select the drafter —
  the model-pair draft when `register_draft_pair` resolves a distinct draft generator, else the
  model-free `PromptLookupDraft` (config-driven `ngram`/`max_draft`). Any miss ⇒ existing single-model
  fallthrough. No security-path change.
- `core-runtime/src/models/speculative_config.rs` — add `prompt_lookup_ngram: usize` (default 3) and
  reuse `max_draft_tokens` for the lookup cap; surfaced through `AdaptiveSpeculativeConfig`.

### Unit Test (`tests/e2e_speculative_kv_test.rs`, NEW, `all(gguf, advanced)` + model-gated)

- `speculative_prompt_lookup_matches_single_model` — with qwen present (early-return otherwise): on a
  deliberately repetitive prompt, the speculative path (prompt-lookup draft + session verifier, greedy)
  produces **token-identical** output to single-model greedy generation, and logs the wall-clock of
  both (asserts correctness; prints the speedup for the operator — does not assert a ratio, since
  wall-clock is host-dependent).

## Feature Inventory Touches

- `entry_id`: `F-62` (Speculative KV-cache reuse — persistent GGUF session) — `operation`: `NEW` —
  `test_path`: `core-runtime/src/engine/gguf/speculative_session_tests.rs` — the enabler that removes
  the per-step full-context re-decode.
- `entry_id`: `F-63` (Prompt-lookup draft — model-free speculative draft) — `operation`: `NEW` —
  `test_path`: `core-runtime/src/engine/adaptive_speculative/prompt_lookup_tests.rs`.

## Definition of Done

### Deliverable: KV reuse + pluggable draft, speculation demonstrably faster on one model

- **D1**: The wired GGUF speculative path decodes the prompt once and only the committed delta + draft
  per step (no full-context re-decode); a model-free prompt-lookup draft makes a >1x wall-clock speedup
  demonstrable on the local qwen model; the model-pair path stays wired for a future 2nd model. Off by
  default; security path unchanged.
- **D2**: `GgufSpeculativeSession` (self_cell) + `PromptLookupDraft` + session-backed `GgufTargetVerifier`
  + the drafter-selection branch in `try_speculative`, all `all(gguf, advanced)`(/`advanced`)-gated.
- **D3**: META_LEDGER #195 research → plan → audit → seal; BACKLOG B-21f done; FEATURE_INDEX F-62/F-63
  NEW; GOVERNANCE_INDEX Tier 4; CHANGELOG note; `self_cell` dep recorded.
- **D4**: `cargo test -p gg-core --features "gguf advanced"` green; the model-gated tests
  (`session_output_equals_fresh_context`, `speculative_prompt_lookup_matches_single_model`) pass
  **locally with qwen present** (token-equivalence); `prompt_lookup_tests` (no model) pass; CI-safe
  legs (`--features gguf`, default) compile; fmt + clippy (changed files) clean.

## CI Commands

- `cargo build -p gg-core --features "gguf advanced"` — the KV-reuse path + self_cell compile
- `cargo test -p gg-core --features "gguf advanced" prompt_lookup` — model-free draft tests pass
- `cargo test -p gg-core --features "gguf advanced"` — full suite (model-gated tests early-return in CI)
- `cargo build -p gg-core --features gguf` — CI-safe (advanced path + self_cell compiled out)
- `cargo fmt --check`
