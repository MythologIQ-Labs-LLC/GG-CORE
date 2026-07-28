# Plan: B-29b-1 — Unified `Model` Trait + Registry Migration

**change_class**: feature

**doc_tier**: standard

**terms_introduced**:
- term: Model
  home: core-runtime/src/engine/model.rs

**boundaries**:
- limitations:
  - Behavior-preserving. GGUF remains the only backend wired into the production load path (`ffi/models.rs`, `python/session.rs` still call `load_gguf_model`). ONNX models can now *live in* the registry type but are still never *loaded* into it.
- non_goals:
  - Manifest loading in the prod load path, `ModelArchitecture` dispatch, wiring `load_onnx_from_manifest` into FFI/Python — all B-29b-2.
  - `ModelMetadata`/`ModelManifest` reconciliation (B-29b-2).
- exclusions:
  - No change to inference behavior, streaming semantics, or any user-facing surface.
  - `set_device_placement` is dropped from the abstraction (dead code — never called; see Design Rationale), not migrated.

## Open Questions

None. Scope fork resolved at cycle start: **staged, B-29b-1 first** (unified trait + registry migration, GGUF-only wired); B-29b-2 (manifest loading + dispatch) is a separate cycle.

## Design Rationale (Simple Made Easy)

The research (brief F1) established `OnnxModel` is a strict subset of `GgufModel`; the two differ only in `GgufModel`'s `infer_cancellable` (defaulted), `set_device_placement` (defaulted), and `as_any`. So the unified trait is `GgufModel`'s shape, and unification is a promotion, not a redesign.

Three SME decisions:
1. **Neutral home.** The unified `pub trait Model` lives in a new `core-runtime/src/engine/model.rs`, not inside `gguf/` or `onnx/` — the shared abstraction should not live in one of its implementors' modules. Both `GgufModel` and `OnnxModel` traits are deleted; every implementor and `dyn` site repoints to `Model`.
2. **Drop `set_device_placement`.** It is a dead trait method: a no-op default (`gguf/mod.rs:74`) with no override and no caller anywhere in the crate (grep: only the definition). Carrying it onto the neutral `Model` trait would couple the abstraction to the GGUF-specific `DevicePlacement`/`gpu` type for zero behavior. It is removed, not migrated.
3. **Streaming stays downcast-based (F5).** `run_stream_sync`/`stream_tokens` keep `as_any().downcast_ref::<GgufGenerator>()`; a registered non-GGUF `Model` fails the downcast and yields the existing `"model does not support streaming"` error — the correct behavior for a non-streaming backend. `as_any` therefore stays on the unified trait.

**Razor note (pre-existing).** `core-runtime/src/engine/inference.rs` is **271 lines today — already over the 250 limit** (pre-existing, like `backend.rs`). Because this migration touches it pervasively, this plan brings it into compliance by extracting the two `#[cfg(feature="gguf")]` streaming methods (~58 lines) into a sibling `inference_streaming.rs` `impl InferenceEngine` block. This is a Razor-compliance extraction, not new behavior.

## Phase 1: Define the unified `Model` trait

### Affected Files

- `core-runtime/src/engine/model.rs` (NEW, ~30 lines) — `pub trait Model: Send + Sync` (`#[async_trait]`): `model_id`, `capabilities`, `memory_usage`, `infer`, `infer_cancellable` (default → `infer`), `unload`, `as_any`. No `set_device_placement`.
- `core-runtime/src/engine/mod.rs` — add `mod model;` + `pub use model::Model;`; change line 85 `pub use gguf::{GgufConfig, GgufGenerator, GgufModel};` → `pub use gguf::{GgufConfig, GgufGenerator};` (drop `GgufModel`); change line 94 `pub use onnx::{OnnxClassifier, OnnxConfig, OnnxEmbedder, OnnxModel};` → `pub use onnx::{OnnxClassifier, OnnxConfig, OnnxEmbedder};` (drop `OnnxModel`). Both traits are removed from the public engine surface; consumers use `Model`.

### Changes

