# Plan: B-29b-2 — Manifest Loading + Architecture Dispatch (prod load path)

**change_class**: feature

**doc_tier**: standard

**terms_introduced**:
- term: BackendChoice
  home: core-runtime/src/models/backend_dispatch.rs
- term: load_model_dispatch
  home: core-runtime/src/models/backend_dispatch.rs

**boundaries**:
- limitations:
  - Manifest resolution is optional: a model with no sibling `manifest.json` (or a
    non-Onnx manifest) loads via GGUF exactly as today. Only a sibling `manifest.json`
    with `architecture: onnx` routes to the ONNX loader.
  - SafeTensors has no loader; it falls to the GGUF default and fails loud there.
- non_goals:
  - A full `ModelMetadata`/`ModelManifest` merge (research F4 — the manifest drives
    dispatch only; `ModelMetadata` still feeds the lifecycle).
  - Any change to `load_metadata`, the lifecycle signature, or the registry.
- exclusions:
  - No new manifest fields; no change to `load_onnx_from_manifest` (B-29a) or the
    unified `Model` trait (B-29b-1).

## Open Questions

None. Research recommended a single bounded L2 cycle with a shared dispatch helper and
optional-manifest-defaults-GGUF; adopted.

## Design Rationale (Simple Made Easy)

The decision (which backend a model resolves to) is decomplected from the effect (reading
the manifest file and constructing the model), mirroring B-29a:

- `choose_backend(manifest: Option<ModelManifest>) -> BackendChoice` — pure, total, no
  IO. `Some(m)` with `m.architecture == Onnx` → `Onnx(Box<ModelManifest>)` (carries the
  manifest forward for the loader); every other case (`None`, `Gguf`, `SafeTensors`) →
  `GgufDefault`. Fully unit-testable without files. Carrying the manifest in the variant
  removes any `unwrap`/`expect` in the loader.
- `load_model_dispatch(model_path, model_id) -> Result<Arc<dyn Model>, InferenceError>` —
  reads the optional sibling `manifest.json` (`with_file_name`, per the B-28 idiom),
  calls `choose_backend`, and performs the load (`load_onnx_from_manifest` for `Onnx`
  else `load_gguf_model`). One shared helper, called from both prod load sites — the
  branch is not duplicated across FFI and Python (research F1).

Both loaders already return `Arc<dyn Model>` (B-29a + B-29b-1), so either arm flows into
`model_lifecycle.load` unchanged. `ModelMetadata` (name+size) continues to feed the
lifecycle; the manifest is read only for dispatch (research F4).

## Phase 1: Shared dispatch helper

### Affected Files

- `core-runtime/src/models/backend_dispatch.rs` (NEW, ~70 lines) — `BackendChoice` enum,
  pure `choose_backend`, and `load_model_dispatch` (reads sibling `manifest.json`,
  dispatches). Module imports: `std::path::Path`, `std::sync::Arc`,
  `crate::engine::{InferenceError, Model}`, `crate::engine::gguf::{load_gguf_model,
  GgufConfig}`, `crate::engine::onnx::{load_onnx_from_manifest, OnnxConfig}`,
  `crate::models::manifest::{ModelArchitecture, ModelManifest}`.
- `core-runtime/src/models/mod.rs` — add `pub mod backend_dispatch;` and
  `pub use backend_dispatch::load_model_dispatch;`.

### Changes

```rust
// core-runtime/src/models/backend_dispatch.rs
use std::path::Path;
use std::sync::Arc;

use crate::engine::gguf::{load_gguf_model, GgufConfig};
use crate::engine::onnx::{load_onnx_from_manifest, OnnxConfig};
use crate::engine::{InferenceError, Model};
use crate::models::manifest::{ModelArchitecture, ModelManifest};

/// Which backend a model file resolves to. The ONNX variant carries the parsed
/// manifest forward so the loader needs no unwrap.
#[derive(Debug)]
pub enum BackendChoice {
    /// A sibling manifest declares ONNX architecture.
    Onnx(Box<ModelManifest>),
    /// No manifest, or a non-ONNX manifest — load as GGUF (default behavior).
    GgufDefault,
}

/// Decide the backend from an optional manifest, without IO. Consumes the
/// manifest, moving it into the `Onnx` variant.
pub fn choose_backend(manifest: Option<ModelManifest>) -> BackendChoice {
    match manifest {
        Some(m) if m.architecture == ModelArchitecture::Onnx => BackendChoice::Onnx(Box::new(m)),
        _ => BackendChoice::GgufDefault,
    }
}

/// Load the backend a model file selects, resolving an optional sibling
/// `manifest.json`. Absent/unparseable/non-ONNX manifest → GGUF (unchanged
/// behavior); `architecture: onnx` → the ONNX manifest dispatcher (B-29a).
pub fn load_model_dispatch(
    model_path: &Path,
    model_id: &str,
) -> Result<Arc<dyn Model>, InferenceError> {
    let manifest = ModelManifest::from_file(&model_path.with_file_name("manifest.json")).ok();
    match choose_backend(manifest) {
        BackendChoice::Onnx(m) => {
            load_onnx_from_manifest(&m, model_path, &OnnxConfig::default())
        }
        BackendChoice::GgufDefault => {
            load_gguf_model(model_path, model_id, &GgufConfig::default())
        }
    }
}
```

### Unit Tests

