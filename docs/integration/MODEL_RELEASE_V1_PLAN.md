# Verified model release on unload (v1.0.0, #137 foundation), revision 4

**Branch:** `feat/v1-unload-reclaim-20261109-no-ci` from `main` `ae55c97`.
**Risk grade:** L2. The change is to model-lifecycle logic, plus one new authenticated IPC message pair. Authentication, path confinement, manifest checks and budget limits are unchanged.
**Issue:** #137. This slice is the part of #137 that does not depend on the warm/cold switch policy still open for the owner in #153.
**Revision 2** answers audit VETO rev 1 (`MODEL_RELEASE_V1_AUDIT.md`, violations V1–V10). §8 maps each violation to its fix.

## 1. Problem (verified on `ae55c97`)

1. `model_unload_response{success:true}` is sent once the registry entry is removed.
   - A request still running on the model keeps it alive.
   - The backend objects and the resident budget are released only when that request ends (`models/lifecycle_resident.rs`).
   - A caller cannot learn when that has happened, or wait for it.
2. Since #174, a `model_load_request` right after such an unload can be refused with `ResourceExhausted`, because the old instance is still charged.
   - `COREFORGE_PRODUCER_CONTRACT.md` §7 still says that load "succeeds regardless".
   - `scripts/rc-ipc-control-acceptance.py::check_switch_after_cancel` asserts the same thing.
   - Both are now inaccurate.
3. `models_request` and its `total_memory_bytes` drop the model at once, so a host sees no model while the old instance is still allocated.
4. Models loaded without resource limits have no release tracking.

Any switching policy needs a truthful "the old instance is gone" signal. #153 also asks for a real state trace before the owner decides. This slice provides both and decides no policy.

## 2. Meaning of "released" (normative)

An unloaded instance is **released** when all three of these hold:

1. the lifecycle has observed that the last `Arc<dyn Model>` was dropped;
2. so the backend destructors ran (`llama_model_free`/context drop, or ONNX session drop);
3. its `ResidentGuard` then returned the budget.