```rust
// core-runtime/src/engine/model.rs
use crate::engine::{InferenceCapability, InferenceConfig, InferenceError};
use crate::engine::{InferenceInput, InferenceOutput};

/// Backend-neutral model abstraction: the registry holds `Arc<dyn Model>`,
/// so GGUF and ONNX backends share one home. Superset of the ONNX surface;
/// `infer_cancellable` defaults to `infer` for backends without per-token
/// cancellation. `as_any` supports the streaming downcast to a concrete
/// backend (a non-streaming backend simply fails the downcast).
#[async_trait::async_trait]
pub trait Model: Send + Sync {
    fn model_id(&self) -> &str;
    fn capabilities(&self) -> &[InferenceCapability];
    fn memory_usage(&self) -> usize;

    async fn infer(
        &self,
        input: &InferenceInput,
        config: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError>;

    async fn infer_cancellable(
        &self,
        input: &InferenceInput,
        config: &InferenceConfig,
        _is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, InferenceError> {
        self.infer(input, config).await
    }

    async fn unload(&mut self) -> Result<(), InferenceError>;

    fn as_any(&self) -> &dyn std::any::Any;
}
```

### Unit Tests

_(No standalone test for the trait definition; it is exercised by Phase 2's registry test and Phase 3's implementor migrations, which invoke real `Model` methods.)_

## Phase 2: Migrate GGUF + the registry/lifecycle to `Model`

### Affected Files

- `core-runtime/src/engine/gguf/mod.rs` — delete `pub trait GgufModel` (lines 45–78, incl. `set_device_placement`); drop now-unused `use crate::engine::gpu::DevicePlacement` (line 20) if unreferenced elsewhere in the file; change `load_gguf_model` return `Arc<dyn GgufModel>` → `Arc<dyn Model>` (lines 89, 106); add `use crate::engine::Model`.
- `core-runtime/src/engine/gguf/generator.rs` — line 173 `impl super::GgufModel for GgufGenerator` → `impl crate::engine::Model for GgufGenerator` (its methods already match; `as_any` already present since `GgufModel` required it).
- `core-runtime/src/engine/inference.rs` — lines 19, 35, 119, 142, 150 `Arc<dyn GgufModel>` → `Arc<dyn Model>`; replace `use crate::engine::gguf::GgufModel` with `use crate::engine::Model`. **Extract** the two `#[cfg(feature="gguf")]` methods `run_stream_sync` + `stream_tokens` (lines ~205–266) into a new sibling module (Phase 2b) to satisfy Razor.
- `core-runtime/src/engine/inference_streaming.rs` (NEW, ~65 lines) — `#[cfg(feature = "gguf")] impl InferenceEngine { run_stream_sync, stream_tokens }`, verbatim moved bodies; referenced from `inference.rs` via `#[cfg(feature = "gguf")] #[path = "inference_streaming.rs"] mod streaming;`.
- `core-runtime/src/models/lifecycle.rs` — line 95 `model: Arc<dyn GgufModel>` → `Arc<dyn Model>`; import `use crate::engine::Model`.

### Changes

Mechanical type substitution + the streaming extraction described above. `GgufGenerator`'s trait body is unchanged (same method set); only the `impl` header trait path changes.

### Unit Tests

- `core-runtime/src/engine/inference_tests.rs` (add one test; existing mocks migrate in Phase 3) — `registry_holds_non_gguf_model_and_infers`:
  construct a mock `Model` whose `model_id()`/`capabilities()`/`memory_usage()`/`infer()` are backend-agnostic (NOT a `GgufGenerator`), register it via `InferenceEngine::register_model`, then:
  - `has_model(id)` → `true`;
  - `model_memory_usage(id)` → the mock's reported bytes (asserts the stored `Arc<dyn Model>` is callable);
  - `run(id, prompt, params)` → the mock's `InferenceResult` output value.
  Proves the registry is backend-neutral (the B-29b-1 deliverable), asserting on returned values, not artifact presence.
