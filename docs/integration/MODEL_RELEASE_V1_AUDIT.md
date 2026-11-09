# AUDIT REPORT

**Tribunal Date**: 2026-11-09
**Target**: Verified model release on unload (`docs/integration/MODEL_RELEASE_V1_PLAN.md`, #137 foundation)
**Worktree / Branch**: `C:\dev\mythologiq\ggq\swap`, `feat/v1-unload-reclaim-20261109-no-ci` (base `main` `ae55c97`; plan file untracked)
**Risk Grade**: L2
**Auditor**: The QoreLogic Judge

---

## VERDICT: VETO

---

### Executive Summary

The core mechanism is sound in outline:

- `InferenceEngine` holds the only long-lived `Arc<dyn Model>` (`engine/inference.rs:24`). Every other clone is request-scoped: `inference.rs:152-158`, `inference_streaming.rs:57-63`, and the speculative verifier session, which is a per-call local in `inference_speculative.rs:75-122` and `gguf/adaptive_speculative.rs:47-80`.
- GGUF contexts are per-call (`gguf/backend.rs:343`).
- `ModelPool`, `SwapManager`, `smart_loader` and `FlightTracker` hold no `Arc<dyn Model>`.
- The IPC wire is JSON-only (`ipc/protocol_codec.rs:12-59`). V2 encoding applies to token arrays only (`ipc/encoding.rs:29-81`).
- None of the touched message types use `deny_unknown_fields`. Only `protocol_typed.rs:44,88` do.
- `tokio::sync::watch` is available through the `sync` feature (`Cargo.toml:18`).

The blueprint is still rejected, for these reasons:

- Its central promise, that `released:true` means "native model memory has been freed", overclaims against the repository's own upstream retention evidence.
- Its "deadline-bounded" claim is false for requests sent without `timeout_ms`.
- The "additive, public API unchanged" claim is false for the publicly re-exported `ModelUnloadRequest` struct literal.
- The CLI `--wait-ms` path cannot work with the CLI's 5 s client timeout.
- The wait blocks the connection reader that must carry `cancel_request`.
- The listing entry is underspecified in a way that can add unauthenticated disclosure, and the listing read is non-atomic.
- The proposed test placement breaches the 250-line razor.
- The RC acceptance script that encodes the now-false contract statement is left untouched.

### Audit Results

#### Security Pass

**Result**: FAIL

- No auth stubs, hardcoded secrets or mock auth. `model_unload_request` stays behind `require_auth` (`ipc/handler.rs:229-234`).
- **V1**: `released:true` overclaims memory release, which breaks fail-closed truthfulness.
- **V2**: the `released:false` contract asserts a deadline bound that not every request has.
- **V5**: the wait runs inline in the connection reader, so that connection's control plane stalls.
- **V6**: the unauthenticated listing entry is underspecified and can disclose a caller-chosen id.

#### Ghost UI Pass

**Result**: FAIL

- **V4**: the plan specifies the CLI flag `models unload --wait-ms N`, but the CLI transport cannot deliver its result for any N greater than 5000 ms. The CLI then reports a connection failure (exit 3) for an unload that succeeded.

#### Section 4 Razor Pass

**Result**: FAIL

| Check | Limit | Blueprint Proposes | Status |
|---|---|---|---|
| Max function lines | 40 | New functions "under 40" (plausible for `unload_and_wait`, `releasing`, `release`) | OK |
| Max file lines (prod) | 250 | `lifecycle.rs` 224 -> "about 248", with a conditional sidecar; `lifecycle_resident.rs` 74 -> ~140; `handler.rs` 490 +2 (pre-existing) | OK (conditional) |
| Max file lines (tests) | 250 | A1-A9 tests, no file named. The natural homes are `lifecycle_tests.rs` (249), `ipc/protocol_tests.rs` (248), `lifecycle_resident_tests.rs` (152, +6 tests) and `ipc/server_real_model_tests.rs` (204, +A9) | **FAIL (V8)** |
| Max nesting depth | 3 | Stated <= 3 | OK |
| Nested ternaries | 0 | 0 | OK |

#### Dependency Pass

**Result**: PASS

| Package | Justification | <10 Lines Vanilla? | Verdict |
|---|---|---|---|
| `tokio::sync::watch` | Already in the tree: `tokio` `sync` feature (`Cargo.toml:18`), used at `ipc/listener.rs:19` and `runtime_init.rs:285` | n/a (existing) | PASS |

No new crates. No forbidden module (`auth/`, `vault/`, `synapse/`, `plugins/`, `network/`) and no forbidden dependency (`reqwest`, `hyper`, WebSocket). There is no network path.

#### Orphan Pass

**Result**: PASS

- The conditional `lifecycle_release.rs` would attach through `#[path]` in `lifecycle.rs`, as `lifecycle_resident.rs` already does (`lifecycle.rs:215-216`). Connected.
- The `handler_models.rs` sidecar is already wired: `handler.rs:229-234` -> `handler_models.rs:54-62`. Connected.
- `MODEL_RELEASE_V1_EVIDENCE.md` is a documentation artifact and is not in the build path by design.

#### Macro-Level Architecture Pass

**Result**: FAIL

- **V3**: public Rust API breakage, which contradicts "additive" and A11.
- **V7**: the listing has two sources of truth (the registry and the release tracker), read without a common snapshot.
- **V9**: the RC acceptance script still encodes the "load succeeds regardless" behaviour that the plan declares false.
- **V10**: A9 can pass without testing anything, and A10's model B is not provisioned.
- Observation (not scored): `models::FlightTracker` (`models/drain.rs:21-30`, exported at `models/mod.rs:37`) is a second drain primitive that is not wired in. The plan does not reconcile it.

### Violations Found

| ID | Category | Location | Description |
|---|---|---|---|
| V1 | Security / truthfulness | Plan §2.4 (`Some(true)` = "native model memory has been freed") and §3; evidence in `docs/integration/UPSTREAM_LLAMA_CPP_RETENTION_REPRO.md`, step 4 table | The code can only prove two things: the last `Arc<dyn Model>` dropped, and `ResidentGuard::drop` ran (`memory/resident.rs:54-60`). The repository's own filed upstream report (utilityai/llama-cpp-rs#1194) shows about 845 MB still committed after `llama_model_free` when a context init failed at compute-buffer reservation. #176 avoids that path but does not fix it. In that case `released:true` returns the budget while the bytes stay committed, and the next load is admitted into the Job Object commit cap. That is the failure this slice exists to prevent. The plan also leaves undefined what happens when the watch `Sender` is dropped without sending: the task is aborted at runtime shutdown, or the no-runtime leak path runs. Neither the `unload_and_wait` mapping (where `changed()`/`wait_for` return `Err`) nor `releasing()` pruning is specified for that case. |
| V2 | Security / truthfulness | Plan §2.4 (`Some(false)`: "...the request is deadline-bounded") and §3 | False for non-streaming and typed requests sent without `timeout_ms`. `ipc/handler_inference.rs:51-52` sets `deadline = None` and then waits on `std::future::pending()` (lines 75-79). `scheduler/queued_request.rs:52-54` and `engine/request_control.rs:39-42` also leave the deadline unset. `InferenceParams::validate` (`engine/inference_types.rs:88-92`) rejects only `Some(0)`. Only streaming requests default to 30 s (`inference_types.rs:104`). Such a holder ends only on `max_tokens` or context exhaustion, cancel, or disconnect. The wire contract would document a guarantee the runtime does not give. |
| V3 | Macro / public API | Plan §2.4 ("additive") and A11; `core-runtime/src/lib.rs:28` (`pub mod ipc`), `src/ipc/mod.rs:34` (re-export), `src/ipc/protocol_model_types.rs:57-69`; external-crate literals at `core-runtime/tests/model_ipc_test.rs:101-103, 164-166, 262-264`; `src/cli/models_lifecycle_cmd.rs:48-51` and `src/main.rs:169-174` (the bin crate calls `gg_core::cli::models_lifecycle_cmd::run_unload`) | `ModelUnloadRequest` has all-pub fields, no `#[non_exhaustive]` and no constructor. Adding the pub field `wait_for_release_ms` breaks every downstream struct literal, and the repo's own integration test stops compiling. Adding `released` to `ModelUnloadResponse` breaks exhaustive destructuring. The `--wait-ms` plumbing needs either a new signature for the pub `run_unload(socket_path, model_id)` or a new function, and the plan specifies neither. A11 checks only `ModelLifecycle::unload` and `model_ops::unload_model`, so it cannot detect the break it claims to rule out. |
| V4 | Ghost path (CLI) | Plan §2.5 and A10; `src/cli/ipc_client.rs:52` (`timeout_duration: Duration::from_secs(5)`) and `:317` (read timeout); `src/cli/ipc_client_auth.rs:19-23`; `src/cli/models_lifecycle_cmd.rs:83-89` | `models unload <id> --wait-ms N` sends through `CliIpcClient::new(..)`, which cuts the read at 5 s. For N > 5000 the CLI gets `CliError::Timeout` and exits 3 with "Error connecting to GG-CORE server... Is the server running?", although the server has already unloaded the model. Plan §2.5 ("exit code stays 0 ... in both cases") is therefore false, and A10, which drives `--wait-ms`, cannot run as written. |
| V5 | Security / availability (self-DoS of the control plane) | Plan §3 ("a waiting client blocks only itself"); `src/ipc/server.rs:226` (the non-streaming `_ =>` branch awaits `handler.process` inline in the reader loop) and `:197-224` (the same loop handles `cancel_request`); `src/ipc/handler.rs:229-234` | `unload_and_wait` would run inline in the connection's read loop. For up to 120 s that connection cannot read a `cancel_request`, including one for the in-flight stream on the same connection that holds the model being waited on. A client disconnect is not observed either. The plan presents the wait as harmless and does not define this behaviour. No acceptance criterion covers "unload(wait), then cancel on the same connection", which is the natural form of the §7 switch path. |
| V6 | Security / information disclosure | Plan §2.2 (`ReleasingModel { model_id, memory_bytes }`) and §2.4 ("same metadata class ... no new disclosure"); `src/ipc/protocol_types.rs:11-25` (`name`, `format`, `size_bytes` and `loaded_at` are required); `src/models/registry.rs:157-167` (`name` = `metadata.name`); `src/ipc/handler.rs:309-352` (unauthenticated listing; model ids are used only for the authenticated capability lookup at 313-318) | The listing entry is underspecified. `ReleasingModel` has no `name`, `format`, `size_bytes` or `loaded_at`, so the implementer has to invent them. The only identifier available is `model_id`, which the caller chooses on an authenticated load (`--id`). The unauthenticated `models_request` never exposes it today: loaded entries carry the registry's `metadata.name`. Emitting `model_id` as `name` would be a new disclosure, which contradicts §2.4. In addition, several releasing entries would share `handle_id: 0`, which the schema documents as the "internal model handle" (`docs/IPC_PROTOCOL_SCHEMA.md`, Models List table). No criterion pins these fields. |
| V7 | Macro / single source of truth | Plan §2.2 (a tracker "separate from the index") and the §2.4 listing; `src/ipc/handler.rs:310-311` (registry read without the lifecycle lock) | `models_request` would read `model_registry.list_models()` and then `model_lifecycle.releasing()` in two unsynchronised steps. The unload path (`lifecycle.rs:161-177`) removes the model from the registry and, per the plan, pushes it to the tracker under the index lock, which the listing does not take. An unload that lands between the two reads lists the same instance as both `ready` and `unloading` and double-counts `total_memory_bytes`. The plan claims the listing is truthful, and A8 cannot detect this race. |
| V8 | Section 4 Razor | Plan §4 (the A1-A9 test column names no files) and §5 (tests omitted); `src/models/lifecycle_tests.rs` 249 lines, `src/ipc/protocol_tests.rs` 248, `src/models/lifecycle_resident_tests.rs` 152, `src/ipc/server_real_model_tests.rs` 204 | No file is designated for at least 6 lifecycle tests (A1-A6), the protocol tests (A7), a handler test (A8) or a real-model test (A9). Each existing home would pass 250 lines; `lifecycle_resident_tests.rs` plus A1-A6 alone adds well over 100 lines. The razor section audits production files only. |
| V9 | Macro / contract drift | Plan §7 (rewrites contract §7) versus `scripts/rc-ipc-control-acceptance.py:181-204` (`check_switch_after_cancel`); `docs/integration/COREFORGE_PRODUCER_CONTRACT.md:74` | The RC control acceptance script asserts that cancel -> unload -> immediate load returns `success is True`, with no wait. That is the "succeeds regardless" behaviour the plan declares false (§1.2). The plan changes the contract text but does not update the script to use `wait_for_release_ms` or to assert `released`, so the acceptance gate and the contract disagree. |
| V10 | Untestable acceptance | Plan A9 and A10; the repo's `models/` holds only `qwen2.5-0.5b-instruct-q4_k_m.gguf`; no test references SmolLM2 (only the docs `UPSTREAM_LLAMA_CPP_RETENTION_REPRO.md` and `V1_SCHEDULER_HOL_BENCHMARKS.md` do) | A9 needs a second GGUF (SmolLM2) that no fixture path or download script provides, and it "skips without the GGUF magic". It can therefore pass without testing anything in every environment, including the one that records the evidence. Its in-flight window ("stream ... then unload ... load of B refused while A is releasing") has no specified mechanism, such as `max_tokens` or a holder, to guarantee the stream is still running when the unload arrives. In A10, the "within ±16 handles" and "10%" bounds have no stated baseline-sampling procedure, and the ONNX leg has no model-B counterpart. |

### Required Remediation

1. **V1**: Redefine `released:true` as "the runtime's last reference to the model was dropped, the backend destructor ran, and the resident budget was returned". Remove "native model memory has been freed" from the plan, the wire doc and contract §7, and cite `UPSTREAM_LLAMA_CPP_RETENTION_REPRO.md` as a known exception. Specify that a watch channel that closes without a `true` value maps to `released:false` in `unload_and_wait`, and that `releasing()` keeps that entry rather than pruning it. Add a unit test that aborts the release task (or drops the sender) and asserts `false` plus a retained entry.
2. **V2**: Choose one fix. Either cap the holder lifetime for requests without `timeout_ms` (a stated server default deadline, with its own test), or reword `Some(false)` and the docs to state the true bound: the deadline if one is set, otherwise `max_tokens` or context exhaustion, cancel, or disconnect.
3. **V3**: Specify a non-breaking surface for `ModelUnloadRequest` and `ModelUnloadResponse`, for example constructors such as `ModelUnloadRequest::new(id)` / `with_wait(..)`. List and update every struct-literal site (`tests/model_ipc_test.rs:101,164,262`, `cli/models_lifecycle_cmd.rs:49`), or else declare the break and its semver consequence. Specify the `run_unload` change, for example by keeping the existing signature and adding `run_unload_with_wait`. Extend A11 to bind `run_unload` and to construct the IPC structs the way an external crate does.
4. **V4**: Specify that the CLI client timeout for unload is `wait_ms + margin`, bounded by 120 s plus that margin. Add a CLI-level test in which a wait longer than 5 s returns exit 0 with `release pending` or `released`.
5. **V5**: Specify where the wait runs. Either spawn it beside the reader, as inference requests already are (`server.rs:145-168`), so that `cancel_request` and disconnects stay observable, or document that the connection's control frames are deferred for up to the wait and test that. Add an acceptance criterion: on one connection, stream -> unload(wait=N) -> cancel(stream) completes with `released:true` well before N.
6. **V6**: Specify every `ModelInfo` field of a releasing entry and its source. Copy the registry's `metadata.name`, `format`, `size_bytes` and `loaded_at` into the tracker at unload time, and never emit the caller-chosen `model_id` to unauthenticated callers. Define the `handle_id` value: either the original handle, or a sentinel documented in the schema. Add a handler test asserting that the unauthenticated entry contains no `model_id` that differs from the metadata name.
7. **V7**: Specify an atomic snapshot: read the registry list and the release tracker under the lifecycle index read lock, or through one lifecycle method that returns both. Add a test that runs an unload concurrently with the listing and asserts that no instance appears twice and that `total_memory_bytes` is not double-counted.
8. **V8**: Name a new test file for each group of criteria, for example `models/lifecycle_release_tests.rs`, `ipc/protocol_release_tests.rs` and `ipc/server_release_real_model_tests.rs`. Give each a line estimate under 250 and wire it through `#[cfg(test)] #[path]`.
9. **V9**: Add `check_switch_after_cancel` in `scripts/rc-ipc-control-acceptance.py` to the change set. It should send `wait_for_release_ms` and assert that `released` is present. The load should succeed only when the model is released; when the release is pending under a one-model budget, it should be refused with `ResourceExhausted`.
10. **V10**: Make A9 deterministic and meaningful:
    - Use two instances of the provisioned Qwen fixture, or provision SmolLM2 with a path and sha256.
    - Size the budget arithmetically from the fixture's reported `memory_usage`.
    - Specify the mechanism that holds the stream open.
    - Fail loudly when an opt-in environment variable requests the real-model gate.

    Give A10 a baseline-sampling procedure (settle time and sample count), an ONNX counterpart for model B, and the exact CLI output strings it parses.

### Verdict Hash

The hash below is SHA-256 over every byte of this file before the line `### Verdict Hash` (UTF-8, LF line endings), computed with `sha256sum`.

```
SHA256(this_report) = 9b3d3f52f133c94a1f49662813b98b5b70fa6436f86a9d21949fd87f04a1c069
```

---

_This verdict is binding. Implementation may NOT proceed without modification. Ledger entry is deferred to merge per caller instruction; no `META_LEDGER.md`, `SHADOW_GENOME.md` or `.agent/staging/AUDIT_REPORT.md` was written._

---

## Revision 2 Tribunal

**Tribunal Date**: 2026-11-09
**Target**: `docs/integration/MODEL_RELEASE_V1_PLAN.md` revision 2 (§8 maps the rev 1 V1-V10); source unchanged from `ae55c97`
**Risk Grade**: L2
**Auditor**: The QoreLogic Judge

### VERDICT (Revision 2): VETO

### Executive Summary (Revision 2)

Revision 2 closes V1, V2, V3, V4, V6, V8 and V9 as written. It only partly closes V5, V7 and V10. Three of its new claims fail against the code, and two acceptance criteria cannot run as specified:

- **Listing undercount.** The listing can still **hide** a releasing instance. The plan does not fix the order of the tracker push relative to registry unregister.
- **Wait bound.** The "one wait per connection, connections capped" bound does not survive a disconnect.
- **A9.** It asks a stub model to stream, and the streaming path refuses non-GGUF models.
- **A10.** Its 1.5x budget ignores the per-request working-memory reservation that shares the same total, so the first stream is refused.

### Claims verified (Revision 2)

- **Shared writer.** `handle_connection` builds one `Arc<ConnWriter>` (`ipc/server.rs:62`). Spawned streaming, inference and typed tasks already clone it (`server.rs:116-139`, `277-291`; `server_requests.rs:47-75`), so a spawned wait can write through it.
- **Per-connection cap.** A local beside `session`/`active_requests` (`server.rs:63-64`), passed to the sidecar, makes the one-wait cap feasible.
- **Wait authentication.** Session validation can mirror `cancel_request` (`server.rs:197-202`, `handler.auth.validate`).
- **Budget in-process.** `Runtime` builds its lifecycle with `ResourceLimits` (`lib.rs:144`, `lib.rs:170`), and IPC load reserves against them (`handler_models.rs:25-37`, `model_ops.rs:113-116`). An in-process `ResourceExhausted` refusal is reachable.
- **Closed-watch semantics.** These are consistent with `tokio::sync::watch`, `tokio = "1.35"` (`Cargo.toml:18`):
  - `wait_for` checks the current value before reporting closure, so an already-signalled watch (`channel(true)`, sender dropped) reads `true`.
  - An unsignalled dropped sender (the no-runtime path at `lifecycle_resident.rs:62-64`, or a task dropped at shutdown) yields `Err`, which maps to `false`.
  - A7 can run without a Tokio runtime, because `futures = "0.3"` is a dependency and `tokio::sync::RwLock` works on any executor.
- **Dedupe key.** `handle_id` is unique and never reused (`registry.rs:73,80,97`, `next_id.fetch_add`).
- **Snapshot construction.** `LoadedModelInfo` has all-pub fields (`registry.rs:48-58`), so the lifecycle can build the snapshot. There is no `registry.info(handle)` API, so the format must replicate the `"unknown"` that `register` uses (`registry.rs:85-88`).
- **Capabilities.** The capability loop (`handler.rs:313-318`) returns none for a released handle, which matches "no capabilities".
- **V3.** `ModelUnloadRequest`, `ModelUnloadResponse` and `ModelInfo` are untouched. `IpcMessage` is `#[serde(tag = "type")]` and not `#[non_exhaustive]` (`protocol_types.rs:291-293`). Two variants with `rename` attributes add exactly 4 lines, following the pattern at `protocol_types.rs:350-354`. The exposure for exhaustive matches is disclosed.
- **V4.** The CLI helpers `connect_authenticated`, `handshake_on` and `exchange_data` are `pub(super)` in `cli::ipc_client_auth` / `cli::ipc_client` (`ipc_client_auth.rs:27,50,62`; `ipc_client.rs:300`), so `cli::models_lifecycle_cmd` can reach them. A two-exchange `run_unload_with_wait` with `with_timeout(N + 5 s)` (`ipc_client.rs:57`) is feasible without growing `ipc_client.rs` (434 lines).
- **A12.** `tests/public_api_compat_test.rs` is 58 lines, so there is room to extend it.
- **Razor.**
  - The new sidecar estimates (about 60-110 lines each) are credible.
  - `lifecycle.rs` going from 224 to at most 240 is credible. The additions are a field, constructor initialisation, the push in `unload`, the `#[path]` declarations, and the snapshot/always-`Resident` change in `load_reserved`.
  - `handler.rs` +2 is credible: `handle_models_request` only needs its `models` source replaced (`handler.rs:310-311`).

### Audit Results (Revision 2)

| Pass | Result | Findings |
|---|---|---|
| Security | FAIL | R1 (the listing hides held memory); R2 (the wait task outlives the connection, so the stated bound is false) |
| Ghost UI | PASS | The CLI `--wait-ms` path can now deliver its result (V4 closed) |
| Section 4 Razor | PASS | All new files are named and budgeted under 250 lines. Pre-existing oversize files grow by dispatch arms only. Observation: `server.rs` "+4" is optimistic, because rustfmt wraps a 4-5 argument sidecar call to about 6-9 lines. It is still only a dispatch arm. |
| Dependency | PASS | No new crates; `tokio` `sync` and `futures` are already present. No forbidden module or dependency. |
| Orphan | PASS | `lifecycle_release.rs`, `protocol_release_types.rs`, `server_release.rs` and the four test files attach through `#[path]`/`mod`, like their siblings (`lifecycle.rs:215-224`, `server.rs:322-336`) |
| Macro-Level Architecture | FAIL | R3 and R4: acceptance criteria that cannot run as specified |

### Violations Found (Revision 2)

| ID | Category | Location | Description |
|---|---|---|---|
| R1 | Security / truthfulness (V7 residual) | Plan §3.2 ("`unload` ... additionally pushes"), §3.3 dedupe; `models/lifecycle.rs:168-174` (engine unregister, then `registry.unregister`, then index remove, then resident release); `ipc/handler.rs:310` (the listing reads the registry without the lifecycle index lock) | Dedupe by `handle_id` prevents double counting. It does not prevent omission. The plan does not fix the order of the tracker push relative to `registry.unregister`, or the order of the listing's two reads. Following today's `unload` order, the push lands after `registry.unregister` (line 169). A listing that reads the registry in that window (instance already absent) and then the tracker (not yet pushed) omits an instance that is still allocated, and `total_memory_bytes` undercounts it. That is the §1.3 defect this slice exists to fix, and A9 cannot detect it. |
| R2 | Security / resource bound | Plan §3.3 ("bounds spawned tasks to one per connection, and connections are already capped") and §4; `ipc/server.rs:55-60` (`_guard: OwnedConnectionGuard` is dropped when `handle_connection` returns); `server.rs:248-256` (on exit, only the `active_requests` tokens are cancelled) | The spawned wait holds `Arc<ConnWriter>` and is not registered for cancellation when the connection ends. After a disconnect, the connection slot is released while the wait keeps running for up to 120 s. An authenticated client can reconnect and start another. Live wait tasks are therefore bounded by reconnect rate × 120 s, not by `max_connections` (`connections.rs:17`). The stated bound is false. |
| R3 | Untestable acceptance | Plan A9 ("stub model with a controllable holder ... cancels its stream"); `engine/inference_streaming.rs:65-70` (streaming downcasts to `GgufGenerator` and otherwise fails with `"model does not support streaming"`) | A non-GGUF stub cannot hold a stream: the stream fails at once with an error terminal. "Cancel answered during the wait, and the wait completes `released:true`" therefore cannot be exercised as written. The only holder a stub supports is a non-streaming `inference_request`, which is spawned beside the reader (`server.rs:145-168`) and can be cancelled with `cancel_request`. |
| R4 | Untestable acceptance | Plan A10 ("total budget of 1.5x one instance"; steps 1 and 9); `scheduler/worker_admission.rs:12-34` (every generation, streaming and typed request reserves working memory against the same total); `memory/resident.rs:3-7` (one counter for resident and working memory); `lib.rs:114` (default `max_context_length: 4096`); `docs/integration/V1_SCHEDULER_HOL_BENCHMARKS.md:42` (about 0.42 GiB working memory per Qwen context at `n_ctx` 4096) | Qwen2.5-0.5B Q4_K_M weights are about 0.37 GiB. Step 1 needs resident(a) + working(a), about 0.79 GiB, which is more than 1.5 × 0.37 ≈ 0.56 GiB. Working-memory admission refuses the stream on `a` before it starts, and step 9 (inference on `b`) fails the same way. The scenario cannot reach its release assertions. |

### Required Remediation (Revision 2)

1. **R1.**
   - Specify that `unload` pushes the `Releasing` entry into the tracker **before** `registry.unregister`, still under the index write lock.
   - Specify that `models_request` reads the registry **first** and the tracker **second**, with dedupe by `handle_id`. Together these guarantee that each held instance appears exactly once.
   - Add an A9 sub-case that pauses an unload between those two steps (or interleaves them deterministically) and asserts the instance is listed exactly once, with correct `total_memory_bytes`.
2. **R2.**
   - Register each spawned wait under a per-connection `CancellationToken` that `handle_connection` cancels at exit, beside `server.rs:250-252`. It can live in `active_requests` or be a dedicated token.
   - Make the wait return promptly on cancellation, without writing.
   - Add an A9 sub-case that disconnects during a pending wait and asserts the task ends within a short bound, observed through a counter or a join.
3. **R3.** Restate A9's holder as a non-streaming `inference_request` against a stub model whose `infer_cancellable` blocks until its stop check fires. Assert that `cancel_response{cancelled:true}` arrives while the wait is pending, and that the wait then completes with `released:true`.
4. **R4.**
   - Size A10's budget from the runtime's own numbers: `resident(a) + working(a) + resident(b) - 1` for the refusal step, and at least `resident(b) + working(b)` for step 9.
   - Take those numbers from `engine.working_memory_estimate` and the fixture's reported `memory_usage`, or set an explicit small `max_context_length` and show the arithmetic.
   - Check A11 step 4's 1.4 GiB figure with the same arithmetic.

### Verdict Hash (Revision 2)

This is a SHA-256 over every byte of this file before the line that starts with `SHA256(report_through_revision_2)` (UTF-8, LF line endings), computed with `sha256sum`. It covers the full rev 1 record and this section.

```
SHA256(report_through_revision_2) = bdbdcfdcb82846e33e8a1aa50b7081625aa36157399a0cd2de41521e45742e3a
```

_The Revision 2 verdict is binding. Implementation may NOT proceed until the plan is revised. No ledger, Shadow Genome or staging file was written._

---

## Revision 3 Tribunal

**Tribunal Date**: 2026-11-09
**Target**: `docs/integration/MODEL_RELEASE_V1_PLAN.md` §10 "Revision 3 amendments" (R1-R4); source unchanged from `ae55c97`
**Risk Grade**: L2
**Auditor**: The QoreLogic Judge

### VERDICT (Revision 3): VETO

### Executive Summary (Revision 3)

**Design.** The R1 ordering argument is complete, and the R2 token covers every normal exit of `handle_connection`.

**R3 (not closed).** The stub "whose `infer` loops ... until the request's stop check fires" cannot see the stop check. The engine passes the check only to `infer_cancellable`, and that method's default ignores it. The stated three-frame response order is also racy.

**R4 (not closed).**
- A10 step 3 cannot produce `model_not_loaded` under the limits it prescribes. Worker admission runs before model lookup, and a held stream occupies the only concurrency slot.
- On the Windows VM, setting `GG_CORE_MAX_TOTAL_MEMORY` to the A10 budget also sets the Job Object commit cap. That leaves roughly 25 MB for all unbudgeted process commit, which drives the daemon toward the known llama.cpp retention path (#1194).

### Claims verified (Revision 3)

**R1 ordering is complete.** The listing reads the registry first and the tracker second, and dedupes by unique `handle_id` (`registry.rs:97`). Every case lands in at least one read, and in exactly one after dedupe:

- **Loaded and present:** the instance is in the registry read.
- **Unloaded after the registry read:** the push (step 3) precedes the tracker read.
- **Unloaded before the registry read:** the push precedes step 5, so the instance is in the tracker unless already released and pruned. A pruned instance holds no memory.
- **A concurrent load:**
  - `registry.register` runs only after admission succeeds (`lifecycle.rs:133-145`), so a refused load is never listed.
  - A successful load enters the registry before the index (`lifecycle.rs:145-153`), so it appears through the registry read or not at all. It has no tracker entry.
- **Reload of the same id:** it gets a new handle (`registry.rs:97`), so old and new instances never dedupe each other.
- **Synchronous `Freed` path (step 7):** it signals only after the drops, so the entry stays pending until then.
- **Snapshot memory:** `memory_bytes` equals the registry value, so totals neither undercount nor double-count.

**R2 exit paths.** Every exit from the `handle_connection` loop is a `break` that reaches the post-loop cancellation (`server.rs:73-246`: lines 73, 79, 83, 167, 191, 233, 245, then `server.rs:248-253`). There is no early `return`. `GG_CORE_MAX_CONNECTIONS` exists (`cli/config_cmd.rs:35,116`; default 64 at `connections.rs:17`).

**R3 cancel wiring.** The connection's cancel does reach the model, but only through `infer_cancellable`:

1. `cancel_request` cancels the request's token (`server.rs:206-209`).
2. `handle_inference_controlled` stores `true` in `flag` (`handler_inference.rs:70-73`).
3. The same `Arc<AtomicBool>` goes into the queue (`queue.rs:97-115`).
4. `QueuedRequest::stop_control` reads that flag (`queued_request.rs:89-95`).
5. The worker passes the control to `run_controlled` (`worker.rs:62,141`).
6. `infer_controlled` builds `check = || control.should_stop()` and passes it **only** to `model.infer_cancellable(&input, &config, Some(&check))` (`engine/inference.rs:188-189`).

**R4 lifecycle reservation.**
- `ModelLifecycle::load` with no pre-load reservation reserves exactly `model.memory_usage()` (`lifecycle_resident.rs:31-45`, `lifecycle.rs:138`).
- The IPC load path first reserves file `size_bytes` (`model_ops.rs:113-116,129-137`) and then resizes to `memory_usage()` (`lifecycle_resident.rs:40`).
- Worker admission reserves `working_memory_estimate()` plus one concurrency slot (`worker_admission.rs:24-31`, `limits.rs` `reserve_call`).
- `GG_CORE_N_CTX` exists (`cli/config_cmd.rs:32,112`; `backend_dispatch.rs:140-145`), and `ContextShape` is the single source of the estimate.

**R4 arithmetic.** For this fixture at `n_ctx = 512`, using the `working_memory.rs:103-118` formula and Qwen2.5-0.5B dimensions (n_vocab 151936, n_ff 4864, n_embd 896, 24 layers, 14 heads, 2 KV heads, head 64):

| Term | Size |
|---|---|
| activations: 512 × 158,592 × 4 | ≈ 325 MB |
| scores | ≈ 29 MB |
| KV | ≈ 6 MB |
| W = (sum) × 1.125 + 32 MiB | ≈ 0.44 GB |
| R (weights; file 491,400,032 bytes) | ≈ 0.49 GB |

`W < R` holds narrowly, and the plan's assertion guards it. The budget margin (R − W)/2 is about 25 MB.

### Audit Results (Revision 3)

| Pass | Result | Findings |
|---|---|---|
| Security | FAIL | S4: the installed A11 step drives the daemon's Job Object cap within about 25 MB of the accounted bytes, toward the known #1194 commit-retention path |
| Ghost UI | PASS | Unchanged from revision 2 |
| Section 4 Razor | PASS | §10 adds tests to the already-budgeted new files only |
| Dependency | PASS | No new crates |
| Orphan | PASS | Unchanged from revision 2 |
| Macro-Level Architecture | FAIL | S1, S2, S3: the A9 and A10 acceptance criteria cannot run as specified |

Observations (not scored):

- The R1 test, 1,000 listings against one concurrent unload "hook-free", is probabilistic. It sits in the `models` layer while the merge lives in `ipc/handler_models.rs`, so it can pass without ever hitting the step 3-5 window. The ordering argument above is what carries R1.
- A panic unwinding `handle_connection` would skip the post-loop cancellation. That applies equally to the existing `active_requests` tokens, so it is not newly introduced.

### Violations Found (Revision 3)

| ID | Category | Location | Description |
|---|---|---|---|
| S1 | Untestable acceptance (R3 residual) | Plan §10 R3 ("a stub model whose `infer` loops on `tokio::time::sleep(10ms)` until the request's stop check fires"); `engine/model.rs:35-44` (the default `infer_cancellable` calls `infer` and ignores `_is_cancelled`); `engine/inference.rs:188-189` (the stop check is passed only to `infer_cancellable`) | `Model::infer(&self, input, config)` has no access to the request's stop check. A stub that loops in `infer` never observes cancellation, so the worker keeps the `Arc` forever. `model_release_wait_response` then returns `released:false` at 10 s, and the criterion fails against correct production code. The stub must override `infer_cancellable` and poll its callback. The plan specifies the wrong method. |
| S2 | Untestable acceptance (R3 residual) | Plan §10 R3 expected order: `cancel_response`, then `inference_response{error_code:"cancelled"}`, then `model_release_wait_response`; `ipc/server.rs:206-222` (the reader writes `CancelResponse`); `server.rs:284-290` (a spawned task writes `InferenceResponse` after `handler_inference.rs:80-88` resolves on the same cancel); the wait response comes from a third spawned task | The three frames are written through the shared `ConnWriter` by three independent tasks, and nothing orders them. `cancel.cancel()` (`server.rs:207`) can wake the inference task, which may write its response before the reader writes `cancel_response`. A test asserting the stated sequence is flaky. |
| S3 | Untestable acceptance (R4 residual) | Plan §10 R4 (`max_concurrent = 1`, `max_memory_per_call = W + 1 MiB`) and A10 step 3 ("A queued request for `a` → `model_not_loaded`"); `scheduler/worker.rs:72-80` (`acquire_guard` runs before `run_blocking`, the only path to the model lookup); `engine/inference_memory.rs:8-11` and `engine/model.rs:23-25` (an unknown id gives estimate `None`); `scheduler/worker_admission.rs:24-31` (`None` falls back to reserving `max_memory_per_call()`); `memory/limits.rs:83-96, 106-125` (`try_acquire_working` reserves memory **and** a concurrency slot; `max_memory_per_call` does not cap it); `ipc/handler.rs:424-433` (`Memory limit exceeded` or `Queue full` is classified as `AdmissionRejected`) | While step 1's back-pressured stream holds the single concurrency slot and R + W of the budget, the request for the unloaded `a` reserves the fallback W + 1 MiB plus a slot before any model lookup. It is refused with `QueueFull` or `MemoryExceeded`, which reaches the client as `admission_rejected`, not `model_not_loaded`. Step 3 fails against correct code. The `max_memory_per_call = W + 1 MiB` setting is not the per-call cap the plan implies. It only sets the size of this fallback reservation and the backend's `max_memory_bytes` (`worker.rs:82`). |
| S4 | Security / resource containment (R4 residual) | Plan §10 R4, A11 ("installed daemon with `GG_CORE_N_CTX=512` and `GG_CORE_MAX_TOTAL_MEMORY` set to that measured budget"); `sandbox_boot.rs:64` (`max_memory_bytes: runtime_config.resource_limits.max_total_memory`); `sandbox/windows.rs:136-137` (`JOB_OBJECT_LIMIT_JOB_MEMORY` = that value); `lifecycle.rs:27-29`; `docs/integration/UPSTREAM_LLAMA_CPP_RETENTION_REPRO.md` (idle baseline about 92 MB; compute-buffer failure at the Job cap retains commit) | On Windows the accounting budget is also the Job Object commit cap. The budget R + W + (R − W)/2 leaves (R − W)/2, about 25 MB for this fixture, for all commit the accounting never charges: the process baseline (about 92 MB per the repro), runtime, tokenizer and thread stacks. When stream `a` creates its context, the real commit (baseline + R + the actual KV and compute buffers) exceeds the cap even though the accounting admitted it. That makes a native allocation fail at the Job limit: the documented path where llama.cpp keeps commit after a failed compute-buffer reservation (#1194), which #176 exists to avoid. The installed scenario would either fail or corrupt the A11 reclaim measurements. |

### Required Remediation (Revision 3)

1. **S1.** Specify that the stub overrides `infer_cancellable` (not `infer`) and loops on `tokio::time::sleep(10ms)` until the passed `is_cancelled` callback returns true, then returns. Cite `engine/inference.rs:188-189` as the only path that carries the stop check.
2. **S2.** State the expected frames as a set collected until all three arrive: `cancel_response{cancelled:true}` and `inference_response{error_code:"cancelled"}` in either order, plus `model_release_wait_response{released:true}`. The only ordering to assert is that the wait response arrives before its 10 s bound.
3. **S3.** Make A10 step 3 reachable, either:
   - by issuing the post-unload request for `a` when the slot and budget allow the fallback reservation (for example `max_concurrent = 2` and a budget that includes `max_memory_per_call`, with the arithmetic shown), or
   - by asserting the code the current admission order actually produces.

   State what `max_memory_per_call` does here (fallback reservation size and backend `max_memory_bytes`), not a per-call cap.
4. **S4.** For the installed Windows run, keep the Job Object cap above the accounted bytes by a measured margin for unbudgeted commit. For example, set the budget to R + W + (R − W)/2 plus the daemon's measured idle private bytes plus a stated safety margin, and show that the refusal arithmetic (a held, b refused) still holds. Alternatively, record that the in-flight installed step runs with an explicit margin and why. Add a check that no native allocation failure (`failed to allocate`) appears in the daemon log during A11.

### Verdict Hash (Revision 3)

SHA-256 over every byte of this file before the line that starts with `SHA256(report_through_revision_3)` (UTF-8, LF), computed with `sha256sum`. It covers the rev 1 and rev 2 records and this section.

```
SHA256(report_through_revision_3) = fb9cd8d93f16700187c0ed26ae73766f5374ccea5fab87afeed4906ccefcfa89
```

_The Revision 3 verdict is binding. Implementation may NOT proceed without modification. No ledger, Shadow Genome or staging file was written._

---

## Revision 4 Tribunal

**Tribunal Date**: 2026-11-09
**Target**: `docs/integration/MODEL_RELEASE_V1_PLAN.md` §11 "Revision 4 amendments" (S1-S4); source unchanged from `ae55c97`
**Risk Grade**: L2
**Auditor**: The QoreLogic Judge

### VERDICT (Revision 4): PASS

### Executive Summary (Revision 4)

S1-S4 are closed. Every acceptance criterion the plan now depends on is reachable against the current code. The budget arithmetic matches how the lifecycle (pre-load file-size reservation, then resize to `memory_usage`) and worker admission (metadata estimate or `max_memory_per_call` fallback, plus one concurrency slot) actually reserve. The Job Object margins hold. No blocking violation remains. The R1/R2 design closures from revision 3 and the V1-V10 closures from revision 2 still stand.

### Claims verified (Revision 4)

**S1: cancellation of the stub.**
- The trait method is `async fn infer_cancellable(&self, input: &InferenceInput, config: &InferenceConfig, is_cancelled: Option<&(dyn Fn() -> bool + Send + Sync)>) -> Result<InferenceOutput, crate::engine::InferenceError>` (`engine/model.rs:37-44`). A stub can override it and poll the callback, which `engine/inference.rs:188-189` passes from `StopControl::should_stop`. The cancel wiring was verified in revision 3.
- Finding on the wording "returns `Cancelled`": the trait's error type `crate::engine::InferenceError` (`engine/error.rs:9-70`, re-exported at `engine/mod.rs:74`) has **no** `Cancelled` variant. That variant exists only on `engine::inference::InferenceError` (`inference_types.rs:31`).
- The criterion still holds whatever the stub returns, for two reasons:
  1. `infer_controlled` checks `control.stop_error()` after the backend returns and replaces any result with `inference::InferenceError::Cancelled` (`engine/inference.rs:192-194`; `request_control.rs:66,97,119`).
  2. The IPC handler answers the client on the cancel itself, through `stopped(id, InferenceError::Cancelled)` (`handler_inference.rs:70-73,83`). `stop_code` maps everything except `DeadlineExceeded` to `InferenceErrorCode::Cancelled` (`handler_control.rs:34-39,51-57`), which serialises as `"cancelled"` (`protocol_types.rs:95-104`, `rename_all = "snake_case"`).

  A stub that returns any value (for example `Ok` with `FinishReason::Cancelled`, or `ModelError`) therefore yields `error_code:"cancelled"`. An attempt to return the nonexistent variant fails to compile; it cannot weaken the test.

**S2: response order.** Asserting the three frames as an unordered set within 10 s matches the three independent writers (`server.rs:206-222`, `284-290`, and the spawned wait).

**S3: the post-release `model_not_loaded` probe is reachable.**
- At step 8, `a` is released and `b` is not yet loaded, so nothing is resident. The request for `a` passes `scan_ingress` and the stop check (`worker.rs:64-71`).
- `acquire_guard` gets estimate `None` for the unknown id (`inference_memory.rs:8-11`, `model.rs:23-25`) and reserves the fallback `max_memory_per_call` = W + 1 MiB plus one slot (`worker_admission.rs:24-31`, `limits.rs:83-125`). That fits, because W + 1 MiB is at most the budget and `max_concurrent` is 2 with no other request.
- `run_controlled` → `get_model` fails with `inference::InferenceError::ModelNotLoaded`, displayed `"Model not loaded: a"` (`inference.rs:142-146`, `inference_types.rs:11-12`).
- The worker sends that string (`worker.rs:88`). `classify_worker_error` matches `"Model not loaded"` → `InferenceErrorCode::ModelNotLoaded` (`handler.rs:434-435`), which serialises as `"model_not_loaded"`.
- The rest of the in-process arithmetic holds:
  - step 1: R + W ≤ budget;
  - step 5: R + W + R > budget, because R > (R − W)/2;
  - step 9: R + W ≤ budget.

  `max_memory_per_call` is correctly described as the fallback size and the backend `max_memory_bytes` (`worker.rs:82`), not a per-call cap.

**S4: installed run under the Job cap.**
- The refusal compares the **pre-load** reservation. The IPC load reserves `metadata.size_bytes` (the file size) before any backend I/O (`handler_models.rs:25-37` → `model_ops.rs:110-116,129-137`). For SmolLM2 that is F_B = 1,055,609,824 B (`UPSTREAM_LLAMA_CPP_RETENTION_REPRO.md`), not its post-load `memory_usage`. The plan's "≈ 1.06 GB" is that figure.
- While A is held, the accounted total is `memory_usage_A + W_A` (the stream is in flight) `+ F_B` ≈ 0.49 + 0.44 + 1.06 ≈ 1.99 GB. That is above the cap of 1,342,177,280 B, so the plan's figure (which omits W_A) is conservative and the refusal holds.
- After release, F_B ≤ the cap and is admitted, then resized down to `memory_usage_B` ≤ F_B.
- Job cap during B's backend load. The cap equals `max_total_memory` (`sandbox_boot.rs:64`, `sandbox/windows.rs:136-137`). GGUF loads with `LlamaModelParams::default()` (`engine/gguf/backend.rs:61`), which is llama.cpp's mmap default, so weights are largely file-backed and charge little commit. Even in the worst case where all weights are private, about 1.06 GB + U ≈ 0.09 GB = 1.15 GB fits under 1.342 GB with about 190 MB to spare. Loading creates no context (`backend.rs:59-73`; contexts are per request at `backend.rs:343`), and step 4 runs no inference on B. The A-stream headroom gate (≥ 64 MiB, measured) and the stderr gate (`failed to allocate`, `ggml_` allocation errors, Job termination) turn any residual pressure into a loud failure rather than a silent one.
- `models list --json` exists (`main.rs:150-152`, `cli/models_cmd.rs:14-16`) and reports `memory_bytes` = `memory_usage`. `GG_CORE_N_CTX` exists (`cli/config_cmd.rs:32,112`).

### Audit Results (Revision 4)

| Pass | Result | Findings |
|---|---|---|
| Security | PASS | Wait and mutations authenticated; wait bounded, off the reader, cancelled with its connection (rev 3); listing complete and deduplicated (rev 3); installed run keeps measured headroom under the Job cap, with fail-loud gates |
| Ghost UI | PASS | CLI `--wait-ms` deliverable (rev 2) |
| Section 4 Razor | PASS | New sidecars and test files budgeted under 250 lines; pre-existing oversize files change by dispatch arms only |
| Dependency | PASS | No new crates (`tokio` `sync`, `futures` present) |
| Orphan | PASS | All new files attach via `#[path]`/`mod` |
| Macro-Level Architecture | PASS | A9, A10 and A11 are reachable as specified. The public API is preserved: no existing pub struct changes, new `#[non_exhaustive]` pair, enum-variant addition disclosed, A12 extended |

### Violations Found (Revision 4)

| ID | Category | Location | Description |
|---|---|---|---|
| (none) | | | |

### Verdict Hash (Revision 4)

SHA-256 over every byte of this file before the line that starts with `SHA256(report_through_revision_4)` (UTF-8, LF), computed with `sha256sum`. It covers the revision 1-3 records and this section.

```
SHA256(report_through_revision_4) = d7fd074c07bad7f111487488ccdd97406678d459653fb2496efcc515d19279b5
```

_The Revision 4 verdict is binding: the gate is OPEN for `/ql-implement` against plan revisions 1-4 as amended by §10 and §11. Per the caller's instruction, the ledger entry is recorded at merge. No ledger, Shadow Genome or staging file was written._
