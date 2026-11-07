# Typed embedding/classification over authenticated IPC: implementation plan (#171)

**Risk grade: L3** (the IPC boundary and the authenticated request path). **Gate: `/ql-audit` PASS required before implementation.**
**Base:** PR #172 typed engine foundation. **Scope:** non-generative typed tasks only. GGUF generation and streaming are unchanged. No HTTP, no new transport, no ONNX-specific bypass.

## Protocol (backwards compatible, additive)

New `IpcMessage` variants:
- `typed_inference_request`
- `typed_inference_response`

Existing messages, clients and the frozen RC1 probe keep decoding unchanged.

```json
{"type":"typed_inference_request","request_id":7,"model_id":"embedder",
 "task":"embedding","input":{"text":"..."},"timeout_ms":30000}
{"type":"typed_inference_request","request_id":8,"model_id":"embedder",
 "task":"embedding","input":{"texts":["a","b"]}}
{"type":"typed_inference_request","request_id":9,"model_id":"clf",
 "task":"text_classification","input":{"text":"..."}}

{"type":"typed_inference_response","request_id":7,
 "result":{"kind":"embedding","dimensions":384,"vector":[...]}}
{"type":"typed_inference_response","request_id":9,
 "result":{"kind":"classification","label":"pos","confidence":0.93,
           "all_labels":[{"label":"pos","confidence":0.93}]}}
{"type":"typed_inference_response","request_id":7,"error":"...","error_code":"unsupported_task"}
```

- **`task`:** a closed enum, either `embedding` or `text_classification`. Anything else fails deserialization and returns an explicit error, never a fallback.
- **`input`:** exactly one of `text` or `texts`. The existing `InferenceInput::validate` limits apply: 64 KiB per text, at most 32 items, no empty items. Batch classification is refused, as in #172.
- **Capability negotiation:**
  - `ModelInfo` in `models_response` gains `capabilities: ["text_generation" | "embedding" | "text_classification"]`, with `#[serde(default)]`.
  - Correction found during implementation: `models_request` is deliberately unauthenticated (orchestrator pattern). Capabilities are therefore filled in **only for authenticated sessions**; unauthenticated listing is unchanged.
  - `handshake_ack` gains `features: ["typed_inference_v1"]`, with `#[serde(default)]`.
  - A request for a task the model lacks returns `error_code: unsupported_task` before any model execution.

## Security pipeline: same controls, no bypass

1. **Connection:** `server.rs` routes `typed_inference_request` exactly like `inference_request`.
   - An unauthenticated request gets a 401 and the connection is closed (existing fail-closed rule).
   - Duplicate active request IDs get a 409.
   - The request registers in the per-connection cancel map, so `cancel_request` and disconnect stop it.
   - It runs beside the reader in its own task.
2. **Handler (`handler_typed.rs`):** shutdown tracking; validation of input, shape and timeout; a deadline from `timeout_ms`; cancel-on-drop. `select!` over the response, cancel and deadline, with the same `cancelled` / `deadline_exceeded` codes. It never returns a partial success.
3. **Queue:** a bounded typed lane inside `RequestQueue`.
   - It shares the existing `max_pending` limit with the generation and streaming lanes. A full queue returns `admission_rejected`.
   - `wait_and_dequeue` wakes for typed work in the same way it does for streaming.
4. **Worker (`worker_typed.rs`), in this order:**
   1. **Ingress:** `SecurityPipeline::scan_prompt` on every input text; any block rejects the whole request.
   2. **Stop check:** cancel/deadline before execution.
   3. **Admission:** `acquire_guard` (the existing resource admission and memory reservation).
   4. **Execute:** `InferenceEngine::run_typed_controlled` inside `spawn_blocking`, so a panic becomes an error and the async workers are never blocked.
   5. **Egress:** classification label strings pass the existing egress filter. Vectors are numeric and not text-scanned, but #172 already checks them for finite values and dimensions.
   6. **Telemetry:** success/failure without content.
5. **No content in logs or telemetry:** no prompt text, vectors or labels; only model id, task, counts and latency.

## Shared control logic: no duplication (audit V1 remediation)

- **Handler helpers:**
  - Today `CancelOnDrop`, the stop-to-code mapping (`stopped`) and the enqueue-failure mapping (`enqueue_failure`) are private to `handler_inference.rs`.
  - They become `pub(super)` items in a new `ipc/handler_control.rs`, used by both the generation handler and the typed handler.
  - The generation path's behaviour is unchanged.
- **Ingress decision:** the worker's ingress verdict check, and the rejection message and telemetry, become one `scheduler::worker_ingress::scan_texts(security, model_id, &[&str]) -> Result<(), String>`.
  - The text path calls it with its one prompt; the typed path calls it with every item.
  - The rejection wording and the security event stay identical.
- **Shared execution helpers:** `acquire_guard` and the panic-safe `spawn_blocking` pattern are reused, not copied.

## Response size bound

- Worst case: 32 items × 16,384 dimensions ≈ 524 k floats, about 8 MiB as JSON. That is under the codec's existing 16 MiB `MAX_RESPONSE_SIZE` and the CLI's 16 MiB read limit.
- An encode failure returns an explicit `execution_failed` error frame, never a dropped response.

## CLI (governed typed protocol only)

```text
gg-core-cli embed    --model ID --text T [--text T2 ...] [--timeout-ms MS]
gg-core-cli classify --model ID --text T [--timeout-ms MS]
```

- Both use the existing authenticated client (`CORE_AUTH_TOKEN`) and print the typed JSON result to stdout.
- Exit code is 1 on any error.
- No ad-hoc local model execution.

## Tests (TDD-light)

- **Protocol:** round-trip and rejection of an unknown task, both or neither input fields, and an unknown result kind.
- **Server (Windows and Unix):** unauthenticated typed request gets 401 and the connection closes; authenticated works; duplicate ID gets 409; cancel/disconnect.
- **Worker:** ingress block, capability mismatch, queue full, cancellation before execution, deadline, and panic converted to an error. Uses mock models.
- **Real execution:** the committed tiny ONNX embedder fixture over real IPC, deterministic vector for a known input. A GGUF model asked for `embedding` gets `unsupported_task`.
- **Existing GGUF generation, streaming and frozen IPC control probe:** unchanged.

## Out of scope

- Typed chat
- ONNX text generation
- NER
- Python/FFI typed surfaces
- New model acquisition

Razor: each new file under 250 lines, each function under 40, nesting depth 3 or less.
