# Scheduler head-of-line blocking behind streams (v1.0.0)

**Risk grade: L2.** This changes scheduling, back-pressure and stream termination. It changes no auth, IPC framing or policy. **Gate: a `/ql-audit` PASS is required before implementation.**
**Base:** v1 integration `775fb91` (merge of #174, #175 and #176 onto the typed-IPC stack). Branch `fix/v1-scheduler-hol-blocking-20261109-no-ci`.
**Tracks:** the availability limitation found during #176 qualification. It predates memory admission and is not caused by it. The v1.0.0 gate is #161.
**Out of scope:** frozen `v0.10.0-rc.1` and the CoreForge pins. GPU, continuous batching and hot-swap are also out of scope.

**Revision 2 (2026-11-09):** responds to the revision-1 VETO (`V1_SCHEDULER_HOL_BLOCKING_AUDIT.md`):
- **V1:** a write timeout poisons and closes the connection.
- **V2:** the stall bound is part of the only `TokenStream` constructor, so the embedded path is covered too.
- **V3:** the producer-contract text is specified.
- **V4:** the Phase 2 thread cap is dropped.

**Revision 3 (2026-11-09):** responds to the revision-2 VETO (V5). One `ConnWriter` owns framing and poison state for every write on a connection. `server.rs`/`server_requests.rs` are now in scope.

**Revision 4 (2026-11-09):** responds to the revision-3 VETO (V6). Frame writes are bounded by `stall` only. The request deadline ends the stream but never poisons the connection.

**Revision 5 (2026-11-09):** Addendum A adds an owner-approved per-context thread policy (auto threads = `max(1, min(logical, 16) / max_concurrent)`, with `GG_CORE_N_THREADS` as the explicit override). It replaces V4's "no per-context thread policy" and nothing else.

**Revision 6 (2026-11-09):** responds to the revision-5 VETO (V7). The divided default applies only where the worker pool actually bounds concurrency. The daemon passes its loop count explicitly into `model_ops`. Embedded FFI/Python and public Rust `model_ops` callers choose `workers` themselves (FFI/Python pass 1) and keep one worker's worth of threads, because embedded `Runtime::infer`/`infer_stream` are not bounded by `max_concurrent`.

## Observed
On the v1 integration daemon, with explicit memory limits and CPU GGUF:
- A worker-path request queued behind an active stream waited until the stream ended (about 17 s).
- When a client stopped reading its stream, other requests timed out at the client's 300 s limit.

## Root cause (code at `775fb91`)
The worker blocks in two independent ways.

**1. One serial worker.** `runtime_init.rs:271` spawns exactly one `worker_loop` (`scheduler/worker.rs:64`).
- Each iteration takes **one** request and awaits it to completion. Streams come first (`:66`), then typed requests (`:70`), then the main queue (`:80`).
- `ResourceLimits` already admits up to `GG_CORE_MAX_CONCURRENT` requests (default 2, `memory/limits.rs:125`), but with a single worker that concurrency is never used.
- Strict stream-first order also lets a steady supply of streams starve the main and typed queues.

**2. Unbounded sends to the stream consumer.** Every stage between the client pipe and the worker can wait forever:

| Stage | Location | Blocking await | Bound today |
|---|---|---|---|
| Pipe write | `ipc/stream_bridge.rs` `write_frame` | `write_all` + `flush` while holding the **per-connection writer mutex** | none |
| Relay | `ipc/handler.rs` `relay_stream` | `sender.send(..).await?` inside a `select!` arm, so neither cancel nor the deadline can pre-empt it | none (the deadline arm is not polled while an arm body awaits) |
| Token channel | `ipc/handler.rs:409` | `TokenStream::new(32)`, a bounded mpsc | fills in 32 frames |
| Backend emit | `engine/gguf/backend.rs:192,196,211` | `rt.block_on(sender.text/token(..))` | none. `should_stop` (`:155`) is checked only *between* tokens. |
| Terminal | `engine/inference_streaming.rs:41`; `scheduler/worker_streaming.rs` pre-execution, admission and non-gguf paths | `sender.end(..).await` | none |
| Worker | `scheduler/worker_streaming.rs` `run_stream` | awaits the `spawn_blocking` JoinHandle | none |

- Once the client stops reading, the pipe buffer fills. The relay then parks inside `write_all`, the channel fills, and the backend parks inside `block_on(send)`. The worker in turn parks on the JoinHandle.
- The stream's own deadline cannot fire, because nothing that checks it is running.
- Only the receiver being dropped releases the worker. That happens when the pipe write errors, which in practice means the client disconnects or times out. That is the 300 s seen.
- The stalled writer also holds the connection mutex, so cancel ACKs and other responses on that connection queue behind it.

**Reproduction:** `scheduler/worker_hol_tests.rs` (commit `0fc892f`). No model is needed.
- A capacity-1, pre-filled, never-read stream is enqueued with a 300 ms deadline.
- An unrelated request then stays unanswered after 2 s (past the deadline).
- It is answered only after the stalled receiver is dropped.
- `stalled_stream_consumer_blocks_unrelated_request_today` asserts today's behaviour and passes.
- `stalled_stream_consumer_does_not_block_unrelated_request` (`#[ignore]`) is the post-fix contract.
- Both run with and without `gguf`: the first blocking send (`end` on the error path) already parks the worker.

## Options considered

| | Option | Fixes stalled consumer | Fixes serialization behind a healthy stream | Risk |
|---|---|---|---|---|
| A | Bounded token send: deadline-bounded `send`, ending the stream on stall | yes | no | low |
| B | Per-stream write deadline in the relay/bridge, with the write moved out of the `select!` arm body | yes, and frees the connection mutex | no | low |
| C | Admission/refusal before dequeue: refuse work that cannot start before its deadline | no (fails fast instead of at 300 s) | no (fails fast) | low |
| D | `N = max_concurrent` workers sharing the queues | partly: one stall consumes one of N slots | yes, up to N | medium (backend concurrency, CPU) |

- **A or B alone** leaves the 17 s serialization in place.
- **D alone** still lets N stalled clients take every worker.
- **C** improves the failure mode but restores no throughput.

## Design (recommended: A + B now, D gated, C deferred)

### Phase 1: bound every send (A + B). This closes the stalled-consumer hold.
1. **Every `TokenStream` is bounded** (revision 2, V2). There is one constructor and no unbounded variant.
   - `TokenStream::new(cap, StreamBound { stall: Duration, deadline: Option<Instant> })` replaces `new(cap)`.
   - Every caller passes it: `ipc/handler.rs:409`, `runtime_facade.rs:109` (embedded `Runtime::infer_stream`, deadline from `config.timeout_ms`) and all tests. The compiler enforces that none is missed.
   - `stall` comes from one `RuntimeConfig` field for both the daemon and the embedded path.
   - Each `token`/`text` send uses `tokio::time::timeout_at(min(now + stall, deadline), send)`.
   - On expiry it returns a new `StreamSendError::Stalled`, and the sender marks itself closed. Every later send fails immediately.
   - `end()` is bounded the same way. If the terminal cannot be delivered it is dropped; the relay then reports `stream dropped before terminal` as an error (the existing B-24a contract: never read as `Complete`).
   - `stall` defaults to 5 s. It is configurable as `GG_CORE_STREAM_STALL_MS` (minimum 100 ms, maximum `timeout_ms`), and the value is shown by `gg-core-cli config defaults|validate`.
2. **The backend stops on a failed send.** `emit_token` already returns `false` when a send fails, and the loop breaks. `run_stream_sync` maps `Stalled` to `InferenceError::ExecutionFailed("stream consumer stalled")`. Telemetry records it as a failure with a content-free reason (`stream_stalled`).
3. **Pre-execution `end()` calls in `worker_streaming.rs`** (cancelled, expired, ingress-rejected, admission-refused, non-gguf) go through the same bounded `end()`.
4. **Relay write deadline (B).**
   - `relay_stream` no longer awaits `sender.send` inside a `select!` arm body. Each frame write is bounded by `stall` **alone** (revision 4, V6), never by the request deadline, and the cancel token is checked while it waits.
   - At the request deadline the relay starts no new frame and ends the stream as today (`stream deadline exceeded`). A frame write already in progress keeps its `stall` bound. Only a write that exceeds `stall` poisons the connection.
   - On expiry the relay cancels the request token and **drops the stream receiver**, so the backend's next send fails immediately rather than after `stall`.
   - It returns `HandlerError::StreamSend("stream write stalled")`.
   - `IpcStreamBridge::write_frame` applies the same bound while holding the writer mutex, so a stalled pipe can hold that mutex for at most `stall`.
   - **One connection writer** (revision 3, V5). A new `ipc/conn_writer.rs` defines `ConnWriter<W> { inner: Mutex<W>, poisoned: AtomicBool }` with a single `write_frame(bytes, deadline) -> Result<(), WriteError>`. It is the **only** length-prefixed framing on the write side:
     - `IpcStreamBridge` holds an `Arc<ConnWriter<W>>`;
     - `server.rs` `write_frame`/`write_frame_locked` are deleted, and every write in `server.rs` (responses, cancel ACKs) and `server_requests.rs` (409, admission errors) calls `ConnWriter::write_frame`;
     - non-streaming writes use the same `stall` bound.
   - **Connection poisoning** (revision 2, V1). A timed-out frame write may already have put part of a length-prefixed frame on the wire, and any later frame would then be parsed against the wrong length. So the first write timeout on a connection:
     - sets `ConnWriter::poisoned`. That flag is checked under the writer mutex before every write, through the single writer above (cancel ACKs, error terminals and non-streaming responses included), and nothing more is written;
     - cancels every request in flight on that connection, through the connection's existing per-request cancel tokens;
     - closes the connection (shutdown of the write half, then drop).
   - No error terminal is attempted after a timeout. The client sees the connection close, which `COREFORGE_PRODUCER_CONTRACT.md` already says to treat as failure plus reconnect.
5. **Resulting worst-case hold:** at most `stall` after the client stops reading, plus one token's decode time. The stream deadline is never exceeded.
6. **Producer contract** (revision 2, V3). `COREFORGE_PRODUCER_CONTRACT.md` §Streaming and §Timeouts are updated in the same change with this text:
   > A client that stops reading a stream for longer than `GG_CORE_STREAM_STALL_MS` (default 5 000 ms, range 100 ms … `timeout_ms`) loses that stream. If the daemon has not yet blocked on the connection, the client receives `stream_chunk{is_final:true, error:"stream consumer stalled"}`. If a frame write itself stalled, the daemon closes the connection and cancels every request on it. Treat either as failure plus reconnect. A stalled reader cannot hold the daemon's worker past this bound. The embedded `Runtime::infer_stream` stream ends with `StreamTerminal::Error("stream consumer stalled")`, or is dropped, under the same bound.

### Phase 2: bounded worker concurrency (D). This needs measured evidence (below).
- `spawn_worker_with_registry` spawns `max_concurrent` loops over the same `RequestQueue`. There is one shutdown token, and a `JoinSet` is awaited on shutdown.
- `max_concurrent` comes from `ResourceLimits`. When no limits are given it is 1, so embedded/test behaviour is unchanged.
- **Every request still goes through `acquire_guard` first.** Working memory (resident weights plus N × per-request estimate) stays within the single checked total, and so within the Job Object cap. A guard refusal is an explicit error, as today.
- **Fairness:** each loop round-robins stream → typed → main, starting from the queue after the one it served last. No queue can starve another.
- **Precondition (blocks Phase 2):** `LlamaBackendInner` carries a hand-written `unsafe impl Sync` (`backend.rs:31`). Phase 2 needs:
  - a review showing that every `&self` path used during generation only creates per-request contexts and reads the immutable `LlamaModel`;
  - a concurrent-generation test (two contexts, one model) in the offline VM that compares the outputs with serial runs.
  - If either fails, the GGUF model gets a per-model `tokio::sync::Semaphore(1)`. Phase 2 then still runs typed (ONNX) and different-model requests in parallel, and the plan records that GGUF generation stays serial.
- **CPU** (revision 2, V4): there is no thread cap. `n_threads` stays as resolved at model load (`backend.rs:65`). Two concurrent GGUF contexts may oversubscribe CPU and slow each request; that is documented in `RELEASE_SUPPORT_AND_MAINTENANCE.md`. Performance is not RC-qualified, and any per-context thread policy is a separate, owner-decided change.

### Deferred: C (admission before dequeue)
- Today an expired request is rejected without execution only when it is dequeued. After Phase 1 the worst hold is bounded, so C is not needed for correctness.
- Revisit it if Phase 2 is refused: it would return an explicit "worker busy, deadline unreachable" instead of a late timeout.

## Security and containment review
- **No authority or surface is added:**
  - no new IPC message, dependency, network path or file access;
  - all changes stay inside `scheduler/`, `engine/streaming.rs` (+ `streaming_bound.rs`), `runtime_facade.rs`, `config.rs`/`cli/config_cmd.rs` (the stall setting), and `ipc/{handler,stream_relay,stream_bridge,conn_writer,server,server_requests}.rs`;
  - no forbidden module or crate.
- **Fail-closed termination:** a stalled or failed stream always ends as `Error` or dropped (an error), never `Complete`. The B-24a/B-24b contracts are unchanged. Egress sanitization still runs before any text frame. A stall stops generation; it never skips the sanitizer, and the withheld tail is discarded, not flushed.
- **Cancellation ordering:** the cancel-ACK-under-writer-lock rule (`stream_bridge.rs`) is kept. `ConnWriter::write_frame` takes an `admit: impl FnOnce() -> bool` that is evaluated **after the lock is acquired and before any byte is written**. `IpcStreamBridge` passes its existing check, "not cancelled, or this is the error terminal", so a chunk that waited for the lock still cannot be emitted after a cancel ACK.
- **After a stall:**
  - a token-channel stall (step 1) still delivers the error terminal through the healthy connection;
  - a write stall (step 4) poisons the connection, and then no frame of any kind is written.
- **DoS posture improves:**
  - Today one authenticated client can hold the only worker indefinitely by not reading. After Phase 1 it can hold it for at most `stall`.
  - After Phase 2 it can hold at most one of N slots for at most `stall`.
  - The queue bound (`max_pending`), per-connection limits and the Windows pipe backlog bound (#165) are unchanged.
- **Memory containment:**
  - Phase 1 does not change reservations: guards are released when the worker returns, which is now bounded.
  - Phase 2 raises peak working memory to N × estimate, which is exactly what `acquire_guard` already admits against the total. The Job Object cap remains the backstop, and the #176 retention scenario is still refused before allocation.
- **Platform honesty:** none of this changes OS confinement claims (Linux seccomp; Windows Job Object without network/filesystem isolation; no macOS sandbox).
- **Unsafe code:** Phase 1 adds none. Phase 2 makes the existing `unsafe impl Sync` load-bearing for concurrency. That is why it is a precondition with its own evidence, not an assumption.

## Razor
- `worker.rs` is 238 lines. Phase 2's multi-loop spawn and fairness go in a new `worker_pool.rs` (≤ 120 lines).
- The bounded-send logic goes in `engine/streaming_bound.rs` (≤ 100 lines). `streaming.rs` (153) only delegates.
- `handler.rs` is already over 250 lines (pre-existing). The relay moves to `ipc/stream_relay.rs` (≤ 150 lines), and `handler.rs` gets shorter.
- `ipc/conn_writer.rs` is ≤ 120 lines. `server.rs` (337, pre-existing) only shrinks, because its framing functions move out. `stream_bridge.rs` (161) shrinks.
- Every function is ≤ 40 lines with nesting ≤ 3.

## Tests
- **Unit, no model:**
  - un-ignore `stalled_stream_consumer_does_not_block_unrelated_request` and delete the characterization test;
  - a bounded `end()` on a full channel returns within `stall`;
  - `Stalled` closes the sender (later sends fail at once);
  - a stalled stream's terminal is never `Complete`;
  - the relay, given a writer that never completes, returns within `stall`, cancels the token and drops the receiver;
  - a bridge write with a stalled writer releases the mutex within `stall`;
  - poisoning (V1): given a writer that accepts the 4-byte length prefix and then blocks, no further byte is written after the timeout (a cancel ACK and an error terminal on the same connection included), every in-flight request token on the connection is cancelled, and the write half is shut down;
  - cancel ordering is unchanged: a data chunk blocked on the `ConnWriter` lock while a cancel ACK is written is refused by `admit` (the existing `stream_bridge` tests are ported unchanged);
  - deadline vs stall (V6): when the request deadline expires during a slow but progressing write (each chunk accepted within `stall`), the stream ends with `stream deadline exceeded` and the connection stays open and usable for the next request;
  - single writer (V5): on a poisoned `ConnWriter`, a non-streaming response, a cancel ACK and a 409 each write zero bytes and return `WriteError::Poisoned`;
  - facade (V2): an unread `Runtime::infer_stream` releases its blocking task within `stall` and ends as an error, never `Complete`;
  - config validation for `GG_CORE_STREAM_STALL_MS` bounds.
- **Phase 2:**
  - with two mock models, two requests overlap in time;
  - fairness: an alternating stream/main load serves both;
  - the guard count never exceeds `max_concurrent`;
  - shutdown joins every loop.
- **Offline VM (real Qwen2.5-0.5B), CPU GGUF:**
  - a stalled-reader client while a second client generates: the second is answered within normal latency plus `stall`, and the stalled stream ends with an error;
  - Phase 2 only: the concurrent-vs-serial output comparison above, and peak private bytes ≤ resident + N × estimate.

## Validation
- Run natively: `cargo fmt --check`, `clippy --all-targets --features onnx,gguf,ffi -D warnings`, and the scheduler/ipc tests in the `coreforge-build` VM (`ggv1-hol` paths only).
- No GitHub Actions: draft PR, `[skip ci]` commits, `-no-ci` branch.

## Implementation notes (Phase 1)
Phase 1 is implemented on this branch. These notes record where the code refines the audited text without widening it:

- **Cancel during a write.** Plan step 4 says "the cancel token is checked while it waits". In the code, cancellation (like the deadline) only decides whether the *next* frame starts: a frame write in progress is never aborted, because aborting part-way would desynchronize framing (V1). That write stays bounded by `stall`. The cancel check under the writer lock (`admit`) is unchanged.
- **Deadline vs. stall on the token channel.** A token send that hits the request deadline returns `StreamSendError::Deadline`. It does not close the sender, so the final terminal is still delivered when capacity is free. Only a stall (`StreamSendError::Stalled`) closes the sender. Any send failure turns an `Ok` backend result into an error terminal (`stop_or_send_failure`), so a truncated stream never reads as `Complete`, including on the embedded path, which has no `should_stop`.
- **Setting.** `GG_CORE_STREAM_STALL_MS` is carried in `IpcServerConfig::stream_stall`. That one field feeds both `IpcHandlerConfig` (daemon) and `Runtime::infer_stream` (embedded). `config validate` warns when the raw value is below the 100 ms floor.
- **Telemetry.** The failure reason is the existing content-free error string ("stream consumer stalled"). No new metric label was added.
- **Daemon-level real-model check** (added after Phase 1): `ipc/server_real_model_tests.rs` runs a stalled reader and a second client through the real connection loop and worker.

## Implementation notes (Phase 2)
- **Precondition, static review.** `LlamaBackendInner` holds `&'static LlamaBackend`, an immutable `LlamaModel` and plain `Copy` configuration. llama-cpp-2 0.1.161 itself declares `unsafe impl Send/Sync for LlamaModel` (`model.rs:35-37`). Every `&self` generation path (`generate_cancellable`, `generate_stream`, `sample_loop`, tokenize/detokenize) creates its own `LlamaContext`, `LlamaBatch` and `LlamaSampler`, and only reads the model and its vocabulary. No interior mutability, statics or thread-locals are on the release generation path. The only `Mutex`-guarded state, the speculative session, belongs to the `advanced` feature, which is outside the release feature set.
- **Precondition, measured.** `tests/concurrent_generation_real_model_test.rs` checks that greedy, seeded generations of two prompts run concurrently on one loaded model, in three rounds, are byte-identical to the serial outputs. The VM result is recorded in the PR. Because the precondition holds, the per-model `Semaphore(1)` fallback is not used.
- **Implemented as specified.**
  - `scheduler/worker_pool.rs` (122 lines) spawns `max_concurrent` loops (1 without limits) into a `JoinSet` under one supervisor handle and one shutdown token. Every request still goes through `acquire_guard`.
  - Each loop round-robins streaming → typed → main, starting after the queue it served last. The existing shared `Notify` (`notify_one` per enqueue, checked before every wait) wakes one idle loop per arrival.
  - There is no thread cap (revision 2, V4).
  - `ResourceLimits::max_concurrent()` is the only new accessor.
- **Deviation (documentation location only).** The plan says to document CPU oversubscription in `RELEASE_SUPPORT_AND_MAINTENANCE.md`, but that file is the RC1 support guide, and v1.0.0 behaviour there would mislead RC1 users. The note is in `COREFORGE_PRODUCER_CONTRACT.md` (§Concurrency) and in the `GG_CORE_MAX_CONCURRENT` row of `config.rs`. It was re-audited (see the audit addendum).
- **Behaviour change to note.** The default daemon (`GG_CORE_MAX_CONCURRENT=2`) now runs two requests at once. `GG_CORE_MAX_CONCURRENT=1` restores serial execution.

## Addendum A: per-context thread policy (revision 5, 2026-11-09)

**Risk grade: L2.** This changes a performance default only. It changes no auth, IPC, memory admission or containment. **Gate: a `/ql-audit` PASS is required before implementation.**
**Authority:** the owner approved planning this policy for v1 after the #177 measurements (`V1_SCHEDULER_HOL_BENCHMARKS.md`). It supersedes the "no per-context thread policy" text in revision 2 (V4) and in the Phase 2 notes, and nothing else.
**Branch:** `feat/v1-thread-policy-20261109-no-ci`, stacked on #177's head `51d0fa4`. It is a separate draft PR, with `[skip ci]` commits.

### Problem (measured)
- `resolve_threads(0)` (`engine/gguf/backend.rs:541`) gives every context `min(logical, 16)` threads, without regard to `max_concurrent`. With N workers that is 12·N threads on 12 vCPUs.
- With the default thread count, aggregate throughput barely moves with N (82.5 → 99.9 tok/s from 1 to 8 workers).
- Explicit 12/N threads gave +25 %, +57 % and +60 % at N = 2, 4 and 8, cut p95 TTFT by 13–51 %, and kept every stream byte-identical to serial.

### Policy
- **Auto (`GG_CORE_N_THREADS` unset or 0):** threads per context = `max(1, min(logical, 16) / workers)`, using integer division.
  - `workers` is the number of contexts the runtime can actually run at once on this load path (revision 6, V7):
    - **Daemon** (startup preload and authenticated IPC load, which pass it explicitly into `model_ops`): `ResourceLimits::max_concurrent()`, or 1 without limits. This is the same rule `worker_pool::spawn` uses, and in the daemon every GGUF request runs on one of those loops.
    - **Embedded FFI/Python** (`ffi/models.rs`, `python/session.rs`): 1. Embedded `Runtime::infer`/`infer_stream` call the engine directly. They are not bounded by `max_concurrent`, and the embedder decides concurrency, so dividing by `max_concurrent` would halve a serial embedder's threads for no benefit. Embedders who run concurrent requests set `GG_CORE_N_THREADS`. Today's embedded behaviour is unchanged.
  - The existing 16-thread cap on `logical` stays, so N = 1 is unchanged from today on every host.
  - On 12 vCPUs this gives 12, 6, 3 and 1 threads at N = 1, 2, 4 and 8, which are the measured configurations.
- **Explicit override (`GG_CORE_N_THREADS` > 0):** this value is used per context, exactly as today. It is not divided by `workers`. Operators who want oversubscription, or a different split, set it.
- **Resolution time:** threads are resolved once per model load, as today (`LlamaBackendInner::load`). `max_concurrent` is fixed for the life of a runtime, so a model's thread count always matches the pool that runs it.

### Design
1. **New `engine/gguf/threads.rs`** (≤ 80 lines including tests; not feature-gated, so it is tested in default builds). It holds:
   - `pub const MAX_AUTO_THREADS: usize = 16`;
   - `pub fn per_context(explicit: u32, workers: usize, logical: usize) -> u32`, the pure policy above (`workers` 0 is treated as 1, and the result is always ≥ 1);
   - `pub fn per_context_for_host(explicit: u32, workers: usize) -> u32`, which calls `per_context` with `num_cpus::get()`.
2. **`ResourceLimits::worker_loops(limits: Option<&ResourceLimits>) -> usize`** in `memory/limits.rs` returns `max_concurrent` or 1. `worker_pool::spawn` uses it in place of its inline `map_or`, so the pool and the thread policy share one definition.
3. **`load_model_dispatch(path, id, workers)`**: it gains a `workers` argument, and `gguf_load_config(env, workers)` sets `GgufConfig::n_threads = threads::per_context_for_host(env.n_threads, workers)`. The ONNX path ignores it.
   - `model_ops::prepare_model_admitted` and `model_ops::load_model_from_path` gain an explicit `workers: usize` argument. They do not infer it from `limits`, because `load_model_from_path` is public and a Rust embedder that pairs it with `Runtime::infer` is not pool-bounded (V7).
     - The daemon callers pass `ResourceLimits::worker_loops(Some(&runtime.resource_limits))`: `runtime_init::preload_models` and `ipc/handler_models.rs` (its `limits` is the same lifecycle object, `lib.rs:170`). That is the object the daemon hands to the worker pool (`runtime_init.rs:276`), so the thread count and the loop count share one source.
     - `model_ops::prepare_model` (no limits) and the tests pass 1.
   - `ffi/models.rs` and `python/session.rs` pass 1 (V7), each with a comment that names the reason.
4. **`backend.rs::resolve_threads(0)`** delegates to `threads::per_context_for_host(0, 1)`. Direct `GgufConfig` users (tests, examples) keep today's behaviour, and the policy has one source.
5. **Operator visibility:** `gg-core-cli config show` prints the resolved value next to the raw one, for example `GG_CORE_N_THREADS=0 (auto: 6 threads per context = min(12 logical, 16) / 2 workers)`. `config defaults` prints `GG_CORE_N_THREADS=0 (auto: min(logical, 16) / GG_CORE_MAX_CONCURRENT, at least 1)`.
6. **Docs:**
   - the `GG_CORE_N_THREADS` row in `config.rs`;
   - `COREFORGE_PRODUCER_CONTRACT.md` §Concurrency, which replaces "There is no per-context thread cap" and states that the embedded path keeps full-core contexts unless `GG_CORE_N_THREADS` is set;
   - `USAGE_GUIDE.md` row;
   - this plan's implementation notes.

### Security and containment review
- **No new surface:** no new environment variable, IPC message, dependency, unsafe code, network path or file access. `num_cpus` is already an unconditional dependency.
- **Arithmetic safety:** `workers` is clamped to ≥ 1, so there is no division by zero. The auto result is in [1, 16]. The explicit path is unchanged, including its existing `i32::try_from` fallback.
- **Memory:** unchanged. Admission is per context working memory, which does not depend on the thread count. llama.cpp's per-thread scratch is inside the estimate's fixed 32 MiB `OVERHEAD` term (`working_memory.rs`, "output buffer, graph metadata, threads"). That term was sized when every context ran up to 16 threads, and the policy only ever lowers the auto count (it is never above today's value), so the estimate stays an upper bound. The Job Object cap is untouched.
- **Availability and DoS:** this improves both. N concurrent requests no longer contend for 12·N runnable threads, so one long generation slows the others less. The worst-case hold bounds from Phase 1 and Phase 2 are unchanged.
- **Determinism:** greedy outputs were byte-identical across 12, 6, 3 and 1 threads in the #177 sweep. The VM gate below re-checks this on the implemented policy. If a model's output ever differs with thread count, that is a numerical property of llama.cpp, not a containment issue, and the explicit override restores any fixed count.
- **Platform honesty:** no OS-confinement claim changes. This is not a CPU quota: a Job Object CPU-rate limit is still not set.

### Razor
- `threads.rs` is new and ≤ 80 lines.
- `backend_dispatch.rs` (236) stays ≤ 250: its inline config test moves into `backend_dispatch_tests.rs` if needed.
- `config_cmd.rs` (239) stays ≤ 250: the auto-description helper lives in `threads.rs`.
- `config.rs` (246) only gets a doc-row text change.
- `backend.rs` (678, pre-existing) only shrinks.
- `limits.rs` (189) gains about 6 lines.
- `model_ops.rs` (226), `handler_models.rs` (55), `ffi/models.rs` (228) and `python/session.rs` (221) each gain only an argument and a comment, and stay ≤ 250.
- `runtime_init.rs` (316, pre-existing) gains one call argument and no new lines.
- Every function is ≤ 40 lines with nesting ≤ 3.

### Tests
- **Unit (`threads.rs`):**
  - explicit > 0 is returned unchanged for any `workers`;
  - auto on 12 logical gives 12/6/3/1 at 1/2/4/8 workers;
  - workers > cores gives 1;
  - workers 0 is treated as 1;
  - logical 32 is capped at 16 (N = 1 → 16, N = 2 → 8);
  - logical 0 or 1 gives 1.
- **Dispatch:** `gguf_load_config` with `n_threads` 0 and 4 workers yields the policy value; with `n_threads` 6 it yields 6 for any `workers`.
- **Embedded (V7):** the FFI and Python load paths pass `workers = 1`. This is pinned by a source-level test in `backend_dispatch_tests.rs`: with `n_threads` 0 and `workers` 1, `gguf_load_config` equals today's `min(logical, 16)`.
- **Pool:** `worker_loops(None)` is 1 and `worker_loops(Some(limits))` is `max_concurrent`. The existing pool tests stay green.
- **CLI:** the `config show` line contains the resolved count; `config defaults` describes the policy.
- **Offline VM (real models, release build), same harness and workload as `V1_SCHEDULER_HOL_BENCHMARKS.md`:**
  - the concurrency sweep at N = 1/2/4/8 with `GG_CORE_N_THREADS` unset, compared with the earlier default-thread and explicit 12/N rows. The pass criterion is: every stream byte-identical to serial, no request errors, and aggregate throughput at N ≥ 2 not below the earlier default-thread rows;
  - TTFT and total latency p50/p95, and peak private bytes, per N;
  - the multi-model run at N = 2 and 4: 0 refusals, 0 failures, 0 allocation failures and no idle drift;
  - one explicit-override row (`GG_CORE_N_THREADS=12`, N = 4) to show that the override is honoured.

### Validation
- On the host: `cargo fmt --check`, `cargo clippy --all-targets --features onnx,gguf,ffi -D warnings`, and the unit tests.
- In the `coreforge-build` VM (`ggv1*` paths only, after the owner session's "VM free"): the full test suite, then the benchmark harness.
- No GitHub Actions. The PR stays a draft.


### Implementation notes (Addendum A)
- **Implemented as specified (revision 6).**
  - `engine/gguf/threads.rs` (79 lines) holds `per_context`, `per_context_for_host` and `describe`. `MAX_AUTO_THREADS` is module-private, so cbindgen does not export it into `gg_core.h`.
  - `ResourceLimits::worker_loops` is used by `worker_pool::spawn`, `runtime_init::preload_models` and `ipc/handler_models.rs`.
  - `load_model_dispatch`, `prepare_model_admitted` and `load_model_from_path` take `workers`.
  - FFI and Python pass 1, as do `prepare_model` and the tests. `resolve_threads` delegates to the policy.
- **Test location.** The dispatch config tests moved from an inline module into `models/backend_dispatch_config_tests.rs`, so `backend_dispatch.rs` stays at 223 lines and the pre-existing 417-line `backend_dispatch_tests.rs` does not grow. The embedded-unchanged check (`workers = 1` equals `min(logical, 16)`) is there.
- **Public API change.** `models::load_model_from_path` and `models::load_model_dispatch` gain a trailing `workers: usize` argument. The `USAGE_GUIDE.md` embedded example and `tests/doc_examples.rs` pass 1.
- **Razor.**
  - Every function is ≤ 40 lines with nesting ≤ 3.
  - `runtime_init.rs` went from 316 to 314 lines and `backend.rs` from 678 to 671.
- **Host validation.** On Windows, without `protoc` or a llama.cpp toolchain:
  - `cargo fmt --check`: clean.
  - `clippy --all-targets --features ffi -D warnings`: clean, apart from the pre-existing host-toolchain `fetch_update` deprecation in `memory/resident.rs`, which is allowed here and absent on the VM's Rust 1.97.
  - `cargo test --features ffi --lib`: 753 passed.
  - `doc_examples`: 3 passed.
  - The GGUF/ONNX build and the measurements run in the VM.
- **Rust 1.99.1 (CI toolchain) in the VM**, rebased on #178 head `aaba859`:
  - `fmt --check` is clean.
  - `clippy --all-targets -D warnings` is clean for default, `advanced`, `ffi`, `onnx` and `gguf`.
  - Tests:

| Feature set | Passed | Failed | Ignored |
|---|---|---|---|
| default | 1434 | 0 | 0 |
| `advanced` | 1569 | 0 | 3 |
| `onnx,gguf,ffi` | 1516 | 0 | 1 (pre-existing) |
