# Plan: B-29a — Manifest-Driven ONNX Loader Dispatch

**change_class**: feature

**doc_tier**: standard

**terms_introduced**:
- term: OnnxLoadPlan
  home: core-runtime/src/engine/onnx/dispatch.rs
- term: plan_onnx_load
  home: core-runtime/src/engine/onnx/dispatch.rs

**boundaries**:
- limitations:
  - Produces `Arc<dyn OnnxModel>`; the engine registry is still `Arc<dyn GgufModel>`-typed, so the dispatched model cannot yet be registered or served end-to-end. That unification is B-29b.
  - The dispatcher has no production caller in this scope — it is an internal, unit-tested seam. Wiring it into `ffi/models.rs` / `python/session.rs` is deferred to B-29b.
- non_goals:
  - Unified GGUF/ONNX model abstraction / registry de-specialization (B-29b epic).
  - End-to-end ONNX inference reachable through Runtime/FFI/Python.
  - ONNX loaders for `TextGeneration` or `NamedEntityRecognition` (no such loader exists; those capabilities are not ONNX-servable here).
- exclusions:
  - Tokenizer path handling (sibling-convention, already shipped in B-28).
  - Any change to `manifest.validate()` structural rules.

## Open Questions

None. The scope fork (B-29a-only vs +B-29b) was resolved at cycle start: **B-29a only**; B-29b remains a tracked follow-up.

## Design Rationale (Simple Made Easy)

The dispatch **decision** (which loader a manifest selects, and whether the manifest is well-formed for ONNX loading) is decomplected from the **effect** (reading the `.onnx` file and constructing the model):

- `plan_onnx_load(&ModelManifest) -> Result<OnnxLoadPlan, InferenceError>` — pure, total, no IO, no `onnx` feature required. This is the load-bearing logic and is fully unit-testable, including happy paths, without a model file on disk.
- `load_onnx_from_manifest(&ModelManifest, &Path, &OnnxConfig) -> Result<Arc<dyn OnnxModel>, InferenceError>` — thin wrapper: calls `plan_onnx_load`, matches the plan, performs the IO via the existing `load_onnx_classifier` / `load_onnx_model`.

The labels-required-for-classification rule lives in `plan_onnx_load` (capability semantics), **not** in `ModelManifest::validate` (structural validation). This keeps structural manifest validity independent of ONNX-dispatch requirements — a manifest can be structurally valid yet not ONNX-dispatchable, and the error is raised where it is actionable (at dispatch), naming the missing `labels`.

**File placement (Section 4 Razor)**: the dispatch unit (`OnnxLoadPlan`, `plan_onnx_load`, `load_onnx_from_manifest`) lives in a **new** `core-runtime/src/engine/onnx/dispatch.rs` module, and its unit tests in a **sibling** `core-runtime/src/engine/onnx/dispatch_tests.rs`, referenced from `dispatch.rs` via `#[cfg(test)] #[path = "dispatch_tests.rs"] mod tests;`. This mirrors the established `onnx/classifier.rs` → `onnx/classifier_tests.rs` convention and keeps every file ≤ 250 lines (`mod.rs` gains only two lines: `mod dispatch; pub use dispatch::*;`). `dispatch.rs` ≈ 70 code lines; `dispatch_tests.rs` ≈ 76 lines.

**Servable-set rule** (total, unambiguous): from the manifest's capabilities, collect the ONNX-servable subset `{TextClassification, Embedding}`. Exactly one present → dispatch to its loader. Zero present → `Err` (no ONNX-servable capability). More than one present → `Err` (ambiguous). `TextGeneration` and `NamedEntityRecognition` are simply not in the servable set, so a manifest carrying `{TextClassification, NamedEntityRecognition}` resolves unambiguously to the classifier.

## Phase 1: Manifest `labels` field

### Affected Files

- `core-runtime/src/models/manifest.rs` — add `labels: Option<Vec<String>>` field to `ModelManifest`; document that it is required by ONNX classifier dispatch (ordered, label `i` ↔ logit `i`). `validate()` is unchanged (structural rules stay independent of dispatch semantics).

### Changes

Add to `ModelManifest`:

```rust
/// Ordered class labels for sequence-classification models
/// (label `i` corresponds to logit `i`). Required when the manifest
/// declares `TextClassification` and is dispatched to an ONNX classifier;
/// `None`/absent for non-classifier models.
#[serde(default)]
pub labels: Option<Vec<String>>,
```

`#[serde(default)]` keeps existing manifests (no `labels` key) parsing to `None`.

### Unit Tests

- `core-runtime/src/models/manifest.rs` (tests module):
  - `manifest_without_labels_parses_to_none` — a manifest JSON with no `labels` key parses; `manifest.labels == None`. Confirms backward-compatible deserialization behavior (output: `None`), not mere field presence.
  - `manifest_with_labels_parses_ordered` — a manifest JSON with `"labels": ["ham","spam"]` parses; `manifest.labels == Some(vec!["ham","spam"])` in that order. Confirms label order is preserved through deserialization.

