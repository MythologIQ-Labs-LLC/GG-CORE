# Plan: B-25 CI Foundation (feature legs + pre-existing defect cleanup)

**change_class**: feature
**doc_tier**: standard
**iteration**: 1
**risk_grade**: L2 (mechanical: clippy hygiene + additive CI config + one Razor
extraction; NO behavior/security/logic change. The FFI/Python inference reroute
is a SEPARATE deferred L3 cycle.)
**high_risk_target**: false
**originating_research**: docs/research-brief-b25-ci-legs-ffi-python-2026-07-26.md
(ledger Entry #104) — every optional-feature surface carries clippy debt
invisible to the default-only CI.

**terms_introduced**: none new.

**boundaries**:
- limitations:
  - NO inference-behavior change. This cycle does NOT reroute FFI/Python through
    the façade and does NOT fix the deadlock — that is the deferred L3 reroute
    cycle (still tracked as the second half of B-25). This cycle only makes the
    feature surfaces build clippy-clean and adds the CI legs that verify them.
  - Clippy fixes MUST be derived from captured `cargo clippy --features <f>
    --all-targets` output, never guessed (Shadow Genome #3). Each fix is
    semantics-preserving (add `# Safety` docs; mark a fn `unsafe` or restructure;
    remove needless `return`/cast/closure; use `clamp`; index-free iteration).
- non_goals:
  - No FFI/Python reroute / deadlock fix (deferred L3).
  - No `cuda`/`metal`/`advanced` CI legs (no GPU runners; proprietary).
  - No dependency changes.
- exclusions:
  - No `security/` changes.

## Open Questions

None blocking. Defect inventory measured by compiling each feature (Entry #104):
ffi 18 (17 `missing_safety_doc` + 1 `not_unsafe_ptr_arg_deref` at
`ffi/runtime.rs:31`) + `ffi/inference.rs` 272 lines; onnx 2
(`engine/onnx/embedder.rs:53,101`); python 3; gguf 6.

## Design summary

Make `gguf`/`onnx`/`ffi`/`python` build clippy-clean under `-D warnings`, extract
`ffi/inference.rs` under the 250-line Razor, and add four CI legs to `rust.yml`
so the consumable surfaces become verified ground. Pure hygiene + additive CI
config; the only structural change is the `ffi/inference.rs` module split.

## Locked Decisions

- **LD-1 — Clippy map from captured output (Shadow Genome #3).** For each
  feature the implementer runs `cargo clippy --features <f> --all-targets`,
  pairs each `error:`/`warning:` with its `-->` location, and applies the
  machine suggestion (or a semantics-preserving equivalent). Verified per feature
  by a clean `-D warnings` re-run. Grep-evidence of current failures:
  `cargo clippy --features ffi --all-targets` → 18; `--features onnx` → 2 at
  `engine/onnx/embedder.rs:53,101`; `--features python` → 3; `--features gguf`
  → 6.
- **LD-2 — `missing_safety_doc` fix = add `# Safety` sections, don't suppress.**
  The ~17 unsafe `extern "C"` fns (ffi/auth.rs, health.rs, inference.rs,
  models.rs, streaming.rs) get a `/// # Safety` doc paragraph stating the
  pointer-validity contract. `ffi/runtime.rs:31` (`not_unsafe_ptr_arg_deref`)
  is marked `unsafe extern "C"` (or its raw-pointer deref restructured) to match
  the rest of the surface. No `#[allow]` blanket suppression.
- **LD-3 — Extract `ffi/inference.rs` (272 → ≤250).** Move the result-writing
  helpers (`write_inference_result` and the bounded-buffer copy path) into a new
  `core-runtime/src/ffi/inference_result.rs` (`pub(crate)`), leaving the three
  entry points (`core_infer`, `core_infer_bounded`, `core_infer_streaming` stays
  in streaming.rs) in `inference.rs`. Behavior identical. Both files ≤250.
- **LD-4 — Four additive CI legs in `rust.yml`.** A new `features` job (matrix
  over `[gguf, onnx, ffi, python]`) running `cargo clippy --features <f>
  --all-targets -- -D warnings` + `cargo test --features <f>` (ffi/python:
  a build step + the cfg-gated binding test where present). gguf/onnx/python on
  `ubuntu-latest` (toolchain + Python headers available on the runner); ffi
  cross-OS is optional — start ubuntu-only to bound CI time. Additive: the
  existing `lint` + `test` (default) jobs are unchanged.

## Phase 1: Clippy-clean the feature surfaces

### Affected Files (derived from clippy output; exact set confirmed at implement)

- `core-runtime/src/ffi/auth.rs`, `health.rs`, `inference.rs`, `models.rs`,
  `streaming.rs` — add `# Safety` doc sections to the flagged unsafe fns
- `core-runtime/src/ffi/runtime.rs` — fix `not_unsafe_ptr_arg_deref` at :31
- `core-runtime/src/engine/onnx/embedder.rs` — needless `return` (:53),
  redundant full-range slice (:101)
- `core-runtime/src/python/` (session.rs / runtime.rs — per clippy) —
  field-assign-after-Default + 2 redundant closures
- `core-runtime/src/engine/gguf/` (per clippy) — `pos` loop-counter ×3
  (use `.enumerate()` / iterator), `f32`→`f32` cast, clamp-like pattern,
  needless `return`

### Changes

Semantics-preserving lint fixes only. No control-flow or value changes.

### Tests

- No new unit tests — these are lint fixes. Correctness is proven by the
  per-feature `-D warnings` clippy passing AND the pre-existing feature tests
  (e.g. `python_binding_test.rs`, onnx/gguf tests) continuing to pass under
  their feature.

## Phase 2: Razor extraction of ffi/inference.rs

### Affected Files

- `core-runtime/src/ffi/inference_result.rs` — NEW: `pub(crate)`
  result-writing helpers moved from inference.rs
- `core-runtime/src/ffi/inference.rs` — remove the moved helpers; `use` them;
  ≤250 lines
- `core-runtime/src/ffi/mod.rs` — register `mod inference_result;`

### Changes

Pure move + module wiring; entry-point signatures unchanged. Verified by
`cargo build --features ffi` + the ffi clippy leg.

### Tests

- Existing FFI behavior is unchanged; covered by the ffi build + any FFI tests.

## Phase 3: CI feature legs

### Affected Files

- `.github/workflows/rust.yml` — add the `features` matrix job (LD-4)

### Changes

```yaml
  features:
    name: features / ${{ matrix.feature }}
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        feature: [gguf, onnx, ffi, python]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy }
      - run: cargo clippy --features ${{ matrix.feature }} --all-targets -- -D warnings
        working-directory: core-runtime
      - run: cargo test --features ${{ matrix.feature }}
        working-directory: core-runtime
```

(Exact steps confirmed at implement against the existing job style; python may
need a `setup-python` step if the runner lacks dev headers — add only if the
leg fails without it.)

## Phase 4: Governance

### Affected Files

- `docs/FEATURE_INDEX.md` — after the ffi leg is green, correct F-39 (FFI) and
  flip F-40 (python) per the new CI coverage; add a note that F-15/F-16/F-36/F-37
  feature paths are now CI-built
- `docs/BACKLOG.md` — mark B-25 CI-foundation half done; keep the reroute half
  (B-25b) open as the deferred L3 cycle
- `docs/ARCHITECTURE_PLAN.md` — note the CI now builds gguf/onnx/ffi/python

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| F-39 | MODIFIED | .github/workflows/rust.yml (features/ffi leg) | the ffi clippy (-D warnings) + build steps pass in CI; fails if the FFI surface regresses clippy/compile |
| F-40 | MODIFIED | .github/workflows/rust.yml (features/python leg) | the python feature test step runs the python binding test in CI |

## Definition of Done

### Deliverable: Clippy-clean feature surfaces
- **D1**: gguf/onnx/ffi/python each build clippy-clean under `-D warnings`.
- **D2**: lint fixes across the files above; `ffi/inference.rs` ≤250 via the
  `inference_result.rs` extraction.
- **D3**: FEATURE_INDEX F-39/F-40 corrected; BACKLOG B-25 CI-foundation half
  marked done, reroute half (B-25b) retained.
- **D4**: `cargo clippy --features gguf --all-targets -- -D warnings`,
  `--features onnx`, `--features ffi`, `--features python` each exit 0 locally;
  `cargo test --workspace` still 0 failures.

### Deliverable: CI feature legs
- **D1**: CI builds+lints all four features.
- **D2**: `rust.yml` `features` matrix job.
- **D3**: documented in ARCHITECTURE_PLAN.
- **D4.d**: waiver — CI-run green is observable only after push (Review
  Boundary; operator pushes). Local per-feature clippy/test stand in until then.
  **Follow-up**: verify the legs green on the branch's first CI run.

## CI Commands

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings` (default)
- `cargo test --workspace`
- Per-feature clippy (the new legs), run locally this cycle and in CI after
  push: clippy with `--all-targets -- -D warnings` under each of the gguf, onnx,
  ffi, and python features, plus the test suite under each.
