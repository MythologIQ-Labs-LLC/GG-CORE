# Research Brief — B-21d + B-21h + B-40 (ADR-007 loose ends + advanced-in-CI)

**Date**: 2026-08-01
**Analyst**: The Qor-logic Analyst
**Target**: close the three remaining ADR-007-adjacent items — B-21d (audit loose ends + a
speculative-security test), B-21h (surface speculative telemetry in `status`), B-40 (lint the
`advanced` feature in CI + fix the surfaced lints).
**Scope**: `.github/workflows/rust.yml`, `engine/{quantize,flash_attn_gpu,multi_gpu,simd_tokenizer_v2}.rs`,
`models/speculative_config.rs`, `engine/mod.rs`, `docs/security/THREAT_MODEL.md`,
`cli/{status,status_format}.rs`, `telemetry/metrics.rs`, `engine/adaptive_speculative/{executor,telemetry}.rs`.
Read-only.

---

## Executive Summary

**B-40 is well-scoped and mechanical**: the `advanced` feature is absent from the CI clippy/test matrix
(`[gguf,onnx,ffi,python]`), leaving 14 clippy lints unenforced — all in non-gguf `advanced` files
(quantize ×10, flash_attn_gpu, multi_gpu, simd_tokenizer_v2, speculative_config's `AdaptiveMode`
`Default`). Adding an `advanced` matrix leg + fixing the 14 closes it. **B-21d is mostly already
resolved by the B-21b-1/b-2/c consolidation** — its F2/F3 items are stale, and its cited
"THREAT_MODEL §12.2 `t1_draft_model_path_enforces_allowlist`" is a **phantom reference** (the doc has
sections 1–8; there is no §12.2). The genuine remaining security work is a grounded test that
speculation adds no path-traversal surface. **B-21h is larger than "signature plumbing"**: `status`'s
`build_status` is fed by an IPC client, not a live engine, and the Prometheus speculative counters
(`record_speculative_cycle`) are **dormant** (never called), while the executor feeds only the
in-process `SpeculativeTelemetry`. Closing it means wiring the executor to the Prometheus counters and
deriving `speculative_stats` from the metrics snapshot — counter-derivable fields only (latency /
net-speedup / auto-disable are not in the metrics channel; full fidelity would need a dedicated IPC
status field, a separate item).

## Findings (verified, file:line-grounded)

### B-40 — the 14 unlinted `advanced` lints
`cargo clippy --features "gguf,advanced" --all-targets`: `flash_attn_gpu.rs:114` (`div_ceil`);
`multi_gpu.rs:44` (derivable `Default`); `quantize.rs` ×10 (`div_ceil` at 88/113/159/185/193;
needless-range-loop at 141/143/161/187; same-item-`Vec`-push at 101); `simd_tokenizer_v2.rs:323`
(`map_or`); `speculative_config.rs:21` (derivable `Default` on `AdaptiveMode`). CI (`rust.yml:64`)
matrix is `[gguf,onnx,ffi,python]`; the `features` job runs `cargo clippy --features <f> --all-targets
-- -D warnings` (`:85`) + `cargo test --features <f>` (`:87`). Adding `advanced` compiles all
non-gguf advanced code (the 14 live there) and its inline tests; the `all(gguf,advanced)` integration
tests are `cfg`-compiled-out under `advanced` alone, so the leg is fast (no llama.cpp) and clean once
the 14 are fixed.

### B-21d — F2/F3 are stale; §12.2 is phantom
- **F2 (stale `engine/mod.rs` comments)**: the module was rewritten across B-21b-1/b-2; its current
  header + section comments (`:1-8,:38`) accurately describe the `advanced` speculative modules. No
  reference to the deleted `speculative.rs`/`speculative_v2.rs` remains (grep-clean). **Already
  resolved** — verify-only.
- **F3 (standalone test)**: no `tests/tier_synergy_test.rs` (or standalone speculative test) exists;
  tier_synergy is tested inline. **Moot.**
- **§12.2 security test**: `docs/security/THREAT_MODEL.md` has sections **1–8 only** — no §12, no
  §12.2, no `t1_draft_model_path_enforces_allowlist` spec. The #180 citation is a **phantom
  reference** (B-13-class). The real question: does speculation add a path-traversal surface?
  `register_draft_pair(target_id, draft_id)` takes model **ids** (not paths); a draft must already be
  a registered model, and all models load through the path-allowlist-enforcing `models/loader.rs`
  (THREAT_MODEL §4.3). So the allowlist is **transitively enforced** and an unregistered/bogus
  draft_id safely falls through to single-model (`try_speculative` uses `get_model(..).ok()?`).

### B-21h — IPC boundary + dormant Prometheus counters
`cli/status.rs:272` hardcodes `speculative_stats: None`; `build_status` (`:182`) is fed only by IPC
client responses (`client.get_metrics()`), so it has no live engine handle — the B-21c note
"no engine ref" understates it (it is an IPC client). `telemetry/metrics.rs:171 record_speculative_cycle`
emits `core_speculative_{drafts_total,accepted_tokens,rejected_tokens}` but has **no caller** (dormant;
only re-exported at `telemetry/mod.rs:20`). The executor (`adaptive_speculative/executor.rs:105`) feeds
only the in-process `SpeculativeTelemetry`. `SpeculativeSessionStats` has 11 fields; the 3 counters
cover drafts/accepted/rejected → acceptance-rate + counts are IPC-derivable; latency / net-speedup /
auto-disable are not.

## Recommendations

1. **B-40**: fix the 14 lints (`div_ceil`; `#[derive(Default)]` + `#[default]` for the two enums;
   `map_or`→`is_some_and`/`map`; the `Vec`-push→`vec!`/`resize`; needless-range-loops→iterators, or
   `#[allow(clippy::needless_range_loop)]` with a one-line justification where the index is genuinely
   load-bearing in the SIMD kernels). Add an `advanced` leg to the `rust.yml` `features` matrix.
2. **B-21d**: verify F2 (comment-clean) + F3 (no standalone test) as resolved; add a grounded security
   test `speculative_draft_pair_cannot_bypass_model_allowlist` (register a draft id that was never
   loaded / a would-be traversal id → `try_speculative`/registration does not load it and falls
   through), and **correct the phantom §12.2 citation** in the backlog (cite THREAT_MODEL §4.3 Model
   Loading instead).
3. **B-21h**: call `record_speculative_cycle(accepted, rejected)` from the executor's per-step path so
   the Prometheus counters go live, and derive `speculative_stats` from the metrics snapshot in
   `build_status` (counter fields populated; latency/speedup/auto-disable left at defaults — disclosed
   as an IPC-fidelity limit; full snapshot over IPC is a follow-on).

## Updated Knowledge (Shadow Genome)

**Pre-consolidation "loose ends" rot.** A loose-ends list captured during scoping (#180) can be
largely obsolete after the main work lands (B-21b-1/b-2/c) — re-verify each item against current code
before "fixing" it, and treat doc-section citations (`§12.2`) as claims to verify, not facts (the
phantom-reference / B-13 pattern applies to a project's own security docs too).

---

_Research complete. B-40 mechanical (14 lints + CI leg); B-21d mostly already done + phantom §12.2 →
grounded test + citation fix; B-21h needs executor→Prometheus wiring + metrics-derived stats (partial,
disclosed). One combined cycle, three phases._
