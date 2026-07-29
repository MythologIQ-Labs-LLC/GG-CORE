# Research Brief — B-29: Manifest-Driven ONNX Backend Auto-Dispatch

**Date**: 2026-07-28
**Analyst**: The Qor-logic Analyst
**Target**: B-29 (issue #72 scope-3) — manifest-driven embedder-vs-classifier
selection for the ONNX loaders.
**Scope**: model-load dispatch path, the ONNX model traits, and the model manifest.

---

## Executive Summary

B-29 as worded ("add manifest-driven backend selection over the ONNX loaders")
rests on a deeper, previously-undocumented gap: **the ONNX loaders are never called
from any production path, and the runtime's model registry is GGUF-typed, so an
ONNX model cannot be stored or served through the engine at all.** A manifest-driven
ONNX dispatcher is straightforward to build, but on its own it produces an
`Arc<dyn OnnxModel>` that the `InferenceEngine` (keyed to `Arc<dyn GgufModel>`)
cannot hold. The manifest also lacks the classifier `labels` the loader requires.
Recommendation: split B-29 into a bounded dispatcher (B-29a) and a model-trait
unification epic (B-29b), and add `labels` to the manifest.

## Findings (verified)

### F1 — the ONNX loaders have no production callers (DRIFT)
- `engine/onnx/mod.rs:100` `load_onnx_classifier` and `:71` `load_onnx_model` are
  referenced **only** within `onnx/mod.rs` (defs) and ONNX tests. A crate-wide grep
  excluding `onnx/mod.rs` finds **zero** call sites.
- The two production load sites both hard-code GGUF:
  `ffi/models.rs:52` `gguf::load_gguf_model(...)` and
  `python/session.rs:106` `crate::engine::gguf::load_gguf_model(...)`. Neither
  branches on `architecture`.
- So there is no dispatch to select embedder vs classifier because there is no
  dispatch to ONNX at all. This is an "exists + tested + not wired" surface (cf.
  Shadow Genome [[exists-tested-not-wired]]).

### F2 — the engine registry is GGUF-typed; ONNX has no home (DRIFT, blocking)
- `engine/inference.rs:19` `models: Arc<RwLock<HashMap<String, Arc<dyn GgufModel>>>>`.
  Registration (`:35`), lookup (`:119`), and streaming all traffic in
  `Arc<dyn GgufModel>`.
- `load_gguf_model` → `Arc<dyn GgufModel>` (`gguf/mod.rs:89`); `load_onnx_model` /
  `load_onnx_classifier` → `Arc<dyn OnnxModel>` (`onnx/mod.rs`). **Different traits.**
- Consequence: even a perfect manifest→ONNX-loader dispatcher yields a value the
  engine cannot store or serve. Making ONNX usable end-to-end requires a **unified
  model abstraction** (a trait both `GgufModel` and `OnnxModel` satisfy, or an enum
  registry), which is a larger refactor than "auto-dispatch."

### F3 — the manifest carries dispatch inputs but not classifier labels
- `models/manifest.rs:12` `ModelManifest { capabilities: Vec<ModelCapability>,
  architecture: ModelArchitecture, .. }`. `ModelCapability` (`:34`) =
  {TextClassification, TextGeneration, Embedding, NamedEntityRecognition};
  `ModelArchitecture` (`:44`) = {Gguf, Onnx, SafeTensors}. `has_capability` (`:88`)
  exists.
- So architecture + capability are sufficient to *choose* embedder vs classifier,
  BUT `load_onnx_classifier(path, model_id, labels, config)` needs `labels:
  Vec<String>` — the manifest has **no** labels field. A classifier can't be
  auto-loaded without it.

### F4 — #72 status
- #72 scope-1 (real classifier) shipped in PR #77 (MERGED 2026-07-27). scope-2 (real
  tokenizer) shipped as B-28 (#83). B-29 is the remaining scope-3. The issue is still
  open for this follow-up.

## Blueprint Alignment

| Blueprint / backlog claim | Actual finding | Status |
|---|---|---|
| "load_onnx_classifier exists but selection is not manifest-driven" (B-29 row) | loaders exist but have NO production caller; selection layer absent entirely | DRIFT (worse than stated) |
| ONNX is a supported runtime backend | engine registry is `Arc<dyn GgufModel>`-only; ONNX unstorable | DRIFT (architectural) |
| Manifest describes capabilities for dispatch | true, but classifier `labels` missing | PARTIAL |

## Recommendations (scope forks for the plan — decide at cycle start)

1. **Split B-29**:
   - **B-29a — manifest→ONNX-loader dispatcher** (bounded, L2): a
     `load_onnx_from_manifest(manifest, path) -> Arc<dyn OnnxModel>` that maps
     `TextClassification → load_onnx_classifier`, `Embedding → load_onnx_model`,
     fails loud on ambiguous/absent ONNX capability. Add manifest `labels:
     Option<Vec<String>>` (required when TextClassification). Unit-testable without a
     model (dispatch logic + manifest parsing).
   - **B-29b — unified model abstraction** (epic, L3): let the engine registry hold
     both GGUF and ONNX models (common trait or enum), so ONNX inference is reachable
     through `Runtime`/FFI/Python. This is the real enabler; larger than B-29's
     original framing.
2. **Manifest**: add `labels: Option<Vec<String>>` (+ validate present when
   `TextClassification`). Tokenizer path stays sibling-convention (B-28); no manifest
   field needed.
3. **Honest scoping note**: B-29a alone does NOT make ONNX servable end-to-end (F2).
   If the goal is usable ONNX inference, B-29b is required and should be sequenced
   with (or ahead of) B-29a. If the goal is only the literal "manifest-driven
   selection over the loaders," B-29a suffices and B-29b is a tracked follow-up.

## Updated Knowledge (Shadow Genome)

New pattern: **"unwired surface hides a deeper unwiring."** A backlog item framed as
"add selection over X" assumed X was reachable; investigation found X (ONNX loaders)
has no production caller AND no registry type to live in. Always trace the item's
prerequisite all the way to a live entry point before scoping — the stated gap can be
the visible tip of a larger architectural gap.

---

_Research complete. Findings advisory; the scope fork (B-29a vs B-29a+B-29b) is an
operator decision at cycle start._
