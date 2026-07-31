# Plan: B-21b-1 — Retire Speculative v1 (triple → double)

**change_class**: breaking

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Removes the v1 speculative implementation (`engine/speculative.rs`) and makes **v2**
    (`speculative_v2.rs`) the single token-level decoder, re-exported under the canonical unsuffixed
    names. The GGUF adapter + backend signatures are ported from v1's to v2's traits/`VerifyResult`.
    All speculative code is `#[cfg(feature = "advanced")]`-gated and dormant (no production/secure
    path consumer), so the removal has no runtime effect on `Runtime::infer`.
- non_goals:
  - No `adaptive_speculative` change; no wiring (that is B-21c); v2 is NOT yet retired (B-21b-2).
  - No behavior change to the surviving v2 decoder.
- exclusions:
  - No CI/workflow change.

## Open Questions

None. Blast radius fully mapped (scoping #180 + B-21b-1 survey). v2 is a superset of v1
(`VerifyResult` gains `probabilities`; `DraftModel`/`TargetModel` gain `get_probabilities`;
`SpeculativeConfig` gains `min_acceptance_rate`/`max_draft_tokens`/`adaptive`). The suffixed v2
re-exports (`SpeculativeV2Config`/`Decoder`) are unused → safe to drop.

## Design Rationale (Simple Made Easy)

Three parallel speculative impls is the redundancy to kill. v1 is the least capable and its only
unique asset is the GGUF backend adapter; v2 is a strict superset (rejection sampling, stats). So v1
retires into v2: port the GGUF adapter + the two backend `VerifyResult` return sites to v2, promote
v2 to the canonical unsuffixed `engine::{DraftModel, TargetModel, VerifyResult, SpeculativeConfig,
SpeculativeDecoder, SpeculativeStats}` re-export (so existing unsuffixed consumers keep resolving —
now to v2), then delete `speculative.rs`. Triple → double, with no loss of capability.

## Phase 1: Port GGUF adapter + backend signatures to v2

### Affected Files

- `core-runtime/src/engine/gguf/speculative.rs` — `use crate::engine::speculative::{…}` →
  `speculative_v2::{…}`; add the v2-required `get_probabilities(&self, context, tokens) -> Vec<f32>`
  to both `impl DraftModel for GgufDraftModel` and `impl TargetModel for GgufTargetModel`, returning
  uniform `vec![1.0; tokens.len()]` (the GGUF generator exposes no per-token probabilities; uniform =
  no probability signal, honest placeholder; the GGUF verify path uses `verify_draft_tokens`
  directly, not `get_probabilities`).
- `core-runtime/src/engine/gguf/backend.rs` (`:211-212`) + `core-runtime/src/engine/gguf/generator.rs`
  (`:130`) — change `crate::engine::speculative::VerifyResult` → `speculative_v2::VerifyResult`. The
  bodies construct via `VerifyResult::{accept_all, diverge_at}` (present in v2), so they compile
  unchanged (v2 fills `probabilities` with uniform).

## Phase 2: Promote v2 to canonical + delete v1

### Affected Files

- `core-runtime/src/engine/mod.rs` — remove `#[cfg(feature="advanced")] pub mod speculative;` and the
  v1 re-export (`pub use speculative::{DraftModel, SpeculativeConfig, SpeculativeDecoder, TargetModel,
  VerifyResult};`). Change the v2 re-export to the canonical unsuffixed names:
  `#[cfg(feature="advanced")] pub use speculative_v2::{DraftModel, TargetModel, VerifyResult,
  SpeculativeConfig, SpeculativeDecoder, SpeculativeStats};` (drop the unused `…V2Config`/`…V2Decoder`
  aliases). Now `engine::{DraftModel, SpeculativeConfig, …}` resolve to v2.
- `core-runtime/src/engine/speculative.rs` — **DELETE**.
- `core-runtime/src/engine/decode.rs` (`:6` `use crate::engine::SpeculativeConfig;`) — verify it
  compiles against v2's `SpeculativeConfig`; if it constructs a literal, add `..Default::default()`
  for the new fields (else no change — it now resolves to v2 transparently).

### Unit Tests (ported)

- `core-runtime/tests/speculative_test.rs` (F-18) — port from v1's decoder to v2 (the canonical
  `gg_core::engine::{…}` imports now resolve to v2, so imports are unchanged): add `get_probabilities`
  (uniform) to `MockDraft` + `MockTarget`; add `..SpeculativeConfig::default()` to the 6 config
  literals (for the 3 new fields). The 6 behavioral tests (accept / reject / fallback / eos /
  draft-tokens / disabled) assert the SAME outcomes against v2 — v2 with uniform probabilities +
  the existing `acceptance_threshold` behaves equivalently for these mocks.
- `core-runtime/tests/e2e_model_test.rs` (`:200`) — change `use gg_core::engine::speculative::{…}` →
  `gg_core::engine::{SpeculativeConfig, SpeculativeDecoder}` (the canonical re-export), + config
  `..Default::default()` if it constructs a literal.

## Feature Inventory Touches

- `entry_id`: `F-18` (Speculative decoding) — `operation`: `MODIFIED` — retarget the source from
  `engine/speculative.rs` (deleted) to `engine/speculative_v2.rs` (the surviving canonical
  token-level decoder); `test_path`: `core-runtime/tests/speculative_test.rs` (ported to v2). The
  feature "speculative decoding" persists; only its implementing file consolidates v1→v2.

## Definition of Done

### Deliverable: single token-level speculative decoder (v2); v1 removed

- **D1**: `engine/speculative.rs` (v1) no longer exists; `engine::{DraftModel, TargetModel,
  VerifyResult, SpeculativeConfig, SpeculativeDecoder, SpeculativeStats}` resolve to v2; the GGUF
  adapter + backend use v2; F-18 points at v2 and its ported test passes. Triple redundancy → double
  (`speculative_v2` + `adaptive_speculative`).
- **D2**: the file changes above; `speculative.rs` deleted; `gguf/speculative.rs` implements v2's
  traits; `engine/mod.rs` re-exports v2 canonically.
- **D3**: META_LEDGER entries (canonical markup) plan, audit, seal; BACKLOG B-21b-1 → done;
  FEATURE_INDEX F-18 retargeted; CHANGELOG note (advanced-gated API change).
- **D4**: `cargo build -p gg-core --features advanced` compiles; `cargo test -p gg-core --features
  advanced --test speculative_test` — the 6 ported tests pass; `cargo test -p gg-core --features
  advanced` green; `cargo fmt --check` + `cargo clippy -p gg-core --features advanced -- -D warnings`
  clean. (`advanced` is a pure Cargo feature — locally buildable on the Windows dev host.)

## CI Commands

- `cargo build -p gg-core --features advanced` — v1 removal + v2 promotion compiles
- `cargo test -p gg-core --features advanced --test speculative_test` — ported v2 tests pass
- `cargo fmt --check`
- `cargo clippy -p gg-core --features advanced -- -D warnings`
