# Backlog

Reconstructed governance artifact (required, non-scaffold). Repaired via
`/qor-remediate` on 2026-07-08 because the Phase 109 governance-health schema
requires it and it had never been created since bootstrap.

This is a **pointer layer**. Canonical work lives in GitHub issues and PRs on
`MythologIQ-Labs-LLC/GG-CORE`; rows here reference that work and never duplicate
it. Priority and status reflect observable state at reconstruction time.

## Legend

- **Priority**: P1 (blocks a clean gate / release), P2 (planned enhancement),
  P3 (opportunistic).
- **Status**: open, in-review, in-progress.

## Engineering backlog (canonical: GitHub issues)

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-01 | clippy `-D warnings` fails on Linux/macOS: dead code + lints in `sandbox/unix.rs` | issue #54 | P1 | in-progress | Fix landed on `chore/hardening-ci-sandbox-lints` (`7a00233`); verify via CI after operator push |
| B-17 | Pre-existing default-feature test failures: gguf validate_path ×2, kv_cache multi-sequence | issue #55 | P1 | open | Blocks green `cargo test --workspace`; fix `validate_path` surface coherently with B-19 |
| B-18 | 13 residual clippy errors on current stable toolchain (post-#47) | issue #56 | P1 | open | Blocks clippy CI leg on all OSes; mechanical fixes across 8 files |
| B-19 | security: `validate_path()` does not reject NUL bytes (FFI truncation class) | issue #57 | P1 | open | Scoped into cycle-2 rev.2 plan (Phase 1) |
| B-20 | security: KV cache cross-sequence data leakage — PageTable is single-sequence (global position-keyed) | issue #58 | P1 | done | Fixed: PageTable redesigned as pure pool; per-sequence page_ids lookup; remanence zeroed; lock order enforced. 15/15 kv_cache tests pass (2 new isolation oracles). F-21 → verified. |
| B-21 | ADR-007 epic: TierSynergy consolidation + DSpark adaptive speculative decoding | issues #60–68, PR #59 | P2 | open | Forward-looking design epic. Decomposition + per-issue briefs: `docs/plan-adr007-epic-execution.md`. One issue per governed cycle |
| B-22 | `cargo clippy --all-targets`: non-exhaustive `FinishReason::Cancelled` match (test + bench) + 1 field-assign | issue #69 | P1 | done | Fixed `626f034`: added `Cancelled` arm to bench match; updated gguf test vec+count (4→5) |
| B-23 | Seal runtime-hardening cycle 2 (session 2026-07-08T1651-6c68b6) | this session | P2 | open | Code+governance committed (`43cc89c`/`bc0c70c`); needs `/qor-substantiate` Entry #85. Steps: `docs/runbook-merge-integration-sequence.md` §4 |
| B-24 | Streaming egress sanitization: in-runtime detokenization + IPC protocol decision (streaming channel carries u32 token IDs; also rejection frame indistinguishable from completion) | plan-security-chain-wiring-2026-07-25 audit advisory | P2 | open | Decide detokenize-in-runtime vs client-side contract; then plan under its own gate |
| B-25 | Consumable FFI/Python surface — CI foundation: add `features` matrix legs (gguf/onnx/ffi/python) to rust.yml (clippy `-D warnings` + build + test per feature), make all four optional features clippy-clean, Razor-extract `ffi/inference.rs` (272→246; new `ffi/inference_result.rs`), fix stale gguf `e2e_model_test.rs` | audit Entry #101/#102, Shadow Genome #7 | P1 | done | Done (session 2026-07-26T0030-b25ffi, ledger Entry #105 PASS): 4 features clippy-clean under `-D warnings --all-targets`; `features` matrix job added to `.github/workflows/rust.yml`. Reroute half tracked as B-25b |
| B-25b | FFI/Python inference reroute through `Runtime::infer/infer_stream` (fix the enqueue-then-await-no-worker deadlock; map `SecurityRejected`; gguf-gate streaming) | research brief 2026-07-26 (Entry #104) | P1 | done | Done (session 2026-07-26T1850-b25b, ledger Entry #107 L3 audit PASS): 5 entry points (`core_infer`/`core_infer_bounded`/`core_infer_streaming`/Python `Session::infer`/`AsyncSession::infer`) rerouted through `Runtime::infer`; enqueue-then-await-no-worker deadlock fixed; consumable surfaces now security-enforced (ingress injection scan + egress PII sanitize); ffi acceptance test un-ignored + injection→`SecurityRejected` test added; verified ffi/python/default clippy + tests green. Real per-token FFI streaming still pending (one-callback full-output today) — ties to B-24 |
| B-26 | [→ COREFORGE] switch gg_core_runtime.rs::infer_with_model from inference_engine.run() to runtime.infer() once the façade ships, so the embedded surface is security-enforced | this session's consumer×security investigation | P1 | open | COREFORGE workspace change (filed to Personal-Task-Management) |
| B-27 | ONNX classifier scope-1: replace fail-loud `OnnxClassifier` stub with real candle-onnx inference (tokenize → `candle_onnx::simple_eval` → deterministic logits selection → `logits_to_classification` softmax/argmax → `ClassificationResult`); mirrors the working `OnnxEmbedder`; `load_onnx_classifier` added to `onnx/mod.rs` | issue #72 | P2 | done | Done (session 2026-07-26T1930-onnxcls, ledger Entry #109 L2 audit PASS): 3 CI-runnable pure unit tests (synthetic logits, no fixture) + 1 fixture-gated e2e that skips when the model is absent/invalid; F-13 test binding re-pointed to `classifier.rs`. Two explicit follow-ups tracked as B-28 (real tokenizer) and B-29 (registry auto-dispatch) |
| B-28 | ONNX classifier scope-2: replace naive `simple_tokenize` with a real WordPiece/subword tokenizer (`tokenizers` crate) for the ONNX classify/embed paths | issue #72 (follow-up) | P2 | open | Out of scope for #72 scope-1; plan under its own gate |
| B-29 | ONNX registry auto-dispatch: manifest-driven embedder-vs-classifier selection (today `load_onnx_classifier` exists but selection is not manifest-driven) | issue #72 (follow-up) | P2 | open | Add manifest-driven backend selection over the ONNX loaders |
| B-02 | ADR: Backend Capability Contract & BitNet-compatible runtime adapter | issue #48 | P2 | open | Author ADR; parent of B-03..B-06 |
| B-03 | Implement `RuntimeBackendCapabilities` schema | issue #49 | P2 | open | Define schema after ADR #48 lands |
| B-04 | Add hardware profile & backend selection policy | issue #50 | P2 | open | Design policy over K8s hardware profiles (F-44) |
| B-05 | Create experimental BitNet backend adapter wrapper | issue #51 | P3 | open | Prototype behind an experimental feature flag |
| B-06 | Build benchmark harness for backend perf & wrapper overhead | issue #52 | P2 | open | Extend existing `core-runtime/benches/` (F-47) |
| B-07 | Define degraded-mode policy for constrained local inference | issue #53 | P2 | open | Specify governance + runtime behavior under resource pressure |
| B-15 | Add Rust CI workflow (fmt --check, clippy -D warnings, cargo test; ubuntu/macos/windows matrix) | research brief 2026-07-08 | P1 | in-progress | Landed on `chore/hardening-ci-sandbox-lints` (`7a00233`); goes green only after PR #47 + B-17/B-18/B-19 + fmt sweep merge |
| B-16 | `sandbox/unix.rs` exceeds Section 4 Razor (523 lines > 250) — pre-existing debt | audit 2026-07-08 (R2) | P3 | open | Future `/qor-refactor` under its own L3 audit; out of scope for lint-only cycle |

## In-flight delivery

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-08 | Cfg-gate advanced-feature tests + clippy cleanup (COREFORGE deep-audit) | PR #47 (branch tip `b661403` on current origin/main) | P1 | in-review | Mergeable, CodeQL green. Research 2026-07-08: does NOT overlap B-01/#54 (disjoint file surfaces) — merge first |
| B-09 | Local `main` diverged from origin (ahead 1: `5d0e5a5`; behind 1: `575d703`) + 6-commit worktree branch `claude/affectionate-edison-7e6b8a` (shim/TierSynergy refactors) | local git graph | P2 | in-progress | Rebase onto origin/main; operator decides worktree-branch fate |
| B-10 | Uncommitted 193-file repo-wide `cargo fmt` sweep (+4079/−2040; `cargo fmt --check` clean) | local git status | P1 | in-progress | Commit as isolated `style:` commit after rebase; run full `cargo test` as semantic check |

## Governance backlog

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-11 | Seed scaffold-owned `docs/ARCHITECTURE_PLAN.md` (absent since bootstrap) | governance-health (Phase 109) | P2 | open | `qor-logic seed` (scaffold-owned; safe to seed) |
| B-12 | Seed scaffold-owned `docs/GOVERNANCE_INDEX.md`; restores Governance Index drift check | governance-health / governance-index (Phase 112/120) | P2 | open | `qor-logic seed`, then re-run `qor-logic governance-index` |
| B-13 | Doc drift: CLAUDE.md cites `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` which does not exist | CLAUDE.md vs `docs/architecture/` | P3 | open | Create the doc or correct the reference |
| B-14 | Deep-verify `verified` rows in `docs/FEATURE_INDEX.md` per SG-035 | docs/FEATURE_INDEX.md | P3 | open | Operator confirms each test truly exercises its feature |

## Notes

- ~~B-01 and B-08 overlap on the clippy cleanup~~ **Corrected by research
  2026-07-08** (`docs/research-brief-runtime-optimization-hardening-2026-07-08.md`):
  PR #47's branch tip `b661403` is the exact commit where COREFORGE observed the
  #54 lints — the surfaces are disjoint. Merge #47 first, then fix #54 separately.
- **B-15 (new, P1)**: no Rust CI exists (`.github/workflows/` = CodeQL only) —
  fmt/clippy/test workflow required before any hardening evidence is obtainable.
- Governance items B-11/B-12 are the remaining findings from the same
  governance-health run that produced this artifact; they are seed-repairable
  (unlike this file and `docs/FEATURE_INDEX.md`, which required reconstruction).
