# Cold-switch transition coordinator: plan (#137 / #153)

**Risk:** L2 (model lifecycle and memory authority; not authentication).
**Base:** `main` `1032d1ba` plus Branch A tests (`3c582e65`).
**Status:** plan for audit before implementation.

**Policy boundary:** #153 still owes owner decisions on drain, cancellation, warm overlap and rollback. This change implements the conservative cold-switch prototype from `V1_STANDALONE_SWITCH_ACCEPTANCE.md`:
- no implicit preemption;
- no cross-model KV handoff;
- no assumed reclamation;
- no silent fallback.

It is **not exposed over IPC by default** (see "Exposure").

## What exists, and what is not reused
- **Kept as-is:** `ModelLifecycle::{load_reserved, unload, wait_released}` and `model_ops::prepare_model_admitted_for_workers`, which provides path confinement, the manifest and sha256 check, and the budget reservation before backend allocation. These are the canonical, qualified paths (#183, #193).
- **Not used:** `ModelPool::switch_to` and `SwapManager::execute_swap` are not physical transitions, and this design does not use them.
- **Gap:** there is no mutation authority spanning several steps. IPC load runs `prepare` (reserve budget, backend load) and then `register`, with no lock across the two. A concurrent load during a switch could therefore take the budget that A's release returned.

## Design
1. **One mutation authority.**
   - `ModelLifecycle` gains `operations: tokio::sync::Mutex<()>`, with `begin_operation()` (waits) and `try_begin_operation()` (refuses when busy).
   - The IPC load handler holds it across prepare and register. The IPC unload handler and daemon preload also hold it.
   - The coordinator holds it for the whole transition and uses `try_begin_operation`, so a second concurrent operator is refused (`Busy`), never queued behind a long switch.
   - Plain `lifecycle.load`/`unload` stay lock-free primitives for in-process and embedded callers, as today. The authority belongs to the operations layer.
2. **`models/transition.rs`**, crate-private (`pub(crate)`):
   - `ColdSwitchRequest { from_id, expected_handle: u64, from_path, to_path, to_id, release_wait: Duration }`;
   - `cold_switch(lifecycle, loader, request) -> Result<SwitchReport, SwitchError>`.
3. **Order of the switch.** Every refusal before step e leaves A serving and untouched.
   - a. Acquire the authority (`Busy` if held).
   - b. **Preconditions:**
     - `from_id` is loaded with exactly `expected_handle`, otherwise `StaleHandle` or `NotLoaded`;
     - `to_id != from_id`, and `to_id` is not loaded (`AlreadyLoaded`);
     - `release_wait <= MAX_RELEASE_WAIT`.
   - c. **Preflight both paths without disruption** (`preflight(loader, path)`):
     - `validate_path`, so a prohibited path is refused;
     - `load_metadata`;
     - `read_sibling_manifest`, which must be valid if present;
     - `choose_backend`, refusing unsupported backends;
     - a capability check (GGUF: text generation; ONNX: a supported plan);
     - the manifest sha256 when declared;
     - **budget fit:** `size_bytes <= total budget`, otherwise B could never fit even after A's release.
     The same preflight runs on `from_path`, so a rollback target is known to be valid before A is touched.
   - d. **Close admissions to A:** `lifecycle.unload(from_id)`. New requests get `NotLoaded`. In-flight and streaming requests continue to completion (no preemption).
   - e. **Bounded wait:** `wait_released(from_id, release_wait)`. On timeout, return `SwitchError::ReleasePending { pending }`. A is unloaded and still draining, B is not loaded, and the report says so. Nothing is reloaded, because A's bytes are still held and reloading would double the charge. The operator may retry `wait_released`, then load.
   - f. **Load B:** `prepare_model_admitted_for_workers(to_path)` under `spawn_blocking`, then `register_prepared`. This re-verifies path, manifest, hash and budget, closing the gap between preflight and use.
   - g. **Readiness:** `validate_handle(B)`.
   - h. **Recovery if f or g fails:** reload A from `from_path` through the same canonical path, under a new handle. Return `SwitchError::RolledBack { cause, restored_handle }`, or `RollbackFailed { cause, rollback_error }`. The report names exactly what is loaded.
4. **`SwitchReport`:** `{ from_handle, to_handle, release_ms, load_ms }`. Handles are never reused.
5. **Disconnects and retries.** The coordinator runs to completion inside its own task. A dropped caller cannot leave the authority held, because the guard is dropped at the end of the task. A retry after `ReleasePending` is an ordinary unload-wait plus load.

## Exposure
- **No IPC message in this change.** `#153` approval is needed before a public `model_transition_request`.
- **Real-model qualification** drives `cold_switch` in-process, the way the existing real-model lib tests drive the backend (see the acceptance plan). It is labelled in-process, not authenticated IPC.

## Tests
**Model-free**, with the lifecycle and fake weights. Each must hold:
- A→B→A success with new handles and budget accounting;
- stale handle refused, with A untouched;
- wrong `from_id` refused;
- `to_id` already loaded refused;
- prohibited, missing, or bad-manifest target refused before disruption;
- target larger than the total budget refused before disruption;
- release timeout gives `ReleasePending` with B not loaded and no double charge;
- B load failure triggers rollback to A, with a new handle and an accurate report;
- rollback failure is reported;
- a concurrent operator gets `Busy`;
- the authority blocks a concurrent IPC-style load during the switch.

**Real-model** (lib test, opt-in `GG_N7_MEASURE`-style gate, fixtures hash-pinned):
- Qwen→SmolLM2→Qwen and Qwen→Phi-3.5→Qwen;
- greedy output before and after (A's output identical after returning);
- an in-flight request held across the unload, which must complete exactly once;
- process private bytes and handle count after each step;
- release and load timings.

## Razor
`transition.rs` stays at 250 lines or fewer, split into preflight and coordinator sidecars if needed. Functions stay at 40 lines or fewer.

## Not in scope
- IPC exposure.
- Warm overlap.
- Preemption of in-flight work.
- GGUF↔ONNX switching. The same code path serves it, but real-model ONNX switching is tested only if fixtures load.
- Owner policy decisions (listed in the report).

## Revision 2: remediation of audit VETO (findings 1–11)

### Mutation authority and recorded source (findings 1, 4, 5, 9, 10)
- **New sidecar `models/lifecycle_operations.rs`**, so `lifecycle.rs` stays under 250 lines. `ModelLifecycle` gains two fields:
  - `operations: Arc<tokio::sync::Mutex<()>>`;
  - `sources: std::sync::Mutex<HashMap<u64, ModelSource>>`, keyed by handle.
- **Public API, documented as advisory:**
  - `try_begin_operation() -> Option<OwnedMutexGuard<()>>`;
  - `begin_operation_within(Duration) -> Option<OwnedMutexGuard<()>>`, bounded and returning `None` as busy;
  - `record_source(handle, ModelSource)` and `source(handle) -> Option<ModelSource>`, with the entry removed when the handle unloads;
  - `unload_expected(id, handle)`, which unloads only if `id` still maps to exactly `handle`, checked under the index write lock;
  - `resident_bytes_excluding(id)`, the sum of the other residents' guard bytes.
- **Scope:** the authority protects ordering and liveness. Memory safety still comes from the budget, which every path, the bypassing ones included, enforces. FFI, Python and the plain `lifecycle.load`/`load_model_from_path` primitives stay unlocked, as documented in the rustdoc.
- **`ModelSource { relative_path, sha256 }`:**
  - Recorded by the operations-layer load paths (IPC load, daemon preload) right after registration.
  - The sha256 is the manifest's declared digest, already verified at load, when present. Otherwise it is computed once by `integrity::hash_file` on the same blocking thread. That costs one extra read for manifest-free GGUF, paid at load time and never during a switch.
- **New lib function `model_ops::load_model_operated(lifecycle, loader, path, id, workers, wait)`:**
  - acquires the authority within `wait` (refusing with `Busy` otherwise);
  - runs prepare under `spawn_blocking`;
  - registers;
  - records the source.
- **Who uses it:** daemon preload (binary crate) and the IPC load handler call it. IPC unload uses `begin_operation_within(30 s)` and then `unload`. IPC waits are therefore bounded at **30 s**, after which they return a clear busy error.

### Coordinator: own task, owned guard, seam (findings 2, 3, 7, 8)
- **Interface:** `transition::spawn_cold_switch(lifecycle: Arc<ModelLifecycle>, prep: Arc<dyn Preparer>, req) -> Result<JoinHandle<SwitchOutcome>, SwitchError::Busy>`.
  - It takes the authority with `try_begin_operation` before spawning.
  - The spawned task owns the guard and every step. Dropping the caller or the `JoinHandle` cannot interrupt a transition or leave the lock held.
- **`Preparer` trait** (crate-private):
  - `preflight(path) -> Result<TargetFacts, ModelOpError>`, where `TargetFacts` is `{ size_bytes, sha256: Option<String> }`;
  - `prepare(path, id, limits) -> Result<PreparedModel, ModelOpError>`;
  - `source_hash(path) -> Result<String, ModelOpError>`.
  - `CanonicalPreparer { loader, workers }` delegates to `validate_path`, `load_metadata`, `read_sibling_manifest`, `choose_backend`, the capability checks, `prepare_model_admitted_for_workers` and `hash_file`.
  - Tests inject a `FakePreparer` that returns `Weights`, errors or chosen sizes.
- **Preflight runs under `spawn_blocking`.**
  - **A** (the rollback source) is *not* re-hashed in preflight. Its recorded `ModelSource` is required instead: `UnknownSource` is refused before disruption. Its path must still validate.
  - **B** is preflighted: path, metadata, manifest and backend checks, with the hash only when a manifest declares one.
  - **Cost:** at most one full hash of B during preflight, one of B again in prepare (when a manifest declares it), and one of A during rollback only. All of it happens while holding the authority, and the plan says so.
- **Budget estimate** (finding 6): refuse before disruption if `B.size_bytes > total − resident_bytes_excluding(from_id)`. This is labelled an estimate. The canonical registration re-checks after resizing to `memory_usage`.
- **Readiness** (finding 8): this is a **registration check** (`validate_handle`), not a smoke inference. A smoke inference is listed as an open #153 decision.
- **Recovery:** if B was registered but the check failed, `unload_expected(B)`, wait for its release (bounded by the request's `release_wait`), and only then reload A.

### Rollback bound to the real A (finding 1)
- **Normal path:** rollback reloads A only from its recorded `relative_path`. After `prepare`, it requires `source_hash(path) == recorded sha256`. On a mismatch, it unloads that prepare (dropping it), returns `RollbackFailed { cause, rollback_error: SourceChanged }`, and reports that neither model is loaded.
- **Missing record:** the switch is refused before any disruption (`UnknownSource`).

### Step order (replaces 3a–3h)
1. `try_begin_operation`, then spawn.
2. Preconditions:
   - `get_handle(from_id) == expected_handle` (`StaleHandle` / `NotLoaded`);
   - a recorded source for that handle (`UnknownSource`);
   - `to_id` neither equal to `from_id` nor loaded;
   - wait ≤ `MAX_RELEASE_WAIT`.
3. Preflight under `spawn_blocking`: B's facts, A's path still valid, and the budget estimate.
4. `unload_expected(from_id, expected_handle)`. Queued requests that have not started then resolve to `NotLoaded`; in-flight and streaming requests continue.
5. `wait_released(from_id, wait)`. On timeout, return `ReleasePending { from_handle, pending }`, with B not loaded and nothing reloaded. This is listed as an open #153 decision because it is an outage.
6. Prepare and register B, then record its source and run the registration check.
7. On failure in step 6, recover as described above and return `RolledBack { cause, restored_handle }` or `RollbackFailed`.
8. Return `Switched { from_handle, to_handle, release_ms, load_ms }`.

### Tests (finding 3 conditions; model-free with `FakePreparer` unless noted)
1. A→B→A success: new handles, budget, sources recorded.
2. Stale handle, wrong `from_id`, `to_id` already loaded, `to_id == from_id`: all refused with A untouched.
3. Unrecorded source: refused before disruption.
4. Target over the estimate: refused before disruption.
5. Target preflight failure (prohibited path, bad manifest, unsupported backend): refused before disruption.
6. Release timeout: `ReleasePending`, B not loaded, budget still charged only for A.
7. B prepare failure: rollback restores A with a new handle and the same sha.
8. Register resize refusal: rollback.
9. B registered but failing the check: unload B, wait, restore A.
10. Rollback source changed: `RollbackFailed`, with the report stating that neither model is loaded.
11. Concurrent operator: `Busy`.
12. `load_model_operated` during a switch: `Busy` within its bound.
13. Caller dropped right after spawn: the transition still completes and the lock is released.
14. `unload_expected` with a mismatched handle is refused.

**Real-model** tests are unchanged from Revision 1.

### Exposure and policy (finding 11)
- There is no IPC message.
- **Open #153 decisions:**
  - the `ReleasePending` outage, versus draining before closing admissions;
  - a smoke-inference readiness check;
  - preemption or cancellation of in-flight work after a deadline;
  - warm overlap.

## Audit record
- **Revision 1: VETO.** Findings 1–11, chiefly:
  - the rollback source was unbound;
  - the switch was cancellable mid-transition;
  - there was no prepare seam;
  - preload was unreachable;
  - the authority was advisory;
  - the budget preflight was weak;
  - hashing was costly;
  - "readiness" was overstated;
  - Razor;
  - IPC waits were unbounded;
  - the `ReleasePending` outage was not flagged.
- **Revision 2: PASS WITH CONDITIONS.** All of the above are addressed in substance. Binding conditions:
  - **R1:** the source hash is identity-bound. On both the record and rollback paths, `hash_file` runs before the load, then `check_identity_unchanged` after it. The rollback comparison happens before registration.
  - **R2:** `lifecycle.rs` stays at 250 lines or fewer, with a single `OpsState` field and the source entry removed inside `unload`.
  - **R3:** a release timeout during recovery returns `RollbackFailed(ReleasePending)`, covered by test 15.
  - **R4:** the record-time hash of B is stated and measured. Three more tests: a bypass unload drops the source; `resident_bytes_excluding` with another resident; the IPC unload busy bound.
  - **R5:** a `JoinError` maps to an explicit unknown-state report.

## Independent review (2026-11-14): APPROVE WITH CONDITIONS, all addressed

| Finding | Severity | Resolution |
|---|---|---|
| 1. Source recorded against a re-looked-up handle | Medium | Sources are recorded against the handle `register_prepared` returned. `record_source` inserts only while that handle is still in the index (checked under the index lock). Test: a source never attaches to an unloaded handle. |
| 2. Rollback parsed a substituted file | Medium | `prepare_sourced(…, expected_sha256)` refuses a mismatched digest right after hashing, before any backend reads the file. |
| 3. Manifest models hashed twice | Medium | The manifest's verified digest is reused, and dispatch still verifies it. Manifest-free GGUF is hashed once. The cost and the 30 s busy error are documented in `IPC_PROTOCOL_SCHEMA.md`. |
| 4. Missing tests | Medium | Added `load_operated` busy and source-recording tests. Preload and IPC now share `transition::load_operated`, so a fake can exercise it. |
| 5. Optimistic budget estimate | Low | The estimate is now `total − (current_memory − A's charge)`, which counts draining releases and working memory. |
| 6. Fragile `verify_ready` keying | Low | Readiness is keyed by `Arc` address. |
| 7. Real test did not prove an in-flight request | Low | It runs a 96-token in-flight request and asserts that the switch's release waited at least 100 ms. Memory probes are measurements, not assertions. |
| 8. Public API | Low | The authority, source, `unload_expected` and charge APIs are `pub(crate)`. Only `load_model_operated`/`unload_model_operated` are public, for the daemon binary. |
| 9. `AlreadyLoaded` during recovery | Low | Reported distinctly: another instance serves, but it is not the verified source. |
| 10. Identity check limits | Low | Documented as a residual limit in `CanonicalPreparer::prepare_sourced`: identity is length plus file id, and the `models/` tree is trusted. |
