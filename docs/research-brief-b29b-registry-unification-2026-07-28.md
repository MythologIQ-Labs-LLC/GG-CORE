# Research Brief — B-29b: Unified GGUF/ONNX Model Abstraction (Registry Unification)

**Date**: 2026-07-28
**Analyst**: The Qor-logic Analyst
**Target**: B-29b (issue #72 follow-up) — let the engine registry hold ONNX models so
manifest-driven ONNX inference is reachable end-to-end through Runtime / FFI / Python.
**Scope**: the `InferenceEngine` registry, the `GgufModel` vs `OnnxModel` trait
surfaces, the model-lifecycle coordinator, the two production load sites, streaming, and
the manifest-load path. Builds on B-29a (`engine/onnx/dispatch.rs load_onnx_from_manifest`).

---

## Executive Summary

The "different traits, no shared home" framing from the B-29a brief (F2) is real but
**much smaller than feared**: `OnnxModel` is a near-exact **subset** of `GgufModel`, so a
single unified `Model` trait is a natural, low-churn refactor (~6 production + ~5 test
sites). The genuinely surprising finding is a **second, previously-undocumented gap**:
the production load path (`ffi/models.rs`, `python/session.rs`) **never loads a
`ModelManifest` at all** — `ModelLoader::load_metadata` synthesizes `ModelMetadata {
name, size_bytes }` from the file itself, with no `architecture`, `capabilities`, or
`labels`. So even after trait unification, architecture dispatch has **no input** in
prod; B-29b must also wire `ModelManifest` loading into the two load sites. Recommend a
staged split: **B-29b-1** (unified `Model` trait + registry/lifecycle migration,
GGUF-only, behavior-preserving) then **B-29b-2** (manifest loading + architecture
dispatch + ONNX impls satisfy `Model`).

## Findings (verified)

### F1 — the two traits are near-identical; `OnnxModel ⊂ GgufModel`
- `GgufModel` (`engine/gguf/mod.rs:47`): `model_id`, `capabilities`, `memory_usage`,
  `infer` (async), `infer_cancellable` (async, **default → infer**), `unload` (async),
  `set_device_placement` (**default no-op**), `as_any`.
- `OnnxModel` (`engine/onnx/mod.rs:45`): `model_id`, `capabilities`, `memory_usage`,
  `infer` (async), `unload` (async).
- `OnnxModel` is exactly `GgufModel` minus `{infer_cancellable, set_device_placement,
  as_any}` — all three of which have **default** or downcast-only semantics. Both traits
  use the identical `Inference{Capability,Input,Config,Output,Error}` types. A unified
  `Model` trait = the current `GgufModel` shape; ONNX impls gain only `as_any` (and
  inherit the two defaults). This is a mechanical promotion, not a redesign.

### F2 — registry blast radius is moderate and enumerable
`Arc<dyn GgufModel>` production sites: `engine/inference.rs:19` (registry map), `:35`
(register_model), `:119` (get_model), `:142`/`:150` (infer helpers); `models/lifecycle.rs:95`
(`load(model_id, metadata, model)`); `engine/gguf/mod.rs:89`/`:106` (load_gguf_model
returns). Test sites: `inference_tests.rs:106/242/263`, `lifecycle_tests.rs:21`,
`worker_tests.rs:21`. ~6 prod + ~5 test edit points. Migration = replace `dyn GgufModel`
with `dyn Model`; `OnnxEmbedder`/`OnnxClassifier` (`onnx/embedder.rs:132`,
`onnx/classifier.rs:158`, currently `impl super::OnnxModel`) implement `Model`;
`load_onnx_from_manifest` (B-29a) return type `Arc<dyn OnnxModel>` → `Arc<dyn Model>`.

### F3 — the production load path never loads a manifest (DRIFT, blocking)
- `ModelLoader::load_metadata` (`models/loader.rs:110`) returns `ModelMetadata { name,
  size_bytes }` (`loader.rs:139`) built from `path.file_stem()` + `fs::metadata(path).len()`
  — it does **not** parse a `manifest.json`. No `architecture`, `capabilities`, or
  `labels` reach the load sites.
- Both prod load sites use this: `ffi/models.rs:52` and `python/session.rs:106` call
  `load_gguf_model(...)` with `metadata.name` only; neither has a `ModelManifest`.
- Consequence: architecture dispatch (choosing GGUF vs ONNX; B-29a's `plan_onnx_load`)
  has **no input** in prod. B-29b must add manifest loading (`ModelManifest::from_file`
  on the sibling `manifest.json`, matching B-28's sibling-file convention) to the load
  sites and branch on `manifest.architecture`.

### F4 — two parallel metadata concepts (reconciliation question)
- `ModelMetadata { name, size_bytes }` (`loader.rs:139`) — used by the FFI/Python load
  path + `lifecycle.load`.
- `ModelManifest { .., capabilities, architecture, labels, .. }` (`models/manifest.rs:12`)
  — used only by `models/preload.rs:8` and `models/swap.rs:11`, NOT the main load path.
- B-29b must decide whether the load path threads a `ModelManifest` alongside/instead of
  `ModelMetadata`, or whether `lifecycle.load` gains an architecture/manifest parameter.

### F5 — streaming needs no special-casing for ONNX
- Streaming (`inference.rs:255`) does `model.as_any().downcast_ref::<GgufGenerator>()` and
  returns `"model does not support streaming"` on failure. Under a unified `Model` trait,
  an ONNX model returns itself from `as_any()`, fails the downcast, and yields exactly
  that error — the correct behavior for a non-streaming backend. No new abstraction needed;
  `as_any` must remain on the unified trait.

## Blueprint Alignment

| Claim | Actual finding | Status |
|---|---|---|
| B-29a brief F2: "ONNX has no registry home; requires unified abstraction (large refactor)" | True, but `OnnxModel ⊂ GgufModel` makes it a mechanical promotion, ~6 prod sites | MATCH (smaller than framed) |
| Load sites branch on `ModelArchitecture` to call `load_onnx_from_manifest` | Load sites have no manifest at all — architecture is never loaded in prod | DRIFT (worse than stated) |
| Manifest describes capabilities for dispatch | True, but only `preload`/`swap` read it; main load path uses name+size `ModelMetadata` | PARTIAL |

## Recommendations (scope forks for the plan — decide at cycle start)

1. **Stage B-29b into two governed cycles** (recommended over big-bang; each is
   independently green + mergeable):
   - **B-29b-1 — unified `Model` trait + registry migration** (L2-L3): promote
     `GgufModel` to a backend-neutral `Model` trait (or introduce `Model` as its exact
     superset), migrate the registry (`inference.rs`), `lifecycle.load`, and the GGUF
     load return to `Arc<dyn Model>`. Behavior-preserving — GGUF stays the only wired
     backend; ONNX impls gain `as_any` and implement `Model` but remain unreached. Fully
     testable with existing GGUF tests + a mock ONNX `Model`.
   - **B-29b-2 — manifest loading + architecture dispatch** (L2): add
     `ModelManifest::from_file` (sibling `manifest.json`) to the two load sites; branch on
     `manifest.architecture` → `load_gguf_model` vs `load_onnx_from_manifest` (B-29a);
     reconcile `ModelMetadata` vs `ModelManifest` (F4) at `lifecycle.load`. This is the
     step that finally makes ONNX servable end-to-end.
2. **Single big-bang** is viable given the moderate blast radius but couples a trait
   migration with a load-path change and the metadata reconciliation in one audit — higher
   VETO surface; prefer staged.
3. **Naming**: prefer promoting `GgufModel` → `Model` (it is already the superset) with a
   deprecated `pub use Model as GgufModel;` shim only if external consumers reference it
   (grep shows none outside the crate; the shim is likely unnecessary — no back-compat
   concern per project rules).

## Updated Knowledge (Shadow Genome)

New pattern: **"a 'unification' framed as a big refactor can be a subset-promotion."** The
B-29a brief flagged GGUF/ONNX as "different traits" implying a large abstraction effort;
verifying the two trait bodies side-by-side showed one is a strict subset of the other,
collapsing the risk. Always diff the actual trait signatures before sizing a "unify X and
Y" epic. Corollary (F3): sizing a dispatch feature also requires confirming the dispatch
*input* is actually loaded in the production path — here the manifest never was.

---

_Research complete. Findings advisory; the scope fork (staged B-29b-1/B-29b-2 vs
big-bang) and the metadata-reconciliation approach are operator decisions at cycle start._