It is **not** a claim that the operating system has reclaimed every page. The known upstream exception is `UPSTREAM_LLAMA_CPP_RETENTION_REPRO.md` (llama-cpp-rs#1194): llama.cpp can keep commit after a failed context allocation. GG-CORE's working-memory admission (#176) avoids triggering that path, but this slice does not cure it. OS-level reclaim is measured separately (A10) and is never inferred from `released`.

**Pending forever (fail closed).** In two cases an instance stays listed as releasing and a wait reports `released:false`; it is never reported as released:

- the release task's sender is dropped without signalling (runtime shutdown);
- there is no runtime, in which case the guard is deliberately leaked (existing behaviour).

**How long a holder can keep an instance.** A holder keeps an unloaded instance until its request completes, is cancelled, is disconnected or reaches its deadline.

- Streaming requests default to a 30 s deadline.
- Non-streaming and typed requests sent **without** `timeout_ms` have **no** deadline.
- The time to release is therefore not bounded by GG-CORE unless the client sets `timeout_ms`. The docs say this, and the plan claims no bound it cannot enforce.

## 3. Design

### 3.1 Release tracking (`models/lifecycle_resident.rs`)

- `Resident` holds `guard: Option<ResidentGuard>` and a snapshot of the registry's `LoadedModelInfo`, taken at load. So every loaded model is tracked, with or without limits.
- `Resident::release(self) -> ReleaseWatch`.
  - If the lifecycle holds the last reference, it drops the model and then the guard, and returns an already-signalled watch.
  - Otherwise it spawns the existing poll task. That task drops the model, then the guard, then sends `true`.
  - With no runtime, it leaks the guard and returns a watch whose sender is dropped, so the watch reads closed/unsignalled.
- `ReleaseWatch::released()` is true **only** if the watch value is `true`. A closed channel reads as `false`.
- `ReleaseWatch::wait(d)` waits for the value to become `true`, up to `d`. If the channel closes without `true`, it returns `false` at once.

### 3.2 Lifecycle (`models/lifecycle.rs` plus new sidecar `models/lifecycle_release.rs`)

- `ModelLifecycle::unload` keeps its signature. It additionally pushes a `Releasing { model_id, info, watch }` into a `ReleaseTracker`: an `Arc<std::sync::Mutex<Vec<…>>>` beside the index, never locked across `.await`.
- New additive methods on `ModelLifecycle`, implemented in the sidecar:
  - `releasing_models() -> Vec<LoadedModelInfo>`: snapshots of unloaded, unreleased instances, each with `state = Unloading` and its original `handle_id`.
    - It prunes released entries first.
    - Entries are a list, so a reloaded id and its old instance never collide.
  - `wait_released(model_id, wait) -> bool`: waits until no unreleased instance of `model_id` remains, up to `wait`.
    - It clones the watches under the mutex, then waits **without** any lifecycle or tracker lock held.
    - It returns `true` when there are no pending instances, including for an id that was never loaded. A missing load is reported by load and unload, not here.

### 3.3 IPC: a new message pair, with no change to existing structs (V3)

Existing pub structs (`ModelUnloadRequest`, `ModelUnloadResponse`, `ModelInfo`) are **not changed**, so external struct literals and destructuring keep compiling.

Wire format of the new pair, which is additive in protocol V1:

```json
{ "type": "model_release_wait_request", "model_id": "local-model", "wait_ms": 30000 }
{ "type": "model_release_wait_response", "model_id": "local-model", "released": true, "pending_instances": 0 }
```

- `wait_ms` is clamped to `[0, 120_000]`.
- `pending_instances` is the number of unreleased instances of that id when the response is built.
- A new Rust struct pair in `ipc/protocol_release_types.rs` carries these fields. It has constructors and `#[non_exhaustive]`, so later fields are not breaking.
- Two variants are added to `IpcMessage`. That enum is not `#[non_exhaustive]`, so an external **exhaustive** `match` on it would need a new arm. That is the same, already-accepted class of change that typed IPC (#173) made, and it is called out in the PR. No existing variant or field changes.
- **Authenticated.** It validates the session the same way `cancel_request` does. Without auth it returns `error{code:401}` and the connection stays open.
- **Off the reader (V5).**
  - `server.rs` hands the message to a sidecar `ipc/server_release.rs`.
  - That sidecar spawns the wait as a task that writes its response through the shared connection writer, as typed and streaming responses already do.
  - The reader keeps serving `cancel_request` and detects disconnects during the wait.
  - Each connection may have at most **one** pending release wait. A second one gets `error{code:409,"release wait already pending"}`. This bounds spawned tasks to one per connection, and connections are already capped.
- **Unauthenticated listing (V6/V7).**
  - `models_request` appends each releasing instance's `LoadedModelInfo` snapshot as a `ModelInfo` built by the existing mapping:
    - `name` is the metadata name, never the caller's `model_id`;
    - the original unique `handle_id`, `format`, `size_bytes`, `memory_bytes` and `loaded_at`;
    - `state:"unloading"`;
    - no capabilities.
  - Releasing entries whose `handle_id` is still present in the registry read are dropped. So an unload racing the listing shows the instance once, never twice, and `total_memory_bytes` never double-counts it.
  - The metadata class is exactly what loaded models already expose.
  - The mapping is a sidecar function in `handler_models.rs`; `handler.rs` changes by a call site only.

### 3.4 CLI (V4)

- `models unload <id>` is unchanged.
- `models unload <id> --wait-ms N`, via the new `run_unload_with_wait`:
  1. It sends the unload, then `model_release_wait_request`, on the same authenticated connection.
  2. Its client timeout is `N + 5 s`, using the existing `CliIpcClient::with_timeout`.
  3. It prints `released` or `release pending: N instance(s) still held by in-flight requests`.
  4. It exits 0 in both cases and 1 on failure. The existing `run_unload` keeps its signature.
- `--wait-ms` is parsed in `main.rs`.

### 3.5 Unchanged on purpose

- The unload response.
- Health reports.
- Default deadlines.
- The unwired `models::FlightTracker` (`models/drain.rs`). It holds no model references, and wiring it in is a #153 policy matter.

## 4. Security boundaries

- Mutations and the wait are authenticated.
- The wait holds no lifecycle lock, no tracker lock across `.await`, no admission or concurrency slot, and does not block the connection reader. It is bounded at 120 s, with one pending wait per connection.
- The listing adds no new metadata class and never shows the caller-chosen id.
- No new filesystem or network authority, and no new dependency. `tokio` `sync` is already enabled.
- Fail closed: `released:true` only after §2 holds. Closed or unsignalled watches are `false`.

## 5. Acceptance criteria and tests (new files, each under 250 lines; V8)

| # | Criterion | Test file |
|---|---|---|
| A1 | Idle unload: watch signalled at once; budget back to the prior value | `models/lifecycle_release_tests.rs` |
| A2 | Unload while a test holder keeps the `Arc`: listed `Unloading` with its original `handle_id`; budget still charged; after the holder drops, `wait_released` returns true and the budget is back | same |
| A3 | `wait_released` with a short wait returns false within `wait + 100 ms` while the holder is alive | same |
| A4 | No lock held during the wait: a concurrent `load` of another id completes while a waiter is pending | same |
| A5 | The same id reloaded while its old instance is releasing: two instances tracked; `wait_released` covers only unreleased ones | same |
| A6 | Unlimited lifecycle (no limits) is tracked identically | same |
| A7 | No-runtime release reads as not released (closed watch → false) and stays listed | same, a `#[test]` without a runtime |
| A8 | Wire: request/response JSON round-trip; `wait_ms` clamp; old messages unchanged (an existing unload JSON still decodes to the same struct) | `ipc/protocol_release_tests.rs` |
| A9 | Server: unauthenticated wait → 401 with the connection kept open; second concurrent wait → 409; while a wait is pending, the same connection's `cancel_request` is answered and cancels its stream (so the wait completes `released:true`); listing during release shows exactly one `unloading` entry, no `model_id`, and correct totals | `ipc/server_release_tests.rs` (stub model with a controllable holder) |
| A10 | **Real model, in-process daemon, deterministic** (Qwen2.5-0.5B, loaded as two ids `a` and `b` of the same file under a total budget of 1.5× one instance). The full scenario is listed after this table. Skips without the GGUF magic, but **panics** when `GG_CORE_REQUIRE_REAL_MODEL=1` (set on the VM) | `ipc/server_release_real_model_tests.rs` |
| A11 | **Installed binary, offline VM:** release `gg-core-cli` daemon plus CLI. The procedure is listed after this table | VM script; results in `docs/integration/MODEL_RELEASE_V1_EVIDENCE.md` |
| A12 | Public Rust API: `public_api_compat_test.rs` extended to bind `ModelLifecycle::unload`, `model_ops::unload_model`, `cli::models_lifecycle_cmd::run_unload`, and `ModelUnloadRequest`/`ModelUnloadResponse` struct literals with their existing fields only; the existing `tests/model_ipc_test.rs` literals compile unchanged | existing test file (+lines, stays under 250) |
| A13 | `scripts/rc-ipc-control-acceptance.py::check_switch_after_cancel` sends `model_release_wait_request` before the reload and asserts `released:true` | script |

**A10 scenario.**

1. A stream on `a` uses a 1 KiB duplex pipe, a 60 s stall limit, and a reader that has stopped. Back-pressure therefore holds the model deterministically.
2. Unload `a` → success.
3. A queued request for `a` → `model_not_loaded`.
4. Listing → one `unloading` entry.
5. Load `b` → refused: `ResourceExhausted` while `a` is held.
6. Wait for 2 s → `released:false`, `pending_instances:1`.
7. Resume reading → the stream ends with exactly one final frame and no error.
8. Wait → `released:true`.
9. Load `b` → success; infer on `b` → OK.

**A11 procedure.**

1. Start the daemon at default limits. Let it settle for 3 s, then take 5 samples 200 ms apart. The median is the baseline for private bytes, working set and handle count.
2. Run 10 cycles over Qwen2.5-0.5B, SmolLM2-1.7B (GGUF), MiniLM (ONNX embed) and DistilBERT-SST2 (ONNX classify). Each cycle per model is: load → one real use → `unload --wait-ms 30000` → settle 2 s → 5-sample median.
3. Criteria:
   - `released` on every idle unload;
   - median idle private bytes in cycle 10 within 10% of cycle 1, so no monotonic growth;
   - idle handle count within baseline ±16 after every cycle.
4. Then run the A10 scenario on the installed binary with a 1.4 GiB total: unload during a stream; refused SmolLM2 load; release observed; admitted load.

## 6. Razor

- New sidecars, each well under 250 lines:
  - `lifecycle_release.rs` (~110);
  - `protocol_release_types.rs` (~60);
  - `server_release.rs` (~90);
  - the four new test files.
- `lifecycle_resident.rs` grows from 74 to about 130, and `lifecycle.rs` from 224 to at most 240.
- Pre-existing oversize files change by call-site lines only:
  - `handler.rs` 490 (+2);
  - `server.rs` 336 (+4);
  - `protocol_types.rs` 361 (+4).

  Recorded as pre-existing, not worsened beyond dispatch arms.
- Functions are at most 40 lines and nesting at most 3.

## 7. Out of scope (owner decision in #153)

- An atomic switch operation, warm A+B routing, automatic rollback, and switch readiness semantics.
- Health or transition-state reporting.
- A default deadline for non-streaming requests.
- GPU/VRAM reclaim, and Linux/macOS installed reclaim (the VM is Windows only).

## 8. Audit rev 1 violation map

| V | Fix |
|---|---|
| V1 | §2 redefines released and cites the retention exception; closed watch = false and the entry is kept (A7) |
| V2 | §2 states that non-streaming and typed requests without `timeout_ms` have no deadline; no unenforced bound is claimed |
| V3 | No existing pub struct changes; new `#[non_exhaustive]` pair; enum-variant addition disclosed; A12 extended |
| V4 | CLI timeout is `wait + 5 s` via `with_timeout`; `run_unload` is kept and `run_unload_with_wait` added |
| V5 | The wait runs as a spawned task writing through the shared writer; one per connection; cancel served during the wait (A9) |
| V6 | Entries reuse the registry `LoadedModelInfo` snapshot (metadata name, original unique `handle_id`); never the `model_id` |
| V7 | Dedupe by `handle_id` against the registry read; totals never double-count (A9) |
| V8 | New test files named in §5 |
| V9 | The acceptance script is updated (A13); contract §7 is corrected |
| V10 | A10 uses only the repo Qwen fixture, holds the model by back-pressure, and fails loud on the VM; A11 has a sampling procedure and a second ONNX model |

## 9. Documentation

- `docs/IPC_PROTOCOL_SCHEMA.md`: the new pair, the meaning of `unloading`, the released definition and the deadline caveat.
- `COREFORGE_PRODUCER_CONTRACT.md` §7: corrected to the truthful behaviour. Documentation on `main` only; the CoreForge pin is untouched.
- `docs/integration/MODEL_RELEASE_V1_EVIDENCE.md`: VM results with variance.

## 10. Revision 3 amendments (audit rev 2 R1–R4; these supersede the conflicting text above)

**R1, ordering, so no instance is ever omitted.**
- `ModelLifecycle::unload` performs these steps in order, under the index write lock:
  1. look up the handle;
  2. take the `Resident` out of the index;
  3. **push its `Releasing` entry to the tracker**;
  4. `engine.unregister_model`;
  5. `registry.unregister`;
  6. remove the id from the index;
  7. start the release (`Resident::release` → watch, stored in the entry pushed in step 3).

  The entry is pushed with an unsignalled watch that the release then fulfils, so it is pending from the moment it is pushed.
- The listing reads the **registry first**, then the **tracker**, and drops tracker entries whose `handle_id` appeared in the registry read.
- Any instance whose memory is still held is therefore in at least one of the two reads, and is listed exactly once:
  - if the registry read happened before step 5, it is in the registry read;
  - otherwise it was pushed (step 3) before the tracker read.
- `total_memory_bytes` neither undercounts nor double-counts.
- Test (in `lifecycle_release_tests.rs`): a hook-free interleaving check. Unload runs concurrently with 1,000 listings, and every listing contains the instance exactly once until release.

**R2, a wait never outlives its connection.**
- `handle_connection` creates a per-connection `CancellationToken` and cancels it on every exit path, next to the existing `active_requests` cancellation.
- The spawned wait `select!`s on that token. When it is cancelled, it ends without writing.
- So pending waits ≤ 1 × live connections, and connections are capped (`GG_CORE_MAX_CONNECTIONS`).
- Test (in `server_release_tests.rs`):
  1. start a wait with a held instance;
  2. drop the client side;
  3. assert the task ends within 1 s (observed through a completion flag shared with the test via the handler's test hook, `Arc<AtomicUsize>` of live waits, `#[cfg(test)]` only).

**R3, A9 uses a non-streaming request.**
- The A9 cancel-during-wait case sends a non-streaming `inference_request` to a stub model whose `infer` loops on `tokio::time::sleep(10ms)` until the request's stop check fires (the cancellation path at `server.rs:145-168`).
- While that request holds the stub, the same connection sends unload then `model_release_wait_request{wait_ms:10000}`, then `cancel_request`.
- Expected:
  - `cancel_response{cancelled:true}`;
  - `inference_response{error_code:"cancelled"}`;
  - then `model_release_wait_response{released:true}` well before 10 s.

**R4, budgets derived from the real estimates, not guessed.**
- A10 (in-process):
  1. Load the Qwen2.5-0.5B fixture as `GgufGenerator` at `n_ctx = 512`.
  2. Read `R = memory_usage()` and `W = working_memory_estimate()` (the same values the lifecycle and worker admission reserve).
  3. Assert `W < R`; if not, panic and print both. Set `budget = R + W + (R − W) / 2`. Then:
     - `R + W ≤ budget`: one instance plus one request fits;
     - `2R > budget`: a second instance does not fit while the first is held;
     - after release, `R + W ≤ budget`: `b` loads and serves.
  4. The runtime is created with `max_total_memory = budget`, `max_memory_per_call = W + 1 MiB` and `max_concurrent = 1`. Instances are registered through `ModelLifecycle::load`, which admits against these limits.
  5. When `GG_CORE_RELEASE_BUDGET_OUT` is set, the test writes `{R, W, budget, n_ctx}` JSON there.
- A11 (installed):
  - The VM runs A10 first, in the same batch. Step 4 of A11 (the in-flight scenario) then runs the installed daemon with `GG_CORE_N_CTX=512` and `GG_CORE_MAX_TOTAL_MEMORY` set to that measured budget.
  - It uses **two ids of the same Qwen file**, so the arithmetic above applies exactly, in place of SmolLM2.
  - The idle cycles (steps 1–3) keep the default limits, where every model fits on its own.
  - The arithmetic and the measured values are recorded in the evidence document.

## 11. Revision 4 amendments (audit rev 3 S1–S4; these supersede the conflicting text above)

**S1.** The A9 stub overrides `Model::infer_cancellable`. It loops on `tokio::time::sleep(10 ms)` and returns `Cancelled` as soon as the `is_stopped` callback it receives returns true. Its plain `infer` returns at once. This is the path `engine/inference.rs` actually uses to pass the connection's cancel to a model.

**S2.** A9 collects frames until it has seen all three of `cancel_response{cancelled:true}`, `inference_response{error_code:"cancelled"}` and `model_release_wait_response{released:true}`, **in any order**. The only timing assertion is that all three arrive within 10 s of the `cancel_request` (the wait's `wait_ms` is 10 000, so a release that never happened shows up as `released:false` or a timeout).

**S3.** A10's limits and steps are corrected.
- `max_concurrent = 2`.
- `max_memory_per_call = W + 1 MiB`. This is **not** a per-call cap on working memory: it is the fallback reservation for a model without an estimate and the backend's `max_memory_bytes` ceiling, and W + 1 MiB keeps it above Qwen's real need.
- The "queued request for `a` gets `model_not_loaded`" check moves to **after** step 8 (release). A request for an unloaded id is admitted on the fallback reservation (W + 1 MiB, which fits once `a`'s bytes are back) and then fails its model lookup with `model_not_loaded`.
- Under the tight budget while `a` is held, the plan asserts nothing about requests for `a`. Steps 1, 2 and 4–9 are unchanged.
- Budget arithmetic, in-process (no Job Object in tests): `budget = R + W + (R − W)/2`. Step 1 needs `R + W ≤ budget`. Step 5 is refused because `2R > budget`. After release, `b` loads (`R ≤ budget`) and serves (`R + W ≤ budget`). The post-release `model_not_loaded` probe needs `R_b + W + (W + 1 MiB) ≤ budget` only if `b` is serving at the same time, so the probe runs **before** `b` is loaded.

**S4.** A11 step 4 (installed, Job Object cap = budget) no longer uses two Qwen ids. It uses Qwen2.5-0.5B as A and SmolLM2-1.7B as B, at `GG_CORE_N_CTX=512`, with `GG_CORE_MAX_TOTAL_MEMORY = 1.25 GiB` (1,342,177,280 B).

Budget arithmetic (both figures are recorded from the run):
- R_A comes from `models list --json`. W_A comes from A10's JSON output, which is the same fixture at the same n_ctx.
- The idle unaccounted process memory U is taken from the run's own baseline median. It is about 92 MB per the retention report.

Expected values:
- **Stream on A fits under the Job cap:** R_A + W_A + U ≈ 0.49 + 0.44 + 0.09 = 1.02 GB, under 1.342 GB with about 0.32 GB of headroom.
- **B is refused while A is held:** R_A + R_B ≈ 0.49 + 1.06 = 1.55 GB, over 1.342 GB. The refusal happens at the pre-load estimate, before any backend I/O.
- **B is admitted after release:** R_B + U ≈ 1.15 GB, under 1.342 GB. Step 4 only **loads** B (no inference on B under this cap), so W_B is never needed.

The run fails if either of these happens:
- the measured R_A + W_A + baseline U leaves less than 64 MiB of headroom under the cap (the evidence shows the computed headroom);
- the daemon's stderr contains `failed to allocate`, `ggml_` allocation errors or a Job Object termination.

The idle cycles (steps 1–3) stay at default limits, where `released` and OS reclaim are measured without cap pressure.