- `core-runtime/src/engine/inference_tests.rs` — `non_gguf_model_stream_reports_unsupported` (`#[cfg(feature = "gguf")]`): register the same non-GGUF mock `Model`, call `run_stream_sync`, and assert the terminal frame is `StreamTerminal::Error(msg)` with `msg` containing `"does not support streaming"` (F5 — the downcast rejects a non-GGUF backend).

## Phase 3: Migrate ONNX implementors + `dyn OnnxModel` sites to `Model`

### Affected Files

- `core-runtime/src/engine/onnx/mod.rs` — delete `pub trait OnnxModel` (lines 45–57); change `load_onnx_model` + `load_onnx_classifier` returns (lines 79, 94, 111, 126) `Arc<dyn OnnxModel>` → `Arc<dyn Model>`; add `use crate::engine::Model`; keep `OnnxConfig`/`OnnxDevice`.
- `core-runtime/src/engine/onnx/dispatch.rs` — lines 71, 88 `Arc<dyn OnnxModel>` → `Arc<dyn Model>`; change import `use super::{OnnxConfig, OnnxModel}` → `use super::OnnxConfig` + `use crate::engine::Model`.
- `core-runtime/src/engine/onnx/embedder.rs` — line 132 `impl super::OnnxModel for OnnxEmbedder` → `impl crate::engine::Model for OnnxEmbedder`; **add** `fn as_any(&self) -> &dyn std::any::Any { self }`.
- `core-runtime/src/engine/onnx/classifier.rs` — line 158 `impl super::OnnxModel for OnnxClassifier` → `impl crate::engine::Model for OnnxClassifier`; **add** `fn as_any(&self) -> &dyn std::any::Any { self }`.

### Changes

`OnnxEmbedder`/`OnnxClassifier` already provide `model_id`/`capabilities`/`memory_usage`/`infer`/`unload`; the only new method is `as_any`. They inherit `infer_cancellable`'s default. The `Model` return type of `load_onnx_from_manifest` (B-29a) now matches the registry type — this is what makes an ONNX model *registerable* (though still not *loaded* in prod; that is B-29b-2).

### Unit Tests

- `core-runtime/src/engine/onnx/dispatch_tests.rs` — existing 10 tests unchanged in behavior; the `plan_onnx_load` tests are type-agnostic. Add `onnx_model_as_any_downcasts_to_concrete` (`#[cfg(feature = "onnx")]` where a real model can be built is not required — instead a construction-free check): assert that an `OnnxEmbedder`/`OnnxClassifier` value used as `&dyn Model` returns a non-`None` `as_any().downcast_ref::<OnnxEmbedder>()` (or classifier), confirming `as_any` returns `self`. If constructing these types requires a model file, gate this test `#[cfg(feature = "onnx")]` and assert via a lightweight constructor; otherwise assert the trait-object round-trip on a minimal instance.

## Phase 4: Migrate test doubles

### Affected Files

- `core-runtime/src/engine/inference_tests.rs` — lines 74 `impl GgufModel for BudgetModel`, 185 `impl GgufModel for CancellableModel` → `impl Model for ...`; lines 106, 242, 263 `StdArc<dyn GgufModel>` → `StdArc<dyn Model>`; update `use` of the trait. (Mocks already implement `as_any`.)
- `core-runtime/src/models/lifecycle_tests.rs` — line 21 `Arc<dyn GgufModel>` → `Arc<dyn Model>`; line 30 `impl GgufModel for MockModel` → `impl Model for MockModel`; update import.
- `core-runtime/src/scheduler/worker_tests.rs` — line 21 `Arc<dyn GgufModel>` → `Arc<dyn Model>`; line 27 `impl GgufModel for MockModel` → `impl Model for MockModel`; update import.
- `core-runtime/tests/backend_test.rs` — lines 6–7 `use gg_core::engine::gguf::{GgufGenerator, GgufModel};` + `use gg_core::engine::onnx::{OnnxClassifier, OnnxEmbedder, OnnxModel};` → drop both `GgufModel`/`OnnxModel`, add `use gg_core::engine::Model;` (the trait methods the test calls on `GgufGenerator`/`OnnxClassifier`/`OnnxEmbedder` now resolve via `Model`).
- `core-runtime/tests/e2e_model_test.rs` — line 9 imports `GgufModel` from the engine re-export (`gg_core::engine::{.., GgufModel, ..}`); replace `GgufModel` with `Model` in that `use` list.

