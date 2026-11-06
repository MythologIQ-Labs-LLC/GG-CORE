# GG-CORE v1.0.0: model-agnostic runtime compatibility contract

**Scope:** implementation and qualification guidance, not an assertion that all model formats or weights already work. **Owner instruction (2026-11-06):** GG-CORE is an independent **model-agnostic inference runtime**. Qwen is a preferred first benchmark target, **not the product's model family, format, or network dependency**. GGUF and ONNX are explicit core goals; model import/acquisition and safe cross-model switching must work over a broad, qualified catalog.

## Separate four independent concerns

1. **Model family and instruct semantics:** Qwen, Llama, Gemma, Phi, Mistral and other license-permitted architectures may require different native kernels, tokenizer implementations and chat templates. Successful file recognition is not successful inference. The [Qwen2.5 opt-in CLI formatter](https://github.com/MythologIQ-Labs-LLC/GG-CORE/pull/168) is an initial compatibility test, not the universal prompt protocol.
2. **File format and backend:** GGUF text-generation through actual llama.cpp bindings; ONNX embedding/classification through the currently implemented manifest-driven path. ONNX text generation remains [research/implementation #115](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/115). SafeTensors appears as an architecture metadata value but **does not have a qualified serving backend**. Native backend capabilities must be advertised honestly rather than converting every non-ONNX file to GGUF.
3. **Lifecycle and resources:** import/register, list/inspect, authorized load, unload, cold or warm tier switch, cancellation/drain, memory reclaim, repeat, recover. The loader is backend-neutral at the `Arc<dyn Model>` boundary but native memory reclamation and concurrent swap remain [#137](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/137) / [#153](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/153) qualification work. Different model weights/architectures imply different CPU/GPU footprints and acceptable latency.
4. **Custody and network authority:** the inference daemon stays air-gapped by default, with authenticated local IPC and no model downloader or HTTP inference port. An independently permissioned, explicit user-initiated **acquisition component** may fetch licensed weights, validate source/publisher/revision/license/format/hash/size, and place them into a controlled local staging area. Offline model import must work with predownloaded weights and all their tokenizer/config dependencies. Nothing is loaded until integrity and path confinement pass.

## Current implementation (source-level; not full v1.0.0 proof)

| Model class | Current backend selection | v1.0.0 acceptance obligation |
| --- | --- | --- |
| Manifest-free GGUF | GGUF default | Qualified real-model text generation on supported CPU and accelerator combinations; fail loudly for unsupported architectures |
| GGUF with verified manifest | GGUF | Verify hash before native read; refuse advertised capabilities the live backend cannot serve; demonstrate multiple GGUF model families/quantizations and their correct templates |
| ONNX embedding | Manifest-driven ONNX embedder | Correct tokenizer and vector semantics on pinned real ONNX weights; secure packaged execution on advertised OS |
| ONNX classification | Manifest-driven classifier with ordered labels | Real licensed classifier, correct label ordering and scoring; explicit rejection of malformed/ambiguous manifests |
| ONNX text generation | **Not implemented in current dispatch** | Fully qualify if included in owner-approved v1 support matrix, without falsely calling embedding support text generation |
| SafeTensors | **No serving backend** | Require explicit supported adapter/conversion route or owner disposition; never silently load through GGUF |
| Other formats | Not a general automatic inference path | Assess usefulness, safe conversion and actual backend support under #160, and advertise only verified combinations |

### Exact source issue addressed in [#169](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/169)

The previous dispatch selected GGUF for **every** manifest other than ONNX, including declared SafeTensors and unsupported GGUF capabilities. The fix routes explicit architectures, rejects unsupported claims before model IO, preserves manifest-free GGUF, and validates ONNX servability with its existing pure planner. This is a **safety/correctness prerequisite** to supporting a large interchangeable model catalog; it does not add a new model backend.

**Known remaining catalog defect:** the legacy loader reads one sibling `manifest.json` per directory, which complicates independently pinned manifests when multiple files share a directory. For broad model management, implement a deterministic **per-model manifest** identity without ambiguous fallback/TOCTOU, and qualify path/hash/license preservation. Do not implicitly accept an arbitrary extension as a supported model.

## Required v1.0.0 cross-family acceptance matrix

Owner to approve precise public support; tests must cover at least:

- **Text generation:** Qwen2.5 Instruct first (authorized 0.5B), then at least two non-Qwen licensed GGUF model families and more than one model size/quantization; verify real model output, chat template correctness, streaming, security, context and CPU/GPU resources. Large 7B/14B model downloads remain separately owner-authorized under the air-gap boundary.
- **ONNX:** at least one real pinned embedding model and one real pinned sequence-classification model, with concrete vector dimension and ordered class labels. Separate real ONNX text-generation acceptance when implemented.
- **Switching:** cross-family GGUF A→B→A and cross-backend GGUF↔ONNX where allowed by host policy, real memory reclaim, bounded peak RAM/VRAM, no unauthorized hot swap or in-flight request corruption.
- **Negative cases:** mismatched manifest format, unsupported capability combinations, corrupted/tampered weights, wrong tokenizer/config, missing files, malicious paths/symlinks, model alias collisions, license/provenance failures, OOM, unauthenticated load/unload, untrusted downloads and misleading advertised supported formats.
- **Artifact evidence:** exact model file hashes, provenance/license, architecture and quantization, tokenizer/chat format, API/IPC schema version, OS/CPU/GPU, security mode, timeout/cancellation, quality/latency/throughput/RAM/VRAM, unload and restart receipts. No GPU or ONNX support claim from library code or synthetic unit tests alone.

**Release requirement:** Final stable `v1.0.0` must support a **broad qualified set** of model families and formats while preserving its air-gapped secure execution design. The owner decides the specific verified support matrix in [#160](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160); [#161](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161) determines final integrated GO. Frozen `v0.10.0-rc.1` and CoreForge pins remain immutable.