## Phase 2: Pure dispatch decision (`plan_onnx_load`)

### Affected Files

- `core-runtime/src/engine/onnx/dispatch.rs` (NEW) — `OnnxLoadPlan` enum + `plan_onnx_load` function (both ungated — no `candle`/`onnx` dependency). Module header imports:
  ```rust
  use std::path::Path;
  use std::sync::Arc;
  use crate::engine::error::InferenceError;
  use crate::models::manifest::{ModelArchitecture, ModelCapability, ModelManifest};
  use super::{load_onnx_classifier, load_onnx_model, OnnxConfig, OnnxModel};
  ```
  (`Path`/`Arc`/`OnnxConfig`/`OnnxModel`/`load_onnx_*` are used by the Phase-3 wrapper in the same file.)
- `core-runtime/src/engine/onnx/mod.rs` — add `mod dispatch;` and `pub use dispatch::{load_onnx_from_manifest, plan_onnx_load, OnnxLoadPlan};` (two lines; re-exports keep the public path stable).

### Changes

```rust
// in onnx/dispatch.rs

/// The loader a manifest resolves to, with its required inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnnxLoadPlan {
    /// Sequence-classification model bound to ordered labels.
    Classifier(Vec<String>),
    /// Embedding model.
    Embedder,
}

/// Decide which ONNX loader a manifest selects, without performing IO.
///
/// Fails loud (returns `Err`) when the manifest is not ONNX architecture,
/// declares no ONNX-servable capability, declares more than one
/// (ambiguous), or declares `TextClassification` without `labels`.
pub fn plan_onnx_load(manifest: &ModelManifest) -> Result<OnnxLoadPlan, InferenceError> {
    if manifest.architecture != ModelArchitecture::Onnx {
        return Err(InferenceError::ModelError(format!(
            "manifest architecture is {:?}, expected Onnx",
            manifest.architecture
        )));
    }
    let classify = manifest.has_capability(ModelCapability::TextClassification);
    let embed = manifest.has_capability(ModelCapability::Embedding);
    match (classify, embed) {
        (true, true) => Err(InferenceError::ModelError(
            "manifest declares both TextClassification and Embedding; ONNX dispatch is ambiguous".into(),
        )),
        (true, false) => match &manifest.labels {
            Some(labels) if !labels.is_empty() => Ok(OnnxLoadPlan::Classifier(labels.clone())),
            _ => Err(InferenceError::ModelError(
                "TextClassification manifest requires non-empty `labels` for ONNX classifier dispatch".into(),
            )),
        },
        (false, true) => Ok(OnnxLoadPlan::Embedder),
        (false, false) => Err(InferenceError::ModelError(
            "manifest declares no ONNX-servable capability (TextClassification or Embedding)".into(),
        )),
    }
}
```

### Unit Tests

- `core-runtime/src/engine/onnx/dispatch_tests.rs` (NEW; sibling test module via `#[cfg(test)] #[path = "dispatch_tests.rs"] mod tests;` in `dispatch.rs`; run under default features — no model file, no `onnx` feature):
  - `plan_classifier_with_labels_returns_ordered_labels` — Onnx + `TextClassification` + `labels=["a","b"]` → `Ok(OnnxLoadPlan::Classifier(vec!["a","b"]))`. Asserts the decision output and label order.
  - `plan_classifier_without_labels_is_error` — Onnx + `TextClassification` + `labels=None` → `Err`, message names `labels`. Asserts the fail-loud output.
  - `plan_classifier_empty_labels_is_error` — Onnx + `TextClassification` + `labels=Some(vec![])` → `Err`. Empty label set is not a valid classifier.
  - `plan_embedding_returns_embedder` — Onnx + `Embedding` → `Ok(OnnxLoadPlan::Embedder)`.
  - `plan_classification_plus_ner_resolves_to_classifier` — Onnx + `{TextClassification, NamedEntityRecognition}` + labels → `Ok(Classifier(..))` (NER not in servable set, so no ambiguity).
  - `plan_both_servable_is_ambiguous_error` — Onnx + `{TextClassification, Embedding}` → `Err`, message says "ambiguous".
  - `plan_non_onnx_architecture_is_error` — `architecture=Gguf` + `Embedding` → `Err`, message names the architecture.
  - `plan_no_servable_capability_is_error` — Onnx + only `TextGeneration` → `Err` (no ONNX-servable capability).

## Phase 3: Effectful wrapper (`load_onnx_from_manifest`)

### Affected Files

- `core-runtime/src/engine/onnx/dispatch.rs` — add `load_onnx_from_manifest` in two `#[cfg]` forms (onnx / not-onnx) in the same new module, mirroring the existing `load_onnx_model` / `load_onnx_classifier` pattern.

### Changes

