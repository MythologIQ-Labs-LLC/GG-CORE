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
| B-01 | clippy `-D warnings` fails on Linux/macOS: dead code + lints in `sandbox/unix.rs` | issue #54 | P1 | done | RESOLVED (reconciliation 2026-07-27, brief `research-brief-backlog-reconciliation-2026-07-27.md`): clippy `-D warnings` green on all 3 OS legs (#78/#79/#80); issue #54 already closed |
| B-17 | Pre-existing default-feature test failures: gguf validate_path ×2, kv_cache multi-sequence | issue #55 | P1 | done | RESOLVED (reconciliation 2026-07-27): `cargo test --lib validate_path`→4 passed, `--lib kv_cache`→7 passed; issue #55 close-ready |
| B-18 | 13 residual clippy errors on current stable toolchain (post-#47) | issue #56 | P1 | done | RESOLVED (reconciliation 2026-07-27): clippy `-D warnings` green (default + gguf/onnx/ffi/python); issue #56 close-ready |
| B-19 | security: `validate_path()` does not reject NUL bytes (FFI truncation class) | issue #57 | P1 | done | RESOLVED (reconciliation 2026-07-27): `models/loader.rs:80` rejects `'\0'` with a fixed `<nul-byte rejected>` sentinel; issue #57 close-ready |
| B-20 | security: KV cache cross-sequence data leakage — PageTable is single-sequence (global position-keyed) | issue #58 | P1 | done | Fixed: PageTable redesigned as pure pool; per-sequence page_ids lookup; remanence zeroed; lock order enforced. 15/15 kv_cache tests pass (2 new isolation oracles). F-21 → verified. |
| B-21 | ADR-007 epic: TierSynergy consolidation + DSpark adaptive speculative decoding | issues #60–68, PR #59 | P2 | open | Forward-looking design epic. Decomposition + per-issue briefs: `docs/plan-adr007-epic-execution.md`. One issue per governed cycle |
| B-22 | `cargo clippy --all-targets`: non-exhaustive `FinishReason::Cancelled` match (test + bench) + 1 field-assign | issue #69 | P1 | done | Fixed `626f034`: added `Cancelled` arm to bench match; updated gguf test vec+count (4→5) |
| B-23 | Seal runtime-hardening cycle 2 (session 2026-07-08T1651-6c68b6) | this session | P3 | done | SUPERSEDED (reconciliation 2026-07-27): that cycle's code is in green `main`; the Merkle ledger has advanced far past the referenced Entry #85 (now Entry #115). No dangling seal blocks the chain (`verify-ledger` clean) |
| B-24 | Streaming egress sanitization: in-runtime detokenization + IPC protocol decision (streaming channel carries u32 token IDs; also rejection frame indistinguishable from completion) | plan-security-chain-wiring-2026-07-25 audit advisory | P2 | decided | DECIDED (ledger Entry #117, brief `research-brief-b24-streaming-egress-2026-07-27.md`): **detokenize-in-runtime** (client-side contract rejected as a permanent egress sanitization bypass). Split into B-24a + B-24b; B-28 resequenced before B-24b |
| B-24a | Typed stream terminal: replace implicit `is_final`/sender-drop with `Complete\|Rejected\|Error` across `TokenStream` + IPC streaming handler (fixes F2: completion indistinguishable from rejection/error) | B-24 decision (Entry #117) | P2 | done | Done (session 2026-07-27T-b24-streaming, ledger Entry #118 L2 audit PASS): `StreamItem::{Token,End}` + `StreamTerminal::{Complete,Rejected,Error}`; `run_stream_sync` centralizes terminal emission; `send(0,true)` error-faking removed; `relay_stream` maps terminals to `StreamChunk::complete`/`error`. Scope tightened at audit: FFI/Python excluded (not `TokenStream` consumers, already error-aware). fmt + clippy `-D warnings` (default+gguf+onnx/ffi/python) + 554 lib tests (3 new) + integration green |
| B-24b | Streaming egress sanitization: in-runtime detokenization + streaming-safe windowed egress sanitizer with holdback; stream sanitized text (fixes F1: streaming PII bypass) | B-24 decision (Entry #117) | P2 | done | Done (session 2026-07-27T-b24b, ledger Entry #121 RESEARCH + Entry #122 SEAL, L3 audit PASS): new `security/stream_sanitizer.rs` `StreamSanitizer` (re-sanitize full buffer, release prefix ≥128 chars behind end + alnum-run guard, flush on terminal); GGUF loop detokenizes + drives it, emits new `StreamItem::Text` (raw tokens never leave); `run_stream_sync`/facade/worker thread `Option<&SecurityPipeline>`; `relay_stream` maps Text→`StreamChunk`. 4 adversarial tests (multi-word PII split, UTF-8 multibyte, terminal flush, passthrough) + integration green; clippy `-D warnings` all legs. **B-24 (F1+F2) fully closed** |
| B-25 | Consumable FFI/Python surface — CI foundation: add `features` matrix legs (gguf/onnx/ffi/python) to rust.yml (clippy `-D warnings` + build + test per feature), make all four optional features clippy-clean, Razor-extract `ffi/inference.rs` (272→246; new `ffi/inference_result.rs`), fix stale gguf `e2e_model_test.rs` | audit Entry #101/#102, Shadow Genome #7 | P1 | done | Done (session 2026-07-26T0030-b25ffi, ledger Entry #105 PASS): 4 features clippy-clean under `-D warnings --all-targets`; `features` matrix job added to `.github/workflows/rust.yml`. Reroute half tracked as B-25b |
| B-25b | FFI/Python inference reroute through `Runtime::infer/infer_stream` (fix the enqueue-then-await-no-worker deadlock; map `SecurityRejected`; gguf-gate streaming) | research brief 2026-07-26 (Entry #104) | P1 | done | Done (session 2026-07-26T1850-b25b, ledger Entry #107 L3 audit PASS): 5 entry points (`core_infer`/`core_infer_bounded`/`core_infer_streaming`/Python `Session::infer`/`AsyncSession::infer`) rerouted through `Runtime::infer`; enqueue-then-await-no-worker deadlock fixed; consumable surfaces now security-enforced (ingress injection scan + egress PII sanitize); ffi acceptance test un-ignored + injection→`SecurityRejected` test added; verified ffi/python/default clippy + tests green. Real per-token FFI streaming still pending (one-callback full-output today) — ties to B-24 |
| B-26 | [→ COREFORGE] switch gg_core_runtime.rs::infer_with_model from inference_engine.run() to runtime.infer() once the façade ships, so the embedded surface is security-enforced | this session's consumer×security investigation | P1 | open | COREFORGE workspace change (filed to Personal-Task-Management) |
| B-27 | ONNX classifier scope-1: replace fail-loud `OnnxClassifier` stub with real candle-onnx inference (tokenize → `candle_onnx::simple_eval` → deterministic logits selection → `logits_to_classification` softmax/argmax → `ClassificationResult`); mirrors the working `OnnxEmbedder`; `load_onnx_classifier` added to `onnx/mod.rs` | issue #72 | P2 | done | Done (session 2026-07-26T1930-onnxcls, ledger Entry #109 L2 audit PASS): 3 CI-runnable pure unit tests (synthetic logits, no fixture) + 1 fixture-gated e2e that skips when the model is absent/invalid; F-13 test binding re-pointed to `classifier.rs`. Two explicit follow-ups tracked as B-28 (real tokenizer) and B-29 (registry auto-dispatch) |
| B-28 | ONNX classifier scope-2: replace naive `simple_tokenize` with a real WordPiece/subword tokenizer (`tokenizers` crate) for the ONNX classify/embed paths | issue #72 (follow-up) | P2 | done | Done (session 2026-07-27T-b28-tokenizer, ledger Entry #119 RESEARCH + Entry #120 SEAL, L2 audit PASS): new `engine/onnx/tokenizer.rs` `OnnxTokenizer::{WordPiece,HashFallback}`; `for_model` loads a sibling `tokenizer.json` offline via `Tokenizer::from_file`, degrades to a named hash fallback (warn) when absent; embedder+classifier use it; hash stub deleted. Dep `tokenizers 0.21` `default-features=false, fancy-regex` (pure-Rust, `http` off) — offline verified empirically (no reqwest/hyper/hf-hub in tree). 562 onnx tests incl. offline WordPiece round-trip; clippy `-D warnings` (all legs) + fmt green |
| B-32 | Flaky test: `cli::tests::test_get_socket_path_{default,from_env}` race on the shared `GG_CORE_SOCKET_PATH` env var under Rust's parallel in-process runner (observed on macOS CI leg, PR #82) | reconciliation follow-up (B-24a CI) | P3 | done | Done (folded into B-28 cycle, Entry #120): the two env tests serialized behind a module `static ENV_LOCK: Mutex<()>` |
| B-29a | ONNX manifest-driven loader dispatch: pure `plan_onnx_load` (capability→loader, fail-loud on non-onnx/ambiguous/missing-labels) + `load_onnx_from_manifest` wrapper + manifest `labels` field | issue #72 (follow-up) | P2 | done | Done (session 2026-07-28T-b29-onnx-dispatch, ledger Entry #125 L2 audit PASS after iter1 razor VETO #124): new `engine/onnx/dispatch.rs` (+sibling `dispatch_tests.rs`, 10 tests), `ModelManifest.labels: Option<Vec<String>>` (`#[serde(default)]`). Internal seam — no production caller yet; end-to-end serving requires B-29b |
| B-29b-1 | Unified `Model` trait + registry migration: new `engine/model.rs` `Model` (GgufModel superset, minus dead `set_device_placement`); registry/lifecycle/loaders/impls migrated `Arc<dyn GgufModel>`→`Arc<dyn Model>`; `GgufModel`/`OnnxModel` traits deleted; ONNX impls gain `as_any` | issue #72 (follow-up) | P2 | done | Done (session 2026-07-28T-b29b-registry-unification, ledger Entry #132 L3 audit PASS after iter1 infra-mismatch VETO #129): behavior-preserving, 14-file migration; `inference.rs` streaming extracted to `inference_streaming.rs` (271→212, Razor); 3 new tests (registry neutrality, stream-unsupported, onnx as_any). GGUF still only wired backend; ONNX registerable but unreached |
| B-29b-2 | Manifest loading + architecture dispatch: shared `models/backend_dispatch.rs` (`choose_backend` + `load_model_dispatch`) resolves an optional sibling `manifest.json` and routes GGUF/ONNX; wired into both prod load sites | issue #72 (follow-up) | P2 | done | Done (session 2026-07-28T-b29b2-manifest-dispatch, ledger Entry #136 L2 audit PASS): `ffi/models.rs`+`python/session.rs` now call the dispatcher; ONNX servable end-to-end when a sibling `manifest.json` declares `architecture: onnx`; manifest optional (defaults GGUF — existing loads unchanged). Closes issue #72 scope-3. FEATURE_INDEX F-57 |
| B-30 | Dependabot: migrate `pyo3` 0.21→0.29 + swap `pyo3-asyncio-0-21`→`pyo3-async-runtimes` 0.29 to clear RUSTSEC-2026-0176 (high, PyList/PyTuple iterator OOB read), RUSTSEC-2026-0177 (medium, PyCFunction Sync), RUSTSEC-2025-0020 (low, PyString buffer overflow) | Dependabot / RustSec | P1 | done | Done (session 2026-07-26T2010-pyo3, ledger Entry #111 L2 audit PASS): Cargo.toml bumped pyo3 0.29 + pyo3-async-runtimes 0.29; async call-site `pyo3_asyncio_0_21::`→`pyo3_async_runtimes::`; `#[pyclass(from_py_object)]` on arg-extracted `InferenceParams`, `#[pyclass(skip_from_py_object)]` on return-only pyclasses; `Option<PyObject>`→`Option<Py<PyAny>>` in `__exit__`/`__aexit__`. All three advisories cleared; python clippy `-D warnings` + python binding tests + default clippy/test/fmt green. Remaining Dependabot item `rand` 0.8→0.9 (low, crypto-code migration) tracked as B-31 |
| B-31 | Dependabot: migrate `rand` 0.8→0.9 (low; touches cryptographic RNG in `security/`) | Dependabot / RustSec | P1 | done | Done (session 2026-07-27T-rand09, ledger Entry #113 RESEARCH + Entry #114 SEAL, L3 audit PASS): `rand = "0.9"`; the 7 `OsRng.fill_bytes` sites migrated to `use rand::{RngCore, TryRngCore}; OsRng.unwrap_err().fill_bytes(..)` (rand_core 0.9 demoted `OsRng` to `TryRngCore`-only); `thread_rng().gen_range`→`rng().random_range` in `bucket.rs`; `rand::random()` sites unchanged. CSPRNG + panic-on-entropy-failure semantics preserved (verified against vendored `rand_core-0.9.3`). fmt + clippy `-D warnings` (default + gguf/onnx/ffi/python) + 551 lib tests + integration security tests green. Our rand runtime tree collapsed to 0.9 (rand 0.9.2 / rand_chacha 0.9.0 / rand_core 0.9.5); residual `rand_core 0.6.4` is held by the RustCrypto `crypto-common` stack, independent of our dep and out of scope |
| B-02 | ADR: Backend Capability Contract & BitNet-compatible runtime adapter | issue #48 | P2 | open | Author ADR; parent of B-03..B-06 |
| B-03 | Implement `RuntimeBackendCapabilities` schema | issue #49 | P2 | open | Define schema after ADR #48 lands |
| B-04 | Add hardware profile & backend selection policy | issue #50 | P2 | open | Design policy over K8s hardware profiles (F-44) |
| B-05 | Create experimental BitNet backend adapter wrapper | issue #51 | P3 | open | Prototype behind an experimental feature flag |
| B-06 | Build benchmark harness for backend perf & wrapper overhead | issue #52 | P2 | open | Extend existing `core-runtime/benches/` (F-47) |
| B-07 | Define degraded-mode policy for constrained local inference | issue #53 | P2 | open | Specify governance + runtime behavior under resource pressure |
| B-15 | Add Rust CI workflow (fmt --check, clippy -D warnings, cargo test; ubuntu/macos/windows matrix) | research brief 2026-07-08 | P1 | done | RESOLVED (reconciliation 2026-07-27): `.github/workflows/rust.yml` = fmt+clippy+test ×3 OS + `features` (gguf/onnx/ffi/python) matrix; all 11 legs green on #78/#79/#80 |
| B-16 | `sandbox/unix.rs` exceeds Section 4 Razor (523 lines > 250) — pre-existing debt | audit 2026-07-08 (R2) | P3 | open | Future `/qor-refactor` under its own L3 audit; out of scope for lint-only cycle |

## In-flight delivery

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-08 | Cfg-gate advanced-feature tests + clippy cleanup (COREFORGE deep-audit) | PR #47 | P1 | needs-decision | RECONCILE 2026-07-27: PR #47 still OPEN but `updatedAt` 2026-07-08, 25 files, `mergeStateStatus` UNKNOWN; edits surfaces main has since rewritten (`bucket.rs`, `output_sanitizer.rs`, `pii_patterns.rs`) and its clippy-cleanup goal is already met on green main. Recommend **close as superseded** unless a diff surfaces unique content. Operator decision |
| B-09 | Local `main` diverged from origin + 6-commit worktree branch `claude/affectionate-edison-7e6b8a` (shim/TierSynergy refactors) | local git graph | P3 | done | RESOLVED (reconciliation 2026-07-27): local `main` == `origin/main` (synced through #80); divergence gone. Worktree-branch fate remains an operator housekeeping call (not blocking) |
| B-10 | Uncommitted 193-file repo-wide `cargo fmt` sweep (+4079/−2040; `cargo fmt --check` clean) | local git status | P1 | done | RESOLVED (reconciliation 2026-07-27): sweep no longer present; working tree clean (`fmt --check` green in CI); superseded by merged history |

## Governance backlog

| ID | Item | Canonical source | Priority | Status | Next action |
|----|------|------------------|----------|--------|-------------|
| B-11 | Seed scaffold-owned `docs/ARCHITECTURE_PLAN.md` (absent since bootstrap) | governance-health (Phase 109) | P2 | done | RESOLVED (reconciliation 2026-07-27): `governance-health --profile skill-entry` reports `OK docs/ARCHITECTURE_PLAN.md` |
| B-12 | Seed scaffold-owned `docs/GOVERNANCE_INDEX.md`; restores Governance Index drift check | governance-health / governance-index (Phase 112/120) | P2 | done | RESOLVED (reconciliation 2026-07-27): `governance-health` reports `OK docs/GOVERNANCE_INDEX.md` |
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

### Reconciliation 2026-07-27 (brief `research-brief-backlog-reconciliation-2026-07-27.md`, ledger Entry #116)

The 2026-07-08 rows were re-verified against green `main` @ v0.8.2. Dispositions:

- **Resolved → done**: B-01, B-10, B-11, B-12, B-15, B-17, B-18, B-19 (evidence in
  the reconciliation brief). GitHub issues **#55, #56, #57, #69** are close-ready
  (held for operator approval per the Review Boundary).
- **Superseded → done**: B-09 (main synced), B-23 (ledger past Entry #85; chain clean).
- **Needs operator decision**: PR **#47** (B-08 — recommend close-superseded);
  PR **#74** (Dependabot rand bump in `core-runtime/fuzz` — recommend close);
  PR **#59** (TierSynergy ADR — keep, part of B-21 epic).
- **Phase 1 sequence (one `/qor-auto-dev-1` cycle each, stop at Review Boundary):**
  revised after the B-24 decision (Entry #117) to
  **B-24a → B-28 → B-24b → B-29 → B-07 → B-16**; fold **B-13** (+ B-11/B-12 issue
  closure) into a docs/governance pass.
- **Deferred epics**: B-02..B-06 (#48–52) and B-21 (#59–68) — ADR-first, multi-cycle;
  scheduled separately, not in this sweep.
