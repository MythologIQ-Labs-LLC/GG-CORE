# Research Brief — B-29b-2: Manifest Loading + Architecture Dispatch in the Production Load Path

**Date**: 2026-07-28
**Analyst**: The Qor-logic Analyst
**Target**: B-29b-2 (issue #72 follow-up) — make ONNX inference reachable end-to-end by
loading a `ModelManifest` in the production load path and dispatching on
`ModelArchitecture`.
**Scope**: the two prod load sites, the sibling-manifest path convention, the
`ModelMetadata`/`ModelManifest` boundary. Builds on B-29a (`load_onnx_from_manifest`) and
B-29b-1 (registry holds `Arc<dyn Model>`).

---

## Executive Summary

The two remaining prerequisites are already in place — `load_onnx_from_manifest` returns
`Arc<dyn Model>` (B-29a + B-29b-1) and the registry holds `Arc<dyn Model>` (B-29b-1) — so
B-29b-2 is a small, bounded wiring change. The two production load sites
(`ffi/models.rs`, `python/session.rs`) are **structurally identical**, so the dispatch
belongs in **one shared helper**, not duplicated. The sibling-file convention B-28
established for the tokenizer (`with_file_name("tokenizer.json")`) extends directly to a
sibling `manifest.json`. To preserve every existing GGUF load (which ships no manifest),
manifest resolution must be **optional with a GGUF default**. This is a single L2 cycle.

## Findings (verified)

### F1 — the two load sites are structurally identical (shared helper, no duplication)
- `ffi/models.rs` (`core_model_load`): `validate_path(path_str)` → `load_metadata(&validated)`
  → `model_id = metadata.name` → `gguf::load_gguf_model(validated.as_path(), &model_id,
  &GgufConfig::default())` → `model_lifecycle.load(model_id, metadata, model)`.
- `python/session.rs` (`load_model`, :99): the identical five-step sequence.
- The only backend-selection point in each is the `load_gguf_model(...)` call. Replacing
  both with one shared `load_model_dispatch(path, model_id)` avoids duplicating the
  branch (SME: single source of truth for dispatch).

### F2 — sibling-manifest path convention is already precedented
- B-28's `OnnxTokenizer::for_model` (`onnx/tokenizer.rs:25`) resolves
  `model_path.with_file_name("tokenizer.json")`. The manifest sibling is therefore
  `model_path.with_file_name("manifest.json")` — same idiom, no new path machinery.
- `ModelManifest::from_file(&Path)` (`models/manifest.rs:61`) already parses a manifest
  JSON file; `ModelArchitecture` (`:44`) = {Gguf, Onnx, SafeTensors}; `labels` (B-29a).

### F3 — manifest resolution must be OPTIONAL, defaulting to GGUF (behavior preservation)
- `ModelLoader::load_metadata` (`loader.rs:110`) synthesizes `ModelMetadata{name,
  size_bytes}` from the file; existing GGUF models have **no** sibling `manifest.json`.
- Therefore the dispatch must: if a sibling `manifest.json` exists and parses with
  `architecture == Onnx` → `load_onnx_from_manifest`; **otherwise** (absent manifest,
  parse error, or `Gguf`/`SafeTensors`) → `load_gguf_model` (today's behavior). A
  mandatory manifest would break every current GGUF load.

### F4 — `ModelMetadata` stays for the lifecycle; the manifest only drives dispatch
- `model_lifecycle.load(model_id, metadata: ModelMetadata, model)` (`lifecycle.rs:91`)
  consumes `ModelMetadata` (name+size). The manifest carries `architecture` +
  `capabilities` + `labels` needed only to *choose and construct* the backend.
- Minimal reconciliation: keep `load_metadata`'s `ModelMetadata` for the lifecycle;
  read the manifest solely for dispatch. No `ModelMetadata`/`ModelManifest` merge is
  required for B-29b-2 (a fuller unification remains optional future work, not blocking).

### F5 — the wiring pieces connect
- `load_onnx_from_manifest(&ModelManifest, &Path, &OnnxConfig) -> Result<Arc<dyn Model>,
  InferenceError>` (`onnx/dispatch.rs`, return type updated in B-29b-1) and
  `load_gguf_model(&Path, &str, &GgufConfig) -> Result<Arc<dyn Model>, InferenceError>`
  (`gguf/mod.rs`) now share the return type the registry stores. A dispatcher can return
  either into `lifecycle.load` unchanged.

## Blueprint Alignment

| Claim | Actual finding | Status |
|---|---|---|
| Load sites branch on `ModelArchitecture` to call `load_onnx_from_manifest` (B-29b brief) | Achievable now; both sites identical → one shared helper | MATCH (ready) |
| Manifest describes architecture for dispatch | True; sibling `manifest.json` via B-28 idiom; must be optional (F3) | MATCH (with default-GGUF guard) |
| `ModelMetadata`/`ModelManifest` reconciliation needed | Not required for B-29b-2 — manifest drives dispatch only (F4) | PARTIAL (deferred, non-blocking) |

## Recommendations (scope forks for the plan — decide at cycle start)

1. **Single bounded cycle (L2).** Add one shared `load_model_dispatch(model_path,
   model_id) -> Result<Arc<dyn Model>, InferenceError>` that resolves the sibling
   `manifest.json` (optional; default GGUF) and branches on `architecture`; call it from
   both `ffi/models.rs` and `python/session.rs` in place of `load_gguf_model`.
2. **Home**: `models/` (it bridges `models::manifest::ModelManifest` and the engine
   backends). A new small module keeps it Razor-clean and unit-testable.
3. **Decompose decision from IO** (mirrors B-29a): a pure `manifest_backend(model_path)
   -> BackendChoice` (reads the sibling, returns `Onnx(manifest)` | `GgufDefault`) that is
   unit-testable with temp-dir fixtures, plus a thin loader that performs the IO. Assert
   routing: onnx-manifest sibling → Onnx branch; no/GGUF manifest → GGUF branch.
4. **Explicit scope note**: SafeTensors has no loader; it falls to the GGUF default and
   fails loudly there — acceptable (no SafeTensors path is wired anywhere).

## Updated Knowledge (Shadow Genome)

No new failure pattern. Confirms the staged split paid off: because B-29a (dispatch) and
B-29b-1 (registry neutrality) were done first, B-29b-2 is a thin, low-risk wiring change
rather than the "large refactor" the original B-29 framing implied.

---

_Research complete. Findings advisory; the shared-helper design + optional-manifest
default are operator decisions at cycle start._