```rust
// in onnx/dispatch.rs
/// Dispatch: load the ONNX model a manifest selects, using its declared
/// capability + labels. Decision is delegated to `plan_onnx_load`; this
/// wrapper only performs the IO.
#[cfg(feature = "onnx")]
pub fn load_onnx_from_manifest(
    manifest: &ModelManifest,
    path: &Path,
    config: &OnnxConfig,
) -> Result<Arc<dyn OnnxModel>, InferenceError> {
    match plan_onnx_load(manifest)? {
        OnnxLoadPlan::Classifier(labels) => {
            load_onnx_classifier(path, &manifest.model_id, labels, config)
        }
        OnnxLoadPlan::Embedder => load_onnx_model(path, &manifest.model_id, config),
    }
}

/// Stub for non-onnx builds. The decision (`plan_onnx_load`) still runs so
/// a malformed manifest fails loud with the same message; only the load is
/// gated out.
#[cfg(not(feature = "onnx"))]
pub fn load_onnx_from_manifest(
    manifest: &ModelManifest,
    _path: &Path,
    _config: &OnnxConfig,
) -> Result<Arc<dyn OnnxModel>, InferenceError> {
    plan_onnx_load(manifest)?;
    Err(InferenceError::ModelError(
        "ONNX support not compiled in. Enable 'onnx' feature.".into(),
    ))
}
```

### Unit Tests

- `core-runtime/src/engine/onnx/dispatch_tests.rs` (same sibling module, default features / not-onnx):
  - `load_from_manifest_stub_rejects_bad_manifest_before_feature_error` — non-Onnx manifest → `Err` naming the architecture (the `plan_onnx_load` error), proving the decision runs ahead of the feature-gate error. Asserts the wrapper surfaces the decision error, not the generic "not compiled in" message.
  - `load_from_manifest_stub_valid_manifest_reports_feature_absent` — a valid Embedding manifest under not-onnx → `Err` whose message says ONNX support is not compiled in. Confirms the gate path for well-formed manifests.

_(The `#[cfg(feature="onnx")]` happy-path load performs real `candle_onnx::read_file` IO and is exercised by the existing onnx-feature integration tests, not by these unit tests — the dispatch decision it relies on is fully covered by Phase 2.)_

## Feature Inventory Touches

Empty — justified. This plan adds an internal dispatch seam with **no production caller** (F1 in the research brief: `ffi/models.rs` / `python/session.rs` hard-code `load_gguf_model`; wiring ONNX in requires the B-29b registry unification). No user-touchable feature surface changes in B-29a.

## Definition of Done

### Deliverable: `plan_onnx_load` pure dispatch decision

- **D1**: A manifest's ONNX loader selection is a total, IO-free function that fails loud on non-Onnx architecture, absent/ambiguous servable capability, and classification-without-labels.
- **D2**: `pub fn plan_onnx_load(&ModelManifest) -> Result<OnnxLoadPlan, InferenceError>` in `core-runtime/src/engine/onnx/dispatch.rs` (re-exported from `mod.rs`), ungated; `OnnxLoadPlan::{Classifier(Vec<String>), Embedder}`.
- **D3**: META_LEDGER entry (canonical inline-backtick hash markup, Entry #124+) records the B-29a dispatch decision and the B-29b deferral; BACKLOG B-29 row updated to B-29a done / B-29b tracked.
- **D4**: `plan_both_servable_is_ambiguous_error`, `plan_classifier_without_labels_is_error`, and `plan_embedding_returns_embedder` pass, asserting the returned `Result`/`OnnxLoadPlan` values (not artifact presence).

### Deliverable: `ModelManifest.labels`

- **D1**: Manifests may carry ordered classifier labels; older manifests without the key still parse.
- **D2**: `pub labels: Option<Vec<String>>` with `#[serde(default)]` on `ModelManifest` in `core-runtime/src/models/manifest.rs`.
- **D3**: Covered by the same ledger entry; field documented inline.
- **D4**: `manifest_without_labels_parses_to_none` and `manifest_with_labels_parses_ordered` pass, asserting the deserialized `labels` value and order.

### Deliverable: `load_onnx_from_manifest` effectful wrapper

- **D1**: Loading an ONNX model from a manifest dispatches to the correct loader; a malformed manifest fails loud identically whether or not the `onnx` feature is compiled in.
- **D2**: `pub fn load_onnx_from_manifest(&ModelManifest, &Path, &OnnxConfig) -> Result<Arc<dyn OnnxModel>, InferenceError>` in `core-runtime/src/engine/onnx/dispatch.rs` (re-exported from `mod.rs`), in both `#[cfg(feature="onnx")]` and `#[cfg(not(feature="onnx"))]` forms.
- **D3**: Covered by the same ledger entry.
- **D4**: `load_from_manifest_stub_rejects_bad_manifest_before_feature_error` passes (decision error precedes feature-gate error).

## CI Commands

```bash
cargo build -p gg-core --all-features                                   # full-feature compile incl. onnx wrapper
cargo clippy -p gg-core --all-targets --all-features -- -D warnings     # lint clean, warnings-as-errors
cargo test -p gg-core --features onnx                                   # onnx-feature run; compiles real-IO wrapper path
cargo test -p gg-core                                                   # default run; plan_onnx_load + manifest + not-onnx stub tests
cargo fmt --check                                                       # formatting
```
