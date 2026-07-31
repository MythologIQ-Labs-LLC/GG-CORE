# Research Brief — B-21b-2: Retire Speculative v2 (double → single)

**Date**: 2026-07-31
**Analyst**: The Qor-logic Analyst
**Target**: B-21b-2 — make `adaptive_speculative` the SOLE speculative EXECUTOR by removing the v2
decoder, while preserving the behavior of the dormant components that share v2's value types.
**Scope**: `engine/speculative_v2.rs`, `engine/gguf/speculative.rs`, `engine/mod.rs`,
`models/tier_synergy/`, `engine/decode.rs`, `gguf/{backend,generator}.rs`, the v2 tests, F-18.

---

## Executive Summary

`speculative_v2.rs` bundles two things: the duplicate token-level **DECODER** (`SpeculativeDecoder` +
`DraftModel`/`TargetModel` traits — the actual redundancy now that `adaptive_speculative` is the wired
executor) and the **value types** (`SpeculativeConfig`, `SpeculativeStats`, `VerifyResult`) that three
DORMANT components depend on: `tier_synergy`'s public API, `decode.rs`'s `DecodeConfig`, and the GGUF
`verify_draft_tokens` return type. Operator decision (AskUserQuestion): the **minimal, behavior-
preserving** path — delete the decoder + traits + the v2 GGUF token adapter + v2 decoder tests, and
**relocate the value types verbatim** into a neutral `engine/speculative_types.rs` so the dormant
consumers keep working unchanged. Result: `adaptive_speculative` is the sole speculative executor
(double → single), no behavior change to tier_synergy/decode.rs.

## Findings (verified)

### F1 — no non-test consumer of the v2 DECODER
`SpeculativeDecoder`/`DraftModel`/`TargetModel` are referenced only by `speculative_v2.rs` itself, the
v2 GGUF adapter `gguf/speculative.rs` (being deleted), the `engine/mod.rs` re-export, and the tests
(`tests/speculative_test.rs`, `tests/e2e_model_test.rs:199-208`). No production code drives the v2
decoder. Clean deletion.

### F2 — the value types have 3 dormant consumers (relocate, don't migrate)
- `models/tier_synergy/{mod.rs:24,38-39,49-50,56,219, status.rs:3,15}` — `SpeculativeConfig`/
  `SpeculativeStats` in `TierSynergy`'s fields + public API (`with_spec_config`, `stats()`,
  `SynergyStatus.spec_config`). Their field shapes differ from the adaptive types, so migrating would
  change TierSynergy's public API + semantics → **relocate the structs verbatim** (behavior-preserving).
- `engine/decode.rs:6,30` — `DecodeConfig.speculative: Option<SpeculativeConfig>` (config type only;
  `decode.rs` does NOT use the v2 decoder). Relocate.
- `gguf/{backend.rs:211-212, generator.rs:130}` — `verify_draft_tokens` returns `VerifyResult`
  (consumed by the B-21c GGUF adaptive verifier, which maps it). Keep `VerifyResult`, from the new
  module.

### F3 — the relocation target
A new `engine/speculative_types.rs` (`advanced`-gated) holds `SpeculativeConfig` (+Default),
`SpeculativeStats` (+`acceptance_rate`/`avg_tokens_per_verification`/`estimated_speedup`), and
`VerifyResult` (+`accept_all`/`diverge_at`/`with_probabilities`) — copied verbatim from
`speculative_v2.rs`. `engine/mod.rs` swaps `pub mod speculative_v2;` → `pub mod speculative_types;` and
re-exports `SpeculativeConfig`/`SpeculativeStats`/`VerifyResult` (the surviving types); the decoder
re-exports (`DraftModel`/`TargetModel`/`SpeculativeDecoder`) are dropped. Consumers switch their
`speculative_v2::` imports to `speculative_types::` (or the re-export).

### F4 — F-18 folds into F-61
F-18 ("Speculative decoding", source `speculative_v2.rs`, test `speculative_test.rs`) — both artifacts
are deleted; the speculative-decoding feature is now F-61 (adaptive, wired). Remove F-18 (subsumed).

## Recommendations

1. **B-21b-2 deliverable (minimal, behavior-preserving)**: NEW `engine/speculative_types.rs`
   (relocated value types); DELETE the v2 decoder/traits/verification-module (rest of
   `speculative_v2.rs`), `gguf/speculative.rs`, `tests/speculative_test.rs`, and the e2e v2 block;
   switch imports (`engine/mod.rs`, `tier_synergy`, `decode.rs`, `gguf/backend.rs`+`generator.rs`,
   `gguf/mod.rs`) to `speculative_types`; remove FEATURE_INDEX F-18. `adaptive_speculative` is the sole
   executor.
2. Verify behavior-preservation: `tier_synergy`'s existing tests + `decode.rs` compile/behave
   unchanged (same struct fields, relocated); `--features "gguf advanced"` builds + tests green.

## Updated Knowledge (Shadow Genome)

**Separate the duplicate from the shared.** A "delete the old impl" refactor stalls when the old file
also defines value types that other (even dormant) code shares. Split it: delete the duplicated
BEHAVIOR (the decoder), relocate the shared VALUE types verbatim, and you retire the redundancy
without churning unrelated consumers' semantics.

---

_Research complete. B-21b-2 = relocate v2's value types to `engine/speculative_types.rs`, delete the
v2 decoder/adapter/tests, switch imports. Behavior-preserving; adaptive is the sole executor. Operator
chose the minimal path._
