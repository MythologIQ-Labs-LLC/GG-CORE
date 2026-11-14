# Authenticated capability snapshot and explained backend eligibility: plan (#49 / #50)

**Risk:** L2. This adds one authenticated read-only IPC frame pair and a pure eligibility function. It has no model-execution or authority change.
**Base:** branch `feat/v1-backend-capability-contract-20261113-no-ci` at `fe29287`: the typed, `#[non_exhaustive]`, Serialize-only capability schema with fail-closed selection.

## Goal
Expose truthful, versioned capability data over **authenticated** IPC without breaking existing clients or Rust consumers. Keep separate:
- compiled paths;
- actually loaded model capabilities;
- detected hardware;
- exact model/task/backend/target cells;
- evidence-qualified support;
- operator constraints.

## Design
1. **`CapabilitySnapshot`** (new `models/capability_snapshot.rs`, `#[non_exhaustive]`, Serialize-only):

   | Field | Content |
   |---|---|
   | `schema_version` | `CAPABILITY_SCHEMA_VERSION` (1) |
   | `runtime_version` | crate version |
   | `compiled` | `compiled_backend_capabilities()`, unchanged |
   | `loaded` | `Vec<LoadedModelCapability { model_id, handle_id, backend: Option<BackendKind>, paths: Vec<ServingPath> }>` |
   | `hardware` | `HardwareFacts { logical_cpus, accelerators_detected: Vec<String> }`, documented as **detection only, never a serving claim** |
   | `qualified` | `Vec<ServingPath>`, always empty until a trusted evidence registry exists |
   | `operator` | `OperatorConstraints { offline_only: true, network_listener: false, auth_required_for_mutation: true, max_total_memory, max_memory_per_call: Option<usize> }` |

   **Loaded model capabilities:**
   - `backend` is derived by downcasting the actual model object (`as_any`) to the GGUF or ONNX backend type, behind `cfg` features. It is `None` if unknown and never guessed.
   - `paths` are the model's own `capabilities()` mapped to `InferenceTask`, each on target `Cpu`. That is the only target any backend serves today. `NamedEntityRecognition` and anything else unmapped is omitted, not invented.
