# Plan: ONNX classifier — implement candle-onnx classification (#72)

**change_class**: feature
**doc_tier**: standard
**iteration**: 1
**risk_grade**: L2 (new inference logic in the ONNX engine backend; pure compute,
no security/auth surface; gated behind the `onnx` feature which CI now builds)
**high_risk_target**: false
**originating_research**: this session's #72 investigation (research gate
artifact 2026-07-26T1930-onnxcls/research-iter1.json).

**terms_introduced**: none new.

**boundaries**:
- limitations:
  - **Tokenizer scope-1 only.** The classifier reuses the embedder's naive
    `simple_tokenize` (hash-based). A real WordPiece/BPE tokenizer (`tokenizers`
    crate) is scope-2, out of scope — noted as a follow-up. This means
    real-model accuracy is limited, but the goal here is: fail-loud stub →
    working candle-onnx load + inference producing a well-formed
    `ClassificationResult`.
  - No registry auto-dispatch (embedder-vs-classifier by manifest). This cycle
    adds a `load_onnx_classifier` load path so the classifier is constructible/
    loadable; manifest-driven auto-selection is a follow-up.
- non_goals:
  - No `tokenizers`-crate integration; no security/scheduler/engine-core changes.
  - No change to the working `OnnxEmbedder`.
- exclusions:
  - Fixtures (fixtures/models/onnx/*.onnx) stay gitignored; end-to-end classify
    test skips when absent (per the established CI convention).

## Open Questions

None blocking. Contract verified: `OnnxClassifier` stub at classifier.rs:12-42;
`ClassificationResult{label,confidence,all_labels}` at engine/output.rs:14-23;
`candle_onnx::simple_eval` + `build_transformer_inputs` template at
embedder.rs:64-116; softmax reference at moe/router.rs:69-75.

## Design summary

Replace the `OnnxClassifier` fail-loud stub with real candle-onnx inference,
mirroring the working `OnnxEmbedder`. The classifier tokenizes, runs
`simple_eval`, extracts the logits tensor `[1, num_labels]`, applies softmax +
argmax, and builds a `ClassificationResult`. The logits→result conversion is a
**pure helper** (`logits_to_classification`) so it is unit-testable in CI without
a model fixture. A `load_onnx_classifier` load path makes the classifier
loadable (mirrors `load_onnx_model`).

## Locked Decisions

- **LD-1 — Struct holds a real model, mirroring the embedder.** Replace
  `_model: Option<()>` (classifier.rs:19) with
  `#[cfg(feature="onnx")] model: Option<candle_onnx::onnx::ModelProto>`; add
  `#[cfg(feature="onnx")] pub fn with_model(model_id, labels, model) -> Self`
  (mirrors embedder.rs:34-47). `labels: Vec<String>` loses `#[allow(dead_code)]`
  (now used to build `all_labels`).
- **LD-2 — Real inference, mirroring `embed_text_onnx`.** `classify_text` (the
  stub at classifier.rs:34-42) becomes: `model.as_ref().ok_or(ModelError)` →
  `simple_tokenize` → `build_transformer_inputs` → `candle_onnx::simple_eval`
  → take the output tensor → `logits_to_classification(&tensor, &self.labels)`.
  Reuse the embedder's `build_transformer_inputs`/`simple_tokenize` by promoting
  them to `pub(super)` in embedder.rs (or a shared `onnx/common.rs`); pick the
  smaller diff at implement (promote in embedder.rs).
- **LD-3 — Pure, CI-testable post-processing.** New
  `#[cfg(feature="onnx")] fn logits_to_classification(logits: &Tensor, labels:
  &[String]) -> Result<ClassificationResult, InferenceError>`: flatten logits to
  `Vec<f32>` (squeeze batch dim); softmax (numerically stable, per
  moe/router.rs:69-75, temperature 1.0); argmax for the top index; build
  `all_labels` as `(label, prob)` sorted desc; `label`/`confidence` from the top.
  Error if `labels.len() != logits.len()` (`ModelError` — mismatched label set).
  This function takes a tensor+labels and is testable with a synthetic tensor —
  **no model fixture needed** → real CI coverage of the classification logic.
- **LD-4 — `load_onnx_classifier` load path.** Add
  `#[cfg(feature="onnx")] pub fn load_onnx_classifier(path, model_id, labels,
  _config) -> Result<Arc<dyn OnnxModel>, InferenceError>` in onnx/mod.rs mirroring
  `load_onnx_model` (candle_onnx::read_file → `OnnxClassifier::with_model`). The
  existing `load_onnx_model` (embedder) is unchanged.

## Phase 1: Classifier inference + pure post-processing

### Affected Files

- `core-runtime/src/engine/onnx/classifier.rs` — struct holds `ModelProto`;
  `with_model`; real `classify_text` via `classify_text_onnx`;
  `logits_to_classification` pure helper; `#[cfg(test)]` unit tests
- `core-runtime/src/engine/onnx/embedder.rs` — promote `simple_tokenize` +
  `build_transformer_inputs` to `pub(super)` (shared with the classifier)

### Changes

`classify_text_onnx` (≤40 lines) mirrors `embed_text_onnx`; the logits post-
processing lives in `logits_to_classification` (≤40 lines, pure).

### Unit Tests (in classifier.rs `#[cfg(test)]`)

- `logits_to_classification_picks_argmax` — synthetic logits `[[-1.0, 3.0,
  0.5]]` (Tensor, no model) with labels `["neg","pos","neu"]` → `label=="pos"`,
  `confidence` ≈ softmax max (> other probs), `all_labels` sorted desc with
  `"pos"` first and probabilities summing ~1.0. **CI-runnable (no fixture).**
- `logits_to_classification_rejects_label_mismatch` — logits width 3 with 2
  labels → `Err(ModelError)`.
- `classify_text_without_model_fails` — `OnnxClassifier::new` (no model) →
  `classify_text` returns `Err(ModelError)` (fail-loud preserved).

## Phase 2: Load path + end-to-end test

### Affected Files

- `core-runtime/src/engine/onnx/mod.rs` — add `load_onnx_classifier`
- `core-runtime/src/engine/onnx/classifier.rs` — fixture-gated e2e test

### Changes

`load_onnx_classifier` mirrors `load_onnx_model` (LD-4).

### Tests

- `classifier.rs::load_and_classify` (fixture-gated) — if
  `fixtures/models/onnx/tinybert-classifier.onnx` exists: load via
  `load_onnx_classifier` with 2 labels, classify a short text, assert the result
  is a well-formed `ClassificationResult` (label ∈ labels, confidence ∈ [0,1],
  `all_labels.len()==2`). Skips with an eprintln when absent (CI convention).

## Phase 3: Governance

### Affected Files

- `docs/FEATURE_INDEX.md` — the ONNX-classifier row: flip its status/descriptor
  from stub to implemented (candle-onnx load+infer; logits→ClassificationResult),
  test path `core-runtime/src/engine/onnx/classifier.rs` (unit) +
  tier2_onnx_classification_test.rs
- `docs/BACKLOG.md` — record #72 scope-1 done; note tokenizer (scope-2) + registry
  auto-dispatch as follow-ups
- `docs/ARCHITECTURE_PLAN.md` — engine/onnx note: classifier now implemented
  (was stub); real tokenizer + manifest dispatch pending

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| FX-ONNX-CLS | MODIFIED | core-runtime/src/engine/onnx/classifier.rs | logits_to_classification on synthetic logits returns the argmax label with a valid softmax confidence and sorted all_labels; the classifier is no longer a fail-loud stub. Fails if the classification post-processing is wrong. |

## Definition of Done

### Deliverable: Working ONNX classifier
- **D1**: The ONNX classifier performs real candle-onnx inference producing a
  well-formed `ClassificationResult` (was a fail-loud stub).
- **D2**: `OnnxClassifier::with_model` + real `classify_text` + pure
  `logits_to_classification`; `load_onnx_classifier` in mod.rs; embedder helpers
  shared via `pub(super)`. All fns ≤40 lines; files ≤250.
- **D3**: FEATURE_INDEX onnx-classifier row updated; BACKLOG #72 scope-1 done +
  scope-2 (tokenizer) / dispatch follow-ups; ARCHITECTURE_PLAN note.
- **D4**: `logits_to_classification_picks_argmax` +
  `logits_to_classification_rejects_label_mismatch` +
  `classify_text_without_model_fails` pass under the onnx feature test leg
  (CI-runnable, no fixture); `load_and_classify` passes locally with the fixture
  and skips in CI.

## CI Commands

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings` (default)
- `cargo test --workspace` (default)
- onnx feature leg (CI matrix + local): clippy `--all-targets -- -D warnings`
  and the test suite under the `onnx` feature.
