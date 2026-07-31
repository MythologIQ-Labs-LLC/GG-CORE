# Plan: B-21b-2 — Retire Speculative v2 Decoder (double → single)

**change_class**: breaking

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Removes the v2 token-level DECODER (`SpeculativeDecoder` + `DraftModel`/`TargetModel` traits) and
    its GGUF token adapter, making `adaptive_speculative` the sole speculative executor. The shared
    value types (`SpeculativeConfig`/`SpeculativeStats`/`VerifyResult`) are relocated VERBATIM to a
    neutral module — tier_synergy + decode.rs + `verify_draft_tokens` behavior is unchanged.
- non_goals:
  - No migration of tier_synergy/decode.rs onto the adaptive types (would change their public API +
    semantics — the operator chose the minimal, behavior-preserving path).
  - No `adaptive_speculative` change; no retirement of the dormant tier_synergy/decode.rs scaffolding
    themselves (separate concern).
- exclusions:
  - No CI/workflow change. All touched code is `advanced`(+`gguf`)-gated.

## Open Questions

None. Blast radius verified (research #191): no non-test v2-decoder consumer; the value types have 3
dormant consumers → relocate. Operator chose Minimal.

## Design Rationale (Simple Made Easy)

`speculative_v2.rs` complected the duplicate DECODER with the shared VALUE types. Un-complect: delete
the decoder (the redundancy), relocate the value types verbatim to `engine/speculative_types.rs`.
Consumers change only their import path; their behavior is identical. `adaptive_speculative` is left
as the single speculative executor.

## Phase 1: Relocate the value types

### Affected Files

- `core-runtime/src/engine/speculative_types.rs` (NEW, `advanced`-gated) — `SpeculativeConfig`
  (+`Default`), `SpeculativeStats` (+`acceptance_rate`/`avg_tokens_per_verification`/
  `estimated_speedup`), `VerifyResult` (+`accept_all`/`diverge_at`/`with_probabilities`), copied
  verbatim from `speculative_v2.rs` (no field/logic change).
- `core-runtime/src/engine/mod.rs` — `pub mod speculative_v2;` → `pub mod speculative_types;`; the v2
  re-export block (`DraftModel, SpeculativeConfig, SpeculativeDecoder, SpeculativeStats, TargetModel,
  VerifyResult`) → `pub use speculative_types::{SpeculativeConfig, SpeculativeStats, VerifyResult};`
  (decoder/traits dropped).

## Phase 2: Delete the v2 decoder + adapter + tests; switch imports

### Affected Files

- **DELETE** `core-runtime/src/engine/speculative_v2.rs` (the decoder + traits + verification module).
- **DELETE** `core-runtime/src/engine/gguf/speculative.rs` (v2 token adapter `GgufDraftModel`/
  `GgufTargetModel`); `core-runtime/src/engine/gguf/mod.rs` — remove its `pub mod speculative;` +
  `pub use speculative::{GgufDraftModel, GgufTargetModel};`.
- **DELETE** `core-runtime/tests/speculative_test.rs` (tests the deleted v2 decoder).
- `core-runtime/tests/e2e_model_test.rs` — remove the `e2e_speculative_decoding` fn (`:198-…`, uses
  the deleted `GgufDraftModel`/`GgufTargetModel`/`SpeculativeDecoder`).
- `core-runtime/src/models/tier_synergy/mod.rs:24` + `status.rs:3` — `speculative_v2::` →
  `speculative_types::` (or the `engine::` re-export). Behavior unchanged.
- `core-runtime/src/engine/gguf/backend.rs:211-212` + `generator.rs:130` — `speculative_v2::
  VerifyResult` → `speculative_types::VerifyResult`. `gguf/adaptive_speculative.rs` (the B-21c
  verifier) still maps `VerifyResult` — unchanged (type now from `speculative_types`).
- `core-runtime/src/engine/decode.rs:6` — `crate::engine::SpeculativeConfig` re-export still resolves
  (now from `speculative_types`); no change needed if the re-export name is preserved.

### Unit Tests

Behavior-preservation is verified by the SURVIVING tests compiling + passing: `tier_synergy`'s
existing tests (config/stats fields identical, relocated), the B-21c `adaptive_speculative::executor`
tests (unaffected), and a clean `--features "gguf advanced"` build/test. No new test (the deleted v2
decoder had its own tests; the adaptive executor is the tested speculative surface, F-61).

## Feature Inventory Touches

- `entry_id`: `F-18` (Speculative decoding, v2) — `operation`: `n/a-justified` (removed) — its source
  (`speculative_v2.rs`) + test (`speculative_test.rs`) are deleted; the speculative-decoding feature
  is subsumed by **F-61** (adaptive, wired). F-18 row removed from FEATURE_INDEX.

## Definition of Done

### Deliverable: single speculative executor (adaptive); v2 decoder removed

- **D1**: `speculative_v2.rs` + `gguf/speculative.rs` no longer exist; the v2 value types live in
  `engine/speculative_types.rs` and tier_synergy/decode.rs/`verify_draft_tokens` behave unchanged;
  `adaptive_speculative` is the only speculative executor. F-18 removed (subsumed by F-61).
- **D2**: the file moves/deletions + import switches above.
- **D3**: META_LEDGER entries (canonical markup) research #191, plan, audit, seal; BACKLOG B-21b-2 →
  done; FEATURE_INDEX F-18 removed; CHANGELOG note.
- **D4**: `cargo build -p gg-core --features "gguf advanced"` compiles; `cargo test -p gg-core
  --features "gguf advanced"` green (incl. adaptive executor + tier_synergy tests); `cargo build
  --features gguf` + default compile (CI-safe); `cargo fmt --check` + clippy (changed files) clean.

## CI Commands

- `cargo build -p gg-core --features "gguf advanced"` — the v2 removal + relocation compiles
- `cargo test -p gg-core --features "gguf advanced"` — surviving tests (adaptive + tier_synergy) pass
- `cargo build -p gg-core --features gguf` — CI-safe (advanced-gated path compiled out; strictly
  covers the bare default build, which also compiles the advanced path out)
- `cargo fmt --check`