2. **IPC:**
   - Additive frames `capabilities_request` and `capabilities_response { snapshot }`. `IpcMessage` is `#[non_exhaustive]` (#112), so Rust consumers are unaffected, and old clients never send the frame.
   - **Authenticated:** it goes through `require_auth`. Capability and operator data is reconnaissance-relevant, so it is not added to the unauthenticated probe set.
   - **Read-only:** no request field can set or alter any claim. The response types have no `Deserialize`, so the server can never ingest client-supplied capability metadata.
3. **Explained eligibility.** `evaluate_backends(catalog, requirement) -> Eligibility { selected: Option<BackendKind>, rejections: Vec<(BackendKind, Rejection)> }`:
   - `Rejection` values: `SchemaVersion`, `PathNotCompiled`, `PathNotQualified`, `NoStreaming`, `NotPinned`, `Ambiguous`.
   - `select_qualified_backend` keeps its signature, implemented on top of the new function.
   - **No silent substitution:** a pinned backend is never swapped, and on ambiguity nothing is selected.
   - **No authority increase:** the result is informational. It loads nothing and raises no budget.
   - **No oscillation:** the function is pure and stateless with no scoring, so identical inputs always give the identical result.
   - **Out of scope here:** model selection, as opposed to backend eligibility. That needs #153 and #50 policy and is not implemented.
4. **CLI:** none in this step. The frame is reachable from authenticated IPC clients.

## Tests
- **Snapshot:**
  - `compiled` matches the feature matrix;
  - `qualified` is empty;
  - `hardware` never contributes to any path;
  - a loaded fake model with `TextGeneration` yields exactly one `(text_generation, cpu)` path, and `backend: None` for a non-backend fake;
  - `NamedEntityRecognition` is omitted;
  - operator constraints reflect the configured limits;
  - the JSON has `schema_version` and contains no `cuda` or `metal` serving path.
- **IPC** (through the real `handle_connection`, model-free daemon):
  - unauthenticated `capabilities_request` → 401, and the connection stays open;
  - authenticated → `capabilities_response` with the loaded stub listed;
  - the frame is listed in the `protocol_compat` variant guard.
- **Eligibility:** each rejection reason, with ambiguity and a pinned backend never substituted. Existing selection tests stay unchanged.

## Razor and compatibility
- New files stay at 250 lines or fewer; `handler.rs` (already over 250) gets only a dispatch line that delegates to a sidecar.
- `protocol_types.rs` gains 2 variants.
- The doctest and in-crate guard lists are updated, as `#112` requires.
- `IPC_PROTOCOL_SCHEMA.md` documents the frames as additive V1.

## Not in scope
- An evidence registry populating `qualified`.
- GPU serving.
- Model auto-selection.
- A CLI command.

## Revision 2: remediation of audit VETO (C1–C8)

**C1: wire payload and the trust boundary.** `IpcMessage` derives `Deserialize`, so it cannot carry a Serialize-only type.
- `capabilities_response { snapshot: serde_json::Value }` carries an **opaque JSON value**. The server builds it only by serializing the Serialize-only `CapabilitySnapshot`.
- The authority types (`RuntimeBackendCapabilities`, `ServingPath`, `CapabilitySnapshot`) stay output-only.
- **Restated invariant:** the server never deserializes inbound capability data into authority types, and never treats it as authority.
- Clients decode the JSON with their own types. A typed Rust client mirror is out of scope.
- **Test:** a client-sent `capabilities_response` hits the existing default arm and gets `error{code:400}` (connection closed, per the current policy), with no state change.

**C2: authentication.** The test follows the real policy:
- **Unauthenticated:** `error{code:401}`, then the connection closes.
- **`require_auth = false`** (`IpcHandlerConfig`): the frame answers without a session, and `operator.auth_required` reports `false`.

**C3: model inspection and truthful paths.**
- **Accessor:** a crate-private `InferenceEngine::inspect_models(|id, &dyn Model|)` visits each registered model under a single read lock and returns no `Arc`.
- **Backend:** found by downcasting to `GgufGenerator` (`cfg(feature = "gguf")`), `OnnxEmbedder` or `OnnxClassifier` (`cfg(feature = "onnx")`). Anything else is `None`.
- **Paths:** with a known backend, the model's declared capabilities mapped to `InferenceTask` on `Cpu`, **intersected with that backend's `compiled_paths`**. With `backend: None`, they appear in a separate `declared_only_paths` field, never in `paths`.
- **Handles:** the registry supplies the handle id.
- **Consistency:** the snapshot reads the registry first, then the engine. It reports only models present in both (registered and serving). A model present in only one is omitted, which is not a serving claim. Instances that are releasing are not capabilities, so they are left out; `models_request` already lists them as `unloading`.

**C4: memory budgets.** These are **deferred in writing**. Backend eligibility does not know a model's resident or working size. The budget, together with host authority and model selection, belongs to model selection under #50/#153. Until then, `Eligibility` never selects a model or raises any budget, and the existing admission and budget checks apply unchanged. The deferral is recorded in `V1_BACKEND_CAPABILITY_CONTRACT.md`.

**C5: operator fields from real configuration.**

| Field | Source |
|---|---|
| `auth_required` | `IpcHandlerConfig.require_auth` |
| `budget` | `Option<{ max_total_memory, per_call_ceiling: Option<usize> }>`, from `lifecycle.resource_limits()` and `ResourceLimits::max_total_memory()`; `None` means no budget configured (embedded, unlimited) |
| `offline_only`, `network_listener: false` | true compile-time facts of this runtime, documented as such |

**C6: hardware.** `HardwareFacts { logical_cpus }` comes from `std::thread::available_parallelism`. Accelerator kinds are **deferred**: the field `accelerators: Option<Vec<String>>` is always `None`, because `GpuManager` is not reachable and is feature-gated. A test asserts that `hardware` never feeds `paths` or `qualified`.

**C7: compatibility and documentation.**
- The two variants are added to `protocol_types.rs` (two lines plus attributes). The payload types go in a new `ipc/protocol_capability_types.rs`.
- The `protocol_compat` doctest and the in-crate guard each go to 30 variants.
- `IPC_PROTOCOL_SCHEMA.md` documents the additive frames. It states that `snapshot.schema_version` is the **capability schema version (1)**, not the IPC protocol version.

**C8: Razor.**
- New files stay at 250 lines or fewer and functions at 40 or fewer.
- `handler.rs` gets only the dispatch arm, and the work lives in a sidecar (`handler_capabilities.rs`).
- Existing size violations, recorded and not grown: `protocol_types.rs` (368 lines; it gains only the variant lines) and `handler.rs` (491 lines).

**Disclosure (finding 8):** the snapshot contains no filesystem paths, device serials or session data. Model ids and handle ids are already in `models_request`.

## Audit record
- **Revision 1: VETO.** Findings:
  - the Serialize-only types could not be carried by a `Deserialize` enum;
  - the unauthenticated-close expectation was wrong;
  - there was no model inspection accessor;
  - loaded paths trusted the model's own declaration;
  - memory budgets were unaddressed;
  - operator values were hardcoded;
  - the hardware source was undefined;
  - Razor.
- **Revision 2: PASS WITH CONDITIONS.** Binding conditions P1–P5, all met:

| Condition | Required | Implementation |
|---|---|---|
| P1 | Client-sent response | `error{code:400}`; the connection stays open; a follow-up `capabilities_request` succeeds with no claim ingested |
| P2 | Joining and locking | The model id is joined to the handle through the lifecycle after the engine lock is released; the visitor is synchronous and holds only the engine read lock |
| P3 | Budget accessor | `ResourceLimits::max_total_memory()` already exists (`memory/resident.rs`); tests cover a configured budget and `None` |
| P4 | Written deferral | Committed to `V1_BACKEND_CAPABILITY_CONTRACT.md` |
| P5 | Razor | `handler.rs` grows only by the dispatch arm and `protocol_types.rs` only by the variant lines; the capability test module is declared from the test daemon file |

