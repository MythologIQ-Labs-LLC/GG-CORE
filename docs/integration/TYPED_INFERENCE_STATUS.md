# v1.0.0 typed inference engine: staged ONNX integration

**Status: developmental source-only foundation. Not a supported IPC, SDK, CLI or product feature yet.**
Related: [#171 typed ONNX IPC](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/171), [#169 backend/manifest correctness](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/169), [#160 model matrix](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/160) and [#161 release gate](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/161).

The runtime is **model-agnostic**. Qwen is only the owner's preferred first GGUF quality fixture. The existing `Arc<dyn Model>` can hold GGUF text generators and ONNX embedding/classification models, but the shipped queue, worker and IPC schema currently support **generation-shaped responses only**. A successful ONNX model load is not proof that embeddings/classification can be served through authenticated IPC.

## Source increment

New crate-private `InferenceEngine::run_typed_controlled` in `core-runtime/src/engine/inference_typed.rs` permits **only Embedding and TextClassification** tasks, when the selected model declares the requested capability. The existing text-generation path remains unchanged.

It validates text/batch inputs, rejects unsupported shapes (including ambiguous classifier batch), enforces model existence, per-call memory budget, context limits, typed output shape, finite embedding values, bounded embedding dimensionality, finite label confidences, and per-request cancellation/deadline both **before and after** the native backend call. The function never silently encodes non-text results into a generation response.

**Critical security limit:** the typed function is *crate-private* and **not connected to any external interface**. Calling it directly does not run the production worker's ingress scanner, egress scanner, queue/admission, session authentication, or request-owner lifecycle. It must **NOT be exposed over IPC, FFI, HTTP, Python or other consumers** until those invariants are implemented together. Backends may not guarantee mid-call cancellation; a late return is converted to an error, but an uncooperative backend still needs runtime/OS execution bounds.

The draft adds mock-based unit regressions under `core-runtime/src/engine/inference_typed_tests.rs` for valid single/batched embeddings and classification, task/model mismatch, wrong output type, invalid dimensions/NaNs, invalid labels, missing inputs, memory bounds, and cancellation. These tests have **not** been run with a Rust toolchain in this authoring session.

## Next minimum viable implementation, without weakening containment

1. **Version/capability negotiation:** Preserve the frozen v0.10.0-rc.1 `inference_request`/response wire contract unchanged. Add an opt-in, authenticated *typed* IPC opcode with explicit `task`, a bounded text or text-batch input, a typed result enum and a consistent error. No generation-shaped embedding vectors, fallback to the wrong model, or unrequested task promotion.
2. **Production queue path:** Introduce typed requests into the SAME bounded scheduler/admission, cancel/deadline and worker security pipeline used by GGUF. Ingress must check **every input item** with the same policy, and **every output** must be filtered/logged/redacted under explicit typed safety rules; avoid a direct `handler → engine` bypass.
3. **Privacy and frame-size security:** Bound vectors, batches and label text before serialization. Reject overflow, NaN/Infinity, invalid dimensions/counts and outputs exceeding IPC `max_frame_size`. Authenticate metadata and capability negotiation appropriately. Do not write vectors, raw prompts, labels, paths or secrets to default logs.
4. **Consumer interfaces:** A standalone `embed` and `classify` CLI or native SDK call should use typed, versioned IPC. There is **no requirement for public HTTP**, and the inference daemon must have **no outbound downloader/network privilege**.
5. **Real backend acceptance:** Pin licensed ONNX embedding and classification weights with SHA-256, tokenizer, ordered labels and exact runtime architecture. Execute clean installed GG-CORE with IPC auth enabled; verify real vector values, class labels and unsupported model/task failures. Repeat model load/unload/reload and GGUF↔ONNX transition with measured RSS, memory reclamation, queue/cancel and threat-model negatives.
6. **Backward compatibility:** Existing GGUF text-generation `infer` CLI, streaming chunks, timeout/error codes, downstream CoreForge pin, and published RC1 continue working unmodified. Do not call ONNX text generation supported until [#115](https://github.com/MythologIQ-Labs-LLC/GG-CORE/issues/115) has actual decode support and acceptance.
7. **Independent validation:** Native Rust `cargo fmt --check`, `cargo test --locked`, relevant feature combinations (`onnx`, `gguf`, `ffi`), strict clippy, Windows/Linux/macOS per support matrix, redacted local sandbox evidence. No CI charges or release mutation unless separately authorized.

## Qualification disposition

This source increment establishes **backend-neutral typed inference semantics**, not public ONNX interoperability. It intentionally fails closed if task, model capability, input shape or output type do not agree. **NO-GO for claiming model-agnostic ONNX serving until the queue, IPC, CLI/SDK, privacy and packaged real-model evidence above pass.**

The existing frozen v0.10.0-rc.1 release and CoreForge consumer pins must remain untouched. Owner expects a complete, long-lived GG-CORE v1.0.0, not a narrower product defined around one preferred model.