### Changes

Type/trait-path substitution only. Every mock already satisfies `Model` (they implemented `GgufModel`, the superset, including `as_any`); dropping `set_device_placement` from the trait removes nothing they relied on (it was a default). The two integration tests (`backend_test.rs`, `e2e_model_test.rs`) call trait methods on concrete backend instances, so they need the `Model` trait in scope instead of the deleted `GgufModel`/`OnnxModel`. These are compile-level migrations; the tests' existing behavior assertions are unchanged and continue to prove registry/lifecycle/worker/e2e behavior.

**Caller-enumeration completeness** (SG-AffectedFilesContract-A, per Shadow Genome #12): a bare-identifier grep `grep -rn '\b(Gguf|Onnx)Model\b' core-runtime --include=*.rs` yields exactly 14 files — all enumerated across Phases 1–4: `engine/{model.rs[new], mod.rs, inference.rs, gguf/mod.rs, gguf/generator.rs, onnx/mod.rs, onnx/dispatch.rs, onnx/embedder.rs, onnx/classifier.rs}`, `models/lifecycle.rs`, and tests `{inference_tests, lifecycle_tests, worker_tests, backend_test, e2e_model_test}.rs`. No site remains outside Affected Files.

## Feature Inventory Touches

Empty — justified. B-29b-1 is a behavior-preserving internal abstraction refactor: no user-touchable feature is introduced or modified (ONNX remains unreached in the prod load path; GGUF behavior is unchanged). New capability is registry-type neutrality, exercised by the Phase 2 unit tests, not a user surface.

## Definition of Done

### Deliverable: unified `Model` trait

- **D1**: GGUF and ONNX models share one backend-neutral registry type; the abstraction carries no dead or backend-specific methods.
- **D2**: `pub trait Model` in `core-runtime/src/engine/model.rs` (`model_id`/`capabilities`/`memory_usage`/`infer`/`infer_cancellable`[default]/`unload`/`as_any`; no `set_device_placement`); re-exported from `engine/mod.rs`. `GgufModel` and `OnnxModel` traits deleted.
- **D3**: META_LEDGER entry (canonical inline-backtick markup) records the unification + `set_device_placement` removal; BACKLOG B-29b split into B-29b-1 (done) / B-29b-2 (open).
- **D4**: `registry_holds_non_gguf_model_and_infers` passes — a non-GGUF `Model` registers and returns its inference output through `InferenceEngine::run`.

### Deliverable: registry/lifecycle migration + Razor compliance

- **D1**: The registry, lifecycle, and GGUF/ONNX loaders all traffic in `Arc<dyn Model>`; `inference.rs` is ≤250 lines.
- **D2**: `Arc<dyn Model>` at `inference.rs` (registry + methods), `lifecycle.rs:load`, `gguf/mod.rs` + `onnx/mod.rs` + `onnx/dispatch.rs` loader returns; streaming methods relocated to `inference_streaming.rs`.
- **D3**: Covered by the same ledger entry; `SYSTEM_STATE.md` onnx/engine tree updated.
- **D4**: `non_gguf_model_stream_reports_unsupported` passes (downcast rejects non-GGUF backend); full suite green under default + `--features onnx`.

## CI Commands

```bash
cargo build -p gg-core --all-features                                   # full-feature compile incl. onnx impls
cargo clippy -p gg-core --all-targets --all-features -- -D warnings     # lint clean, warnings-as-errors
cargo test -p gg-core --features onnx                                   # onnx impls compile + dispatch tests
cargo test -p gg-core                                                   # default: registry/lifecycle/worker + new neutrality tests
cargo test -p gg-core --features gguf                                   # gguf streaming path incl. non-gguf stream-unsupported test
cargo fmt --check                                                       # formatting
```