- `core-runtime/src/models/backend_dispatch_tests.rs` (NEW; sibling via
  `#[cfg(test)] #[path = "backend_dispatch_tests.rs"] mod tests;`; run under default
  features — no model file needed for the routing assertions):
  - `choose_onnx_manifest_selects_onnx` — `choose_backend(Some(onnx_manifest))` matches
    `BackendChoice::Onnx(m)` and the carried `m.architecture == Onnx`. Asserts the
    decision output + that the manifest is moved into the variant.
  - `choose_gguf_manifest_selects_gguf_default` — a `Gguf`-architecture manifest matches
    `BackendChoice::GgufDefault`.
  - `choose_safetensors_manifest_selects_gguf_default` — `SafeTensors` matches
    `GgufDefault` (no loader; falls to default).
  - `choose_absent_manifest_selects_gguf_default` — `choose_backend(None)` matches
    `GgufDefault`.
  - `dispatch_routes_onnx_manifest_to_onnx_loader` — write a temp dir with a dummy model
    file + a sibling `manifest.json` declaring `architecture: onnx` (+ `labels`); call
    `load_model_dispatch`; under default features assert the `Err` message contains
    `"ONNX support not compiled in"` (proves it routed to the ONNX loader, not GGUF).
  - `dispatch_routes_no_manifest_to_gguf_loader` — temp dir with a dummy model file and
    **no** `manifest.json`; `load_model_dispatch` → `Err` containing `"GGUF support not
    compiled in"` (routed to the GGUF default). Both routing tests assert the observable
    loader selection via its feature-gated error, not artifact presence.

## Phase 2: Wire both production load sites

### Affected Files

- `core-runtime/src/ffi/models.rs` — in `core_model_load`, replace the
  `gguf::load_gguf_model(validated.as_path(), &model_id, &gguf::GgufConfig::default())`
  call with `crate::models::load_model_dispatch(validated.as_path(), &model_id)`; drop
  the now-unused `gguf` load import if it becomes unused (keep `GgufConfig` only if still
  referenced). The surrounding error handling (`ModelLoadFailed`) is unchanged.
- `core-runtime/src/python/session.rs` — in `load_model`, replace
  `crate::engine::gguf::load_gguf_model(validated.as_path(), &model_id,
  &crate::engine::gguf::GgufConfig::default())` with
  `crate::models::load_model_dispatch(validated.as_path(), &model_id)`. Error mapping to
  `PyRuntimeError` is unchanged.

### Changes

Both sites keep `validate_path` → `load_metadata` → `model_id = metadata.name` →
`lifecycle.load(model_id, metadata, model)`; only the backend-construction call changes
to the shared dispatcher. No signature changes to `load_metadata` or `lifecycle.load`.

### Unit Tests

_(The two call-site edits are covered by the existing FFI/Python load tests plus the
Phase 1 routing tests; a full end-to-end ONNX load requires a real `.onnx` model +
manifest fixture, out of scope for unit tests and exercised by the fixture-gated
integration suite.)_

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| FX-model-load-dispatch | NEW | core-runtime/src/models/backend_dispatch_tests.rs | `load_model_dispatch` routes a sibling `architecture: onnx` manifest to the ONNX loader and a manifest-absent model to the GGUF loader (asserted via the feature-gated loader error) |

_(If `FEATURE_INDEX.md` has no existing model-load row, this ships as a NEW entry at
implement time; the routing tests are its proof.)_

## Definition of Done

### Deliverable: `choose_backend` + `load_model_dispatch`

- **D1**: The production load path selects GGUF or ONNX from an optional sibling manifest,
  defaulting to GGUF so existing GGUF loads are unchanged.
- **D2**: `pub fn choose_backend(Option<ModelManifest>) -> BackendChoice` (Onnx variant
  carries `Box<ModelManifest>`) and `pub fn load_model_dispatch(&Path, &str) ->
  Result<Arc<dyn Model>, InferenceError>` in `core-runtime/src/models/backend_dispatch.rs`,
  re-exported from `models/mod.rs`.
- **D3**: META_LEDGER entry (canonical markup) records the dispatcher + both call-site
  rewires; BACKLOG B-29b-2 → done; FEATURE_INDEX gains the model-load-dispatch row.
- **D4**: `dispatch_routes_onnx_manifest_to_onnx_loader` and
  `dispatch_routes_no_manifest_to_gguf_loader` pass, asserting the routed loader via its
  error.

### Deliverable: both load sites call the dispatcher

- **D1**: FFI (`core_model_load`) and Python (`load_model`) construct the backend via the
  shared dispatcher, so an ONNX model with a manifest is servable end-to-end.
- **D2**: `crate::models::load_model_dispatch(validated.as_path(), &model_id)` replaces
  the `load_gguf_model` call at both sites.
- **D3**: Covered by the same ledger entry.
- **D4.d**: No new unit test at the call sites (end-to-end ONNX load needs a real model
  fixture). **Follow-up phase**: fixture-gated integration coverage tracked with the ONNX
  e2e fixtures (existing `#[ignore]`/fixture pattern in `tests/`).

## CI Commands

```bash
cargo build -p gg-core --all-features                                   # full-feature compile
cargo clippy -p gg-core --all-targets --all-features -- -D warnings     # lint clean, warnings-as-errors
cargo test -p gg-core                                                   # default: choose_backend + routing tests
cargo test -p gg-core --features onnx                                   # onnx loader path compiles
cargo test -p gg-core --features gguf                                   # gguf loader path
cargo fmt --check                                                       # formatting
```
