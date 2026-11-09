# AUDIT REPORT

**Tribunal Date:** 2026-11-09
**Target:** scheduler head-of-line blocking, `docs/integration/V1_SCHEDULER_HOL_BLOCKING_PLAN.md` (revision 1, sha256 `8f6609a8c8c2871c69f47b515af310d8177bce55166fa20b9feacd71ccc77cc5`)
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** VETO

## Executive summary
The root-cause analysis checks out against `775fb91`, and the reproduction (`0fc892f`) is deterministic and needs no model. The verdict is VETO for four reasons:
- The proposed write deadline can corrupt the length-prefixed framing of a live connection.
- The fix leaves the embedded streaming path unbounded.
- A consumer-visible behaviour change is not reflected in the producer contract.
- Phase 2 pins a CPU-thread policy with no defined mechanism or owner decision.

## Passes
- **Security: FAIL (V1).**
  - Plan §Phase 1.4 wraps `write_all`/`flush` in a timeout and then attempts "one bounded error terminal write".
  - A timeout that cancels `write_all` part-way leaves a **partial frame** on the wire: the length prefix, or some of the payload, has been written.
  - Any later frame on that connection (the error terminal, a cancel ACK, or another request's response) is then parsed against a wrong length. That is protocol desynchronization on an authenticated channel.
  - The plan says nothing about the connection's state after a write timeout.
  - No credential, auth-bypass or placeholder issue.
- **Ghost path: FAIL (V2, V3).**
  - **V2:** `TokenStream::new` (unbounded) is still used by `runtime_facade.rs:109`, the embedded `Runtime::infer_stream` path that `COREFORGE_PRODUCER_CONTRACT.md` names as CoreForge's in-process surface. Its `spawn_blocking` task would still park forever on a consumer that does not read. The plan adds `new_bounded` but does not say whether `new` is kept, removed or bounded, so the defect is fixed for one caller of a shared primitive and silently kept for the other.
  - **V3:** the plan adds a new terminal reason ("stream consumer stalled") and a 5 s server-side read-stall bound. `COREFORGE_PRODUCER_CONTRACT.md:65-66` defines the streaming terminals and client timing and is not updated. The plan also does not say which `error`/`error_code` text the client sees.
- **Razor: PASS.**
  - The new files are sized within limits (≤ 150, ≤ 120, ≤ 100 lines).
  - `handler.rs` (579 lines, pre-existing violation) shrinks.
  - `worker.rs` stays at 238 lines.
- **Dependencies: PASS.** None added; `tokio::time::timeout_at` is already available.
- **Orphans: PASS.** Each proposed file is reached from an existing module (`scheduler/mod.rs`, `engine/mod.rs`, `ipc/mod.rs`) and named in the tests.
- **Macro architecture: FAIL (V4).**
  - Phase 2 states that "spawning N loops also caps the effective threads at `max(1, cores / N)` per context". But `n_threads` is resolved once at model load (`engine/gguf/backend.rs:65`, `resolve_threads`) and stored in `LlamaBackendInner`.
  - The plan names no mechanism for changing it per context.
  - The plan makes an unconditional halving of single-request decode threads the default whenever `max_concurrent = 2`, which is the shipped default. That is a behaviour trade-off decided without the owner.

## Violations
| ID | Category | Location | Description |
|---|---|---|---|
| V1 | Security (protocol integrity) | Plan Phase 1.4; `ipc/stream_bridge.rs` `write_frame` | A cancelled partial `write_all` desynchronizes framing for every later frame on the connection |
| V2 | Ghost path | `runtime_facade.rs:109`; Plan Phase 1.1 | The embedded `infer_stream` path is left unbounded; one shared primitive has two behaviours |
| V3 | Ghost path (contract) | `COREFORGE_PRODUCER_CONTRACT.md:65-66` | The new stall terminal and bound are not in the producer contract; the client-visible error is unspecified |
| V4 | Macro | Plan Phase 2 "CPU" | Thread cap has no mechanism, and a default regression is decided without the owner |

## Required remediation
1. **V1:** after any write timeout, the connection writer must be **poisoned**:
   - no further frame is written on that connection;
   - the connection is closed;
   - every request in flight on it is cancelled.
   The error terminal may only be attempted when **zero bytes** of the current frame were written. Add a test: a writer that accepts a partial frame and then blocks, after which no further bytes are written and the connection closes.
2. **V2:** make the bound a property of every `TokenStream`. Either `new` takes the stall bound, or `new` is removed and every caller (IPC handler, `runtime_facade`, tests) passes it explicitly. Add a facade test: an unread `infer_stream` releases its blocking task within the bound.
3. **V3:** update `COREFORGE_PRODUCER_CONTRACT.md` in the same change. Specify:
   - the exact terminal text (or connection close, per V1);
   - the default `GG_CORE_STREAM_STALL_MS`;
   - that a stalled reader cannot hold the daemon past it.
4. **V4:** either:
   - drop the thread cap from Phase 2, keeping `n_threads` as resolved at load and documenting the oversubscription; or
   - specify the mechanism (per-context `n_threads` in `ContextShape::params`) **and** record an explicit owner decision on the default.

## Ledger
Deferred to merge (parallel v1 branches), as for the other v1 tribunals. Shadow Genome entry #4.

---

# AUDIT REPORT: Revision 2

**Tribunal Date:** 2026-11-09
**Target:** plan revision 2 (sha256 `ab4b8f9147de5ff089f5c56bbdee5944723e745eaad248eeb4771723d263d3bb`)
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** VETO

## Revision-1 violations
- **V1:** addressed in intent: poisoning, no terminal after a timeout, closure, cancellation via `server.rs:269` `ActiveRequests`, which exists.
- **V2, V3, V4:** resolved. There is one bounded constructor, the contract text is specified, and the thread cap is dropped.

## Passes
- **Security: FAIL (V5).**
  - A connection has **two** independent frame writers over the same `Arc<Mutex<W>>`:
    - `IpcStreamBridge::write_frame` (`ipc/stream_bridge.rs`);
    - `write_frame_locked` (`ipc/server.rs:65`), used for non-streaming responses, cancel ACKs and the 409/admission errors (`server.rs:298`, `server_requests.rs:33,40,74`).
  - The plan's scope list ("all changes stay inside `scheduler/`, `engine/streaming.rs`, `ipc/handler.rs` and `ipc/stream_bridge.rs`") excludes `server.rs` and `server_requests.rs`.
  - So the poisoned flag "checked before every write" cannot be enforced on the second writer. After a partial stream frame, a non-streaming response or cancel ACK would still be written to a desynchronized connection, which is the V1 failure through a different path.
- **Ghost path: PASS.**
- **Razor: PASS.** No new violations; `server.rs` (337 lines, pre-existing) is not grown by the plan.
- **Dependencies: PASS.**
- **Orphans: PASS.**
- **Macro architecture: FAIL (V5).** Length-prefixed framing is implemented twice, and the plan's poison state would live in one copy only. There is no single source of truth for connection write state.

## Violations
| ID | Category | Location | Description |
|---|---|---|---|
| V5 | Security / Macro | `ipc/server.rs:53-72`, `ipc/server_requests.rs`, plan "Security" scope list | The second frame writer bypasses the poison flag; framing is duplicated |

## Required remediation
1. Introduce one connection writer type that owns both the framing and the poison state. Make `IpcStreamBridge` and every `server.rs`/`server_requests.rs` write go through it, and delete the duplicate `write_frame`.
2. Add `server.rs` and `server_requests.rs` to the scope list.
3. Add a test that a non-streaming response on a poisoned connection writes zero bytes.

Shadow Genome entry #4 is extended: a finding was resolved in one writer while a second writer over the same resource existed.

---

# AUDIT REPORT: Revision 3

**Tribunal Date:** 2026-11-09
**Target:** plan revision 3 (sha256 `23335432020f688092d57877cb021dd3ab2b668805505b0a5cfb9944ac4917d3`)
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** VETO

## Revision-2 violation
- **V5: resolved.**
  - `ConnWriter` is the only write-side framing.
  - `server.rs` and `server_requests.rs` are in scope.
  - The cancel re-check runs under the lock through `admit`.
  - The poisoned-writer test covers non-streaming, ACK and 409 writes.

## Passes
- **Security: PASS.**
- **Razor: PASS.**
- **Dependencies: PASS.**
- **Orphans: PASS.**
- **Macro: PASS.**
- **Ghost path (contract): FAIL (V6).**
  - Plan Phase 1.4 bounds each frame write by `min(now + stall, request deadline)`, and any write timeout poisons the connection.
  - So a stream whose deadline falls while a frame write is pending closes the connection, cancelling **other** requests on it, after less than `stall` of non-reading.
  - The contract text in §6 says closure happens only when a client "stops reading … for longer than `GG_CORE_STREAM_STALL_MS`". The mechanism and the published contract disagree. One request's deadline also gains connection-wide blast radius that is documented nowhere.

## Violations
| ID | Category | Location | Description |
|---|---|---|---|
| V6 | Ghost path (contract) | Plan Phase 1.4 vs §6 | Deadline-bounded frame writes poison the connection sooner than the published stall bound |

## Required remediation
1. Bound each frame write by `stall` alone.
2. At the request deadline, the relay stops starting new frames and ends the stream as today (`stream deadline exceeded`). A frame write already in progress keeps its `stall` bound. Only a write that exceeds `stall` poisons the connection.
3. Add a test: deadline expiry during a slow but progressing write does not close the connection.

---

# AUDIT REPORT: Revision 4

**Tribunal Date:** 2026-11-09
**Target:** plan revision 4 (sha256 `bff0cc1486036e22559d14fad2616e50a61b9939f2fc5dbb18af44942490bc50`)
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** PASS

## Revision-3 violation
- **V6: resolved.** Frame writes are bounded by `stall` only. The request deadline ends a stream without connection-wide effect, and a test pins that.
- The published contract (§6) and the mechanism now agree.

## Passes
- **Security: PASS.**
  - There is no auth, credential or policy change.
  - Framing integrity is guarded by one poisonable `ConnWriter`.
  - Cancel-after-ACK ordering is kept under the lock (`admit`).
  - Egress sanitization is never skipped, and a stalled stream never reads as `Complete`.
- **Ghost path: PASS.**
  - The embedded `infer_stream` path is bounded by construction.
  - The producer contract and the CLI config surface are named in scope.
- **Razor: PASS.**
  - The new files are within limits (`conn_writer` ≤ 120, `stream_relay` ≤ 150, `streaming_bound` ≤ 100, `worker_pool` ≤ 120 lines).
  - The pre-existing oversize files (`handler.rs` 579, `server.rs` 337) only shrink.
- **Dependencies: PASS.** None added.
- **Orphans: PASS.** Every new file is reached from `ipc/mod.rs`, `engine/mod.rs` or `scheduler/mod.rs`, and is exercised by named tests.
- **Macro architecture: PASS.**
  - One stream-bound primitive, one write-side framing, one stall setting.
  - Phase 2 is explicitly gated on `unsafe impl Sync` evidence, with a defined fallback.
  - Phase 2 changes no CPU-thread default.

**Scope of this PASS:**
- **Phase 1 (A + B)** may be implemented.
- **Phase 2 (D)** may be implemented only after its stated precondition evidence (the `unsafe impl Sync` review and the VM concurrent-vs-serial comparison) is recorded against this plan.
- **Option C** remains deferred.

## Ledger
Deferred to merge (parallel v1 branches), as for the other v1 tribunals. Shadow Genome entry #4: remediation applied in revisions 2–4.

---

# AUDIT REPORT: Phase 2 implementation addendum

**Tribunal Date:** 2026-11-09
**Target:** plan revision 4 plus Phase 2 implementation notes (sha256 `206aad3672e3e24a627290c5ae1c9cc032d3da98f38b657eed5396794bd2c57e`); implementation `0e4f3de`
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** PASS

This tribunal sat because the implementation departs from the plan text in one place: where the CPU oversubscription note is documented.

## Precondition (Phase 2 gate)
- **Static review: satisfied.**
  - `LlamaBackendInner` holds only immutable shared state.
  - llama-cpp-2 0.1.161 itself declares `LlamaModel: Send + Sync`.
  - Every generation path builds its own context, batch and sampler.
  - No statics, thread-locals or interior mutability are on the release path. The speculative `Mutex` is `advanced`-only.
- **Measured: satisfied.** `concurrent_generations_match_serial_outputs` passed in the offline VM (real Qwen2.5-0.5B, 7.3 s, not skipped): three concurrent rounds were byte-identical to the serial greedy outputs. The `Semaphore(1)` fallback is correctly not applied.

## Passes
- **Security: PASS.**
  - Concurrency is bounded by `max_concurrent` loops and by `acquire_guard`. The pool test shows that the peak in flight equals `max_concurrent` and never exceeds it.
  - Memory remains one checked total, with the Job Object cap as the backstop.
  - No new IPC surface, dependency or unsafe code.
  - The default behaviour change (2 concurrent requests) is documented, together with the serial opt-out.
- **Ghost path: PASS.** The contract §Concurrency and the config table describe the runtime behaviour.
  - **Deviation (documentation location):** the note is not in `RELEASE_SUPPORT_AND_MAINTENANCE.md`. Accepted, because that file is the RC1 support guide and v1 behaviour there would be a false claim about RC1.
- **Razor: PASS.**
  - `worker_pool.rs` is 122 lines. That exceeds the plan's own ≤ 120 estimate but is well within the 250 limit, so it is not a violation.
  - Every new function is ≤ 40 lines with nesting ≤ 3.
  - `server.rs` (337 → 336) has not grown.
- **Dependencies: PASS.** None added.
- **Orphans: PASS.**
  - `worker_pool` is reached from `spawn_worker_with_registry` (daemon `runtime_init.rs`, and every test).
  - `ResourceLimits::max_concurrent` is used by `worker_pool::spawn`.
- **Macro architecture: PASS.**
  - Execution logic stays in `worker.rs`, `worker_streaming.rs` and `worker_typed.rs`; the pool only schedules.
  - Queue ownership and wake semantics are unchanged (`notify_one` per enqueue, checked before each wait).

## Ledger
Deferred to merge (parallel v1 branches), as for the other v1 tribunals.

---

# AUDIT REPORT: Revision 5 (Addendum A, per-context thread policy)

**Tribunal Date:** 2026-11-09
**Target:** plan revision 5 (sha256 `3e15a82489a894a5bb50e06215a68955bd971f1eb91fdfa7a0e935cb76dd0b58`)
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** VETO

## Executive summary
The policy itself is sound, owner-approved and measured. The verdict is VETO because the plan applies the divided default on load paths where `max_concurrent` does not bound concurrency. That silently regresses embedded users.

## Passes
- **Security: PASS.**
  - No new input, IPC surface, dependency or unsafe code.
  - Division by zero is excluded (`workers ≥ 1`), and the auto result is in [1, 16].
  - The memory claim checks out: per-thread scratch is inside the fixed 32 MiB `OVERHEAD` term (`working_memory.rs:22`), and the auto count never rises.
- **Ghost path: FAIL (V7).**
  - Design step 3 claims that "every load path sees the real worker count", using `ResourceLimits::max_concurrent()` from the lifecycle.
  - But the embedded `Runtime::infer` (`runtime_facade.rs:60-80`) and `Runtime::infer_stream` (`runtime_facade.rs:93-122`) call `InferenceEngine` directly. They never pass through `worker_pool` or `acquire_guard`, so embedded concurrency is chosen by the caller and is not bounded by `max_concurrent`.
  - With `ResourceLimitsConfig::default()` (`max_concurrent: 2`, `limits.rs:32`), a serial FFI or Python embedder would get 6 threads instead of 12 on 12 vCPUs: a single-request decode regression with no concurrency gain.
  - The same applies to a Rust embedder calling the public `models::load_model_from_path`.
- **Razor: PASS.** The size estimates are within limits, and pre-existing oversize files only shrink.
- **Dependencies: PASS.** `num_cpus` is already an unconditional dependency (`Cargo.toml:95`).
- **Orphans: PASS.** `threads.rs` is reached from `engine/gguf/mod.rs`, `backend.rs`, `backend_dispatch.rs` and `config_cmd.rs`.
- **Macro architecture: PASS, apart from V7.** There is one policy function, and the pool and the policy share `worker_loops`.

## Violations
| ID | Category | Location | Description |
|---|---|---|---|
| V7 | Ghost path (behaviour regression) | Plan Addendum A, Design step 3; `runtime_facade.rs:60,93`; `ffi/models.rs:50`; `python/session.rs:106`; `models/model_ops.rs:151` | The divided default is applied on load paths whose inference is not bounded by `max_concurrent`, halving embedded single-request threads |

## Required remediation
1. Apply `max_concurrent` division only where the worker pool bounds execution, which is the daemon.
2. Make `workers` an explicit argument of the public `model_ops` load functions, rather than inferring it from `limits`.
3. FFI and Python pass 1.
4. Add a test that pins the embedded value as unchanged, and update the contract to say so.

## Ledger
Deferred to merge (parallel v1 branches), as for the other v1 tribunals. Shadow Genome entry #5.

---

# AUDIT REPORT: Revision 6

**Tribunal Date:** 2026-11-09
**Target:** plan revision 6 (sha256 `c900d005e29616c265503a52d255e12ba1fb0a91e2d788f712ab414b5151bf58`)
**Risk Grade:** L2
**Auditor:** The QoreLogic Judge
**VERDICT:** PASS

## Revision-5 violation
- **V7: resolved.**
  - `workers` is an explicit argument of `prepare_model_admitted` and `load_model_from_path`.
  - Only the daemon callers (`runtime_init::preload_models` and `ipc/handler_models.rs`) pass `worker_loops(Some(&runtime.resource_limits))`, which is the same object given to `worker_pool::spawn` (`runtime_init.rs:276`).
  - FFI and Python pass 1, so embedded behaviour equals today's `min(logical, 16)`, pinned by a test.
  - The contract text names the embedded behaviour and the override.

## Passes
- **Security: PASS.** As at revision 5. There is no surface change and the arithmetic is bounded. Memory admission is unchanged and stays an upper bound.
- **Ghost path: PASS.**
  - Every load path is enumerated: daemon preload, IPC load, FFI, Python and public Rust.
  - `config show` and `config defaults` expose the resolved value.
  - The contract, `config.rs` row and `USAGE_GUIDE.md` are in scope.
- **Razor: PASS.**
  - `threads.rs` is ≤ 80 lines; every touched file ≤ 250 stays ≤ 250.
  - `backend.rs` (678) shrinks, and `runtime_init.rs` (316) does not grow.
  - Every function is ≤ 40 lines with nesting ≤ 3.
- **Dependencies: PASS.** None added.
- **Orphans: PASS.** Every new item has a production caller and a named test.
- **Macro architecture: PASS.**
  - One policy function (`threads::per_context`) is reached from both the dispatch path and `resolve_threads`.
  - One worker-count definition (`ResourceLimits::worker_loops`) is shared by the pool and the daemon load path.
  - Threads are still resolved once per model load.

**Scope of this PASS:** Addendum A (revision 6) may be implemented on `feat/v1-thread-policy-20261109-no-ci`. The VM re-measurement in its Tests section is the acceptance gate. Any default other than `max(1, min(logical, 16) / workers)` on the daemon path needs a new tribunal.

## Ledger
Deferred to merge (parallel v1 branches), as for the other v1 tribunals.
