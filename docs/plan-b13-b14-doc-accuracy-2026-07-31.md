# Plan: B-13 + B-14 — Documentation & Feature-Index Accuracy

**change_class**: hotfix

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Creates the missing `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` (code-grounded) and
    corrects the two mis-stated `FEATURE_INDEX.md` rows (F-45, F-38). Documentation only.
- non_goals:
  - No Rust source/behavior change; no new feature; no change to the 58 already-correct index rows.
  - Not a from-scratch re-audit of all 60 tests — the tool confirms existence+pass; this fixes the
    2 defects and records the SG-035 deep-verify methodology.
- exclusions:
  - No CI/workflow change.

## Open Questions

None. The doc's content is determinable from the code (research F2); the two index defects and
their fixes are identified (F3/F4).

## Phase 1: Create `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` (B-13)

### Affected Files

- `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` (NEW) — a code-grounded technical spec so the
  `CLAUDE.md:92` and `TANDEM_EXPERIMENTS_PROPOSAL.md` references resolve. Sections:
  - **Purpose & C.O.R.E.** — Contained / Offline / Restricted / Execution; place in the COREFORGE
    stack; triage philosophy (from `docs/CONCEPT.md`, `CLAUDE.md`).
  - **Security boundaries** — process/filesystem/network/IPC table (from `CLAUDE.md`); the
    forbidden-modules / forbidden-deps invariants.
  - **Module map** — the `core-runtime/src/{ipc,scheduler,engine,models,memory,telemetry,security,
    sandbox,ffi,python,cli}` tree with one-line responsibilities + key file references (verified
    against the current tree).
  - **The secure inference path** — `Runtime::infer`/`infer_stream` (`runtime_facade.rs`) as the
    SOLE external entry: ingress `SecurityPipeline::scan_prompt` → engine → egress
    `sanitize_output` (+ streaming sanitizer); `InferenceEngine::run*` is `pub(crate)` (B-33).
  - **Model dispatch** — `models/backend_dispatch.rs` `load_model_dispatch` selecting GGUF vs ONNX
    by sibling `manifest.json`; the unified `engine::Model` trait.
  - **Scheduler / memory** — `RequestQueue` (priority `BinaryHeap` + async Mutex/Notify),
    `MemoryPool`, KV/prompt cache.
  - **Consumable-dependency shape** — rlib + cdylib (C-ABI, `include/gg_core.h`) + PyO3.
  - A short header noting it is a living overview that cross-references `ARCHITECTURE_PLAN.md`
    (file-tree contract) and the ADRs.

### Unit Tests

No unit test (documentation). Verification: the file exists at the cited path; a link/grep check
confirms `CLAUDE.md:92` and the `TANDEM_EXPERIMENTS_PROPOSAL.md` references now resolve to a real
file; every module/file path the doc cites exists in the tree (grep-checked).

## Phase 2: Correct the two mis-stated FEATURE_INDEX rows (B-14)

### Affected Files

- `docs/FEATURE_INDEX.md`:
  - **F-45** (Veritas shim): change test path from `n/a` to the real inline tests
    (`core-runtime/src/shim/rate_limiter.rs; core-runtime/src/shim/service_tier.rs;
    core-runtime/src/shim/mod.rs` — 14 unit tests) and status `unverified` → `verified`.
  - **F-38** (sandbox isolation): status `unverified` → `verified`, with the description noting the
    sandbox path is unix-gated (CI-verified via `sandbox_test.rs`; not buildable on the Windows dev
    host) — the test is functional (asserts `SandboxConfig::default()` invariants), not presence-only.

### Unit Tests

No unit test (index correction). Verification: `qor-logic scripts feature_index_verify --repo-root
.` re-run — F-45 now resolves to existing, passing tests; F-38 reflects its CI-gated reality; the
SG-035 deep-verify (citation reconciled against reality; tests exercise, not merely assert presence)
is recorded in the seal.

## Feature Inventory Touches

Empty — justified. This CORRECTS existing FEATURE_INDEX rows (F-45/F-38 metadata) and adds an
architecture doc; it introduces/modifies no user-touchable runtime feature.

## Definition of Done

### Deliverable: accurate architecture doc + corrected feature index

- **D1**: `CLAUDE.md`'s "Full technical specification" reference resolves to a real, code-accurate
  `CORE_RUNTIME_ARCHITECTURE.md`; every FEATURE_INDEX row's status + test citation matches reality
  (F-45 tested, F-38 CI-verified).
- **D2**: NEW `docs/architecture/CORE_RUNTIME_ARCHITECTURE.md`; `FEATURE_INDEX.md` F-45/F-38 rows
  corrected.
- **D3**: META_LEDGER entries (canonical markup) research #177, plan, audit, seal; BACKLOG B-13 +
  B-14 → done.
- **D4**: `test -f docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` succeeds; `grep -c
  CORE_RUNTIME_ARCHITECTURE docs/architecture/CORE_RUNTIME_ARCHITECTURE.md` (self-consistent);
  `qor-logic scripts feature_index_verify --repo-root .` shows no row mis-citing a `n/a`/absent test
  for a tested feature (F-45 resolved); the seal records the deep-verify.

## CI Commands

- `qor-logic scripts feature_index_verify --repo-root .` — the corrected index resolves
- `python -c "import pathlib; assert pathlib.Path('docs/architecture/CORE_RUNTIME_ARCHITECTURE.md').exists()"` — the new doc exists
