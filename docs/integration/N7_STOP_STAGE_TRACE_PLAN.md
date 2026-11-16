# N7 stop-stage instrumentation and re-measurement: plan (#184)

**Risk:** L2. Adds an opt-in, default-off diagnostic output to the daemon plus backend stop-observation events. No change to inference, scheduling, admission, authentication, IPC frames or limits.
**Base:** `main` at `1032d1ba` (contains #195 Rev 4: deterministic prefill schedule, 1 s chunk budget, sub-64 remainder split).

## Why
#184 acceptance item 1 requires separately timestamped stages: cancel arrival, daemon acknowledgment, stop observed by the GGUF backend, last native work, worker release, CPU quiescence. On `main`:
- Only two events exist (`stop answered to caller`, `worker released after stop`), non-streaming only.
- The packaged daemon never installs a tracing subscriber, so even those are inert (recorded as limitation 4 of the C2 report). The `RUST_LOG` line in `print_usage` is not read by the daemon either.
- Nothing records when the backend actually observed the stop, or how long the in-flight native decode ran.

Without those, the #195 tail numbers are total wall time from the harness and cannot separate "in-flight chunk" from "other work after stop".

## Design
1. **Opt-in stop trace** (new `telemetry/stop_trace.rs`).
   - `GG_CORE_STOP_TRACE=1` (exactly `1`; anything else, or unset, is off) makes `serve` install one global subscriber before preload.
   - Output: JSON lines to **stderr only**. No file path is accepted, so the variable cannot be used to write anywhere.
   - Filter is hard-coded `Targets` = `gg_core::stop` at INFO. It does not take a user filter string, so enabling it never surfaces any other target's events (which could carry model ids, paths or errors).
   - Timer: `fmt::time::Uptime`, monotonic seconds since the subscriber was installed.
   - Spans: the current span's fields are included, which carry `queue_id`.
   - If a subscriber is already installed (embedders), it does nothing and returns `false`.
2. **Stage events**, all target `gg_core::stop`, INFO. Fields are integers, booleans and fixed strings only: never prompt text, tokens, output, auth tokens, session ids, model paths or error strings beyond the stop kind.

   | Stage | Where | Fields |
   |---|---|---|
   | S1 `cancel request received` | `ipc/server.rs`, authenticated `cancel_request` branch | `request_id`, `matched` |
   | S2 `stop answered to caller` | `handler_inference.rs` (exists) | `request_id`, `queue_id`, `kind` (`cancelled` / `deadline_exceeded`) instead of the error string |
   | S3 `backend stop observed` | GGUF prefill chunk loop and both token loops | `phase` (`prefill` / `decode`), `position`, `remaining` (prefill) or `generated` (decode), `last_decode_tokens`, `last_decode_ms` |
   | S4 `worker released after stop` | `worker.rs` (exists), plus the streaming worker | as today, plus `stream` bool |
   | S5 CPU quiescence | external: the packaged harness samples daemon process CPU | — |

   - **Correlation:** the worker enters an INFO span `stop_scope { queue_id }` (target `gg_core::stop`) inside each `spawn_blocking` closure, so S3 inherits `queue_id` from the span. With no subscriber the callsite is disabled and costs one relaxed atomic load.
   - **S3 cost:** one `Instant::now()` per decode call (prefill chunk and generated token) to report `last_decode_ms`. That is on the order of tens of nanoseconds against decode calls of 20 ms or more.
   - `backend.rs` (600 lines, already over the limit) gains only one-line calls; the helper lives in a new `engine/gguf/stop_observe.rs`.
3. **`print_usage`:** replace the inert `RUST_LOG` line with `GG_CORE_STOP_TRACE=1` and a description of what it emits. This is the only help change, and the reason is that the existing line documents a setting the daemon does not read.

## Re-measurement (the reproduction #184 asks for)
Extend `scripts/qualification/n7_packaged_stop_tail.ps1`:
- Launch the daemon with `GG_CORE_STOP_TRACE=1` and stderr redirected to a per-run file, then join S1–S4 by `request_id` / `queue_id`.
- Per stop, report:
  - ack latency (S1→S2);
  - backend observation delay (S2→S3);
  - `last_decode_ms`;
  - release delay (S3→S4);
  - quiescence (stop sent→CPU quiet, existing sampler).
- Add scenarios: cancel during **decode** (short prompt, long `max_tokens`), and a **queued** cancel while both workers are busy.
- Matrix: Qwen2.5-0.5B Q4_K_M, contexts 8192 / 16384, threads 1 / 2 / auto, 3 repetitions.
- Daemons:
  - `main` `1032d1ba`, trace off;
  - new, trace off (throughput and output-hash control);
  - new, trace on.

**Decision rule for a compute change:** if S2→S3 is dominated by `last_decode_ms` (an in-flight native chunk or token), the #195 schedule is already the minimal cooperative fix. Further reduction then needs a native abort callback, which `llama-cpp-2` 0.1.161 does not expose (`LlamaContext.context` and `LlamaContextParams.context_params` are `pub(crate)`). That would be a wrapper change for the owner, not part of this branch. Only if S3→S4 or other post-observation work is material is a code fix in scope here, and it gets its own plan revision.

## Tests
- **Unit:**
  - `enabled()` parse: `1` → on; unset, `0`, `true` or empty → off.
  - The subscriber built by the module, with a captured writer, emits `gg_core::stop` events and drops every other target. The output contains the span's `queue_id`.
  - Install-twice returns `false`.
- **Worker:** a cancelled non-streaming request and a cancelled streaming request, using existing stub models, emit S4 with the request's `queue_id`. S2 and S4 share it. No prompt text appears in the captured output.
- **Real model** (`GG_CORE_REQUIRE_REAL_MODEL` gate, existing fixture): a stop raised during a 1-thread multi-chunk prefill emits exactly one S3 with `phase=prefill`, `remaining > 0` and `last_decode_tokens` equal to a scheduled chunk length. A stop during generation emits `phase=decode`.
- All existing suites unchanged.

## Razor and compatibility
- New files stay at 250 lines or fewer and functions at 40 or fewer.
- No IPC, config-file, dependency or feature change.
- `tracing-subscriber` (json) is already a dependency.

## Not in scope
- A hard native interrupt.
- ONNX, typed requests, GPU.
- CoreForge.
- Any default-on logging.

## Revision 2: remediation of audit VETO (findings 1–10)

Where this section differs from the design above, this section governs.

**F1: non-blocking output.**
- The subscriber writes through a `MakeWriter` that `try_send`s each formatted line into a bounded `std::sync::mpsc::sync_channel` (1024 lines). One dedicated thread named `gg-stop-trace` writes the lines to stderr.
- A full channel drops the line and increments an `AtomicU64`. Every event carries `dropped`, the count so far.
- No emitting thread ever blocks on stderr.
- Unit test: a sink that never drains, 5,000 emits return within a bound, and `dropped > 0`.
- The daemon's existing `eprintln!` lines and llama.cpp's native stderr logging have the same pipe-backpressure exposure today. They are unchanged and out of scope.

**F2: exact filter, no log bridge.**
- The filter is `filter_fn(|m| m.target() == "gg_core::stop")`, an exact match applied to spans and events. `Targets` is not used.
- Install goes through `tracing::subscriber::set_global_default`, so no `LogTracer` is installed.
- Tests:
  - `gg_core::stopx` and `gg_core::stop::x` are dropped;
  - a `log`-crate record is not captured.
- **The `.err` file is mixed output.** It also contains llama.cpp native logs, which include the model path, and `eprintln!` lines.
  - The report script keeps only JSON lines whose `target` is `gg_core::stop`.
  - Raw `.err` files are never committed. Committed evidence is the filtered stage JSONL plus `results.jsonl`.

**F3: streaming coverage and correlation.**
- The span is `stop_scope { queue_id, stream }`. `stream` also appears on S2, S3 and S4.
- **Non-streaming:** the worker builds the span, enters it inside `spawn_blocking`, and wraps the future with `.instrument(span)`.
- **Streaming:** the span uses the streaming queue's id.
  - S2 is emitted by `stream_relay` when it writes the cancel or deadline abort frame: `request_id`, `stream=true`, `kind`.
  - Streaming S4 is decided by `control.should_stop()` after `run_stream` returns, not by the result.
- The two queues assign ids independently, so `queue_id` is unique only within one value of `stream`.
- **Joins:**
  - non-streaming: request_id → queue_id via S2;
  - streaming: S2 → S3/S4 by order, because the relay does not know the worker's queue id.
- `request_id` is client-chosen and unique only per connection. Joins are therefore valid for single-client, one-stop-at-a-time measurement, which is what the harness does. The evidence states this.

**F4: common clock.**
- At install, the subscriber emits **S0 `stop trace started`** with `unix_ms`, the system clock at that instant. Daemon uptime plus S0 maps every stage to unix ms.
- The harness records `stopSentUnixMs` and `quietUnixMs` per scenario, so S4 and CPU quiescence share one axis.
- **Precision:** both clocks are the same host's system clock. Error is expected to be in single-digit milliseconds; it is stated, not assumed zero.

**F5: Razor, `backend.rs` net ≤ 0 lines.**
- Timing lives in `backend::decode` itself. Its body becomes a call to `stop_observe::timed_decode(ctx, batch)`, which records the duration and token count of the last decode in a thread-local. Every prefill chunk and generated token passes through `decode`.
- The stop checks at `backend.rs` 109, 137, 157 and 392 replace their condition in place with `stop_observe::stopped(is_cancelled, phase, n)`. That emits S3 and resets the thread-local, same line count.
- `prefill.rs`'s check calls the same helper with `phase=prefill`.
- No function that is already over 40 lines gets longer. The line deltas are recorded in the evidence.
- A thread-local is safe here: one request runs on one blocking thread from setup to return. It is reset at every S3 and at the setup check, so a previous request's value is never reported. With `phase=setup`, `last_decode_*` is reported as null.

**F6: decision rule and what counts as relinquished.**
- **"Dominated" is a number:** S2→S3 ≤ `last_decode_ms` + 5 ms in at least 95% of traced stops.
- Ack latency (S1→S2) and the cooperative schedule are **not** evidence that compute stopped. The #184 claim rests only on:
  - **S4**, worker released. The llama context and KV cache are locals of the backend call and are dropped before it returns, so S4 is after teardown.
  - **S5**, process CPU below the quiet threshold, measured externally. It covers any ggml thread spin.
- S3 is "stop observed by the backend", not "last native work". Native work is bounded by S4 and S5. No separate context-drop event is added.

**F7: overhead control.**
- The batch runs `new-off` and `new-on` interleaved per configuration.
- **Pre-stated tolerances:**
  - uncancelled output hashes identical;
  - throughput medians within ±3%.
- Quiescence from trace-on runs is reported only alongside trace-off quiescence, and claimed as representative only if the worst values agree within 100 ms (one sampler period). Otherwise only trace-off tails are cited, and the stage deltas are labelled as perturbed.

**F8: S1.** S1 is emitted only after `auth.validate` succeeds. `active_requests` is per connection, so `matched` reveals nothing about other connections. Unit test covers `matched=false`.

**F9: stop points.**
- The setup checks before context creation emit S3 with `phase=setup`.
- Stream receiver disconnects (the `emit_token` `false` branch) are **untraced** and listed as such.

**F10: not in scope** (in addition to the list above):
- **Stops caused by client disconnect, or by deadline:** neither has an S1. A deadline has S2 with `kind=deadline_exceeded`.
- **The speculative path:** its `generate_from_tokens` loop has no stop check. It runs only when adaptive speculative config is set, which is off by default and has no `serve` environment switch. It is untouched.
- **ggml threadpool spin:** covered only by S5.
- **A native abort callback:** needs an owner decision on the `llama-cpp-2` wrapper.

## Audit record
- **Revision 1: VETO.** Findings F1–F10:
  - blocking stderr;
  - prefix filter and `LogTracer` install;
  - streaming correlation;
  - no common clock;
  - `backend.rs` Razor;
  - a vague decision rule;
  - overhead control;
  - S1 auth placement;
  - untraced setup stops;
  - scope list.
- **Revision 2: PASS.** Binding conditions:

| Condition | Required | Implementation |
|---|---|---|
| A1 | Reset the stop-observation thread-local on every request entry, whether or not a stop is found; test an unstopped request followed by a request stopped before its first decode on the same thread | `stop_observe::setup_stop` resets, then checks; it replaces `stop_requested` at both entry checks |
| A2 | Runs whose stage records show `dropped > 0` are marked incomplete and excluded from stage statistics | `n7_stop_stage_report.py` |

## Implementation notes (where the code differs from the text above)
- **S3 field names:** `phase`, `done` (prompt position for prefill, tokens generated for decode, 0 for setup), `remaining` (prefill only), `last_decode_tokens`, `last_decode_ms`, `dropped`. These replace `position` / `generated` in the design table.
- **Overhead with the trace off:** `timed_decode` always takes two `Instant::now()` readings and writes one thread-local per native decode. That costs tens of nanoseconds against decodes of 20 ms or more. It is more than the "one relaxed atomic load" stated in Design item 2, which applies only to the disabled event and span callsites.
- **Install-twice test: dropped.** A process-wide install would conflict with the test-only global capture (`stop_trace::test_capture`) that the cross-thread correlation tests need. Install semantics are `tracing::subscriber::set_global_default`'s.
- **`log`-crate bridge test: replaced by inspection.** `log` is not a direct dependency, and adding a dev-dependency would change `Cargo.lock`, which the offline VM cannot resolve. No `LogTracer` is installed because install goes through `set_global_default`, not `try_init`.
- **A stop seen by the engine's pre-check** (`InferenceEngine::run_controlled`) before the backend is called produces S2 and S4 with no S3. The report tolerates it.

## Independent review (2026-11-14): APPROVE WITH CONDITIONS, all fixed
| Finding | Fix |
|---|---|
| C1 Razor: long functions grew | S1 and S2 moved into `stop_trace::{cancel_received, answered}`. The streaming span and S4 moved into `worker_streaming::traced`. `StreamControl::of` replaces inline construction. Function and file deltas against `1032d1b` are listed after this table. |
| C2 S1 emitted after the cancel, so S2 could precede it | The token is cloned out of the map, S1 is emitted, then the cancel is signalled. The IPC test asserts S1 is captured before S2. |
| C3 worker test could match another queue's id 1 | One predicate over message, `queue_id`, `stream == false` and `cancel_flag == true`, after the request's own marker. |
| C4 A2 under-detects drops of the run's own lines | The report marks a run incomplete if the daemon's trace shows any drop, or if a required stage is missing (S4 for running stops; no S3 or S4 for queued stops). |
| Streaming S4 could be spurious once a deadline passed after a natural finish | S4 is decided inside the blocking closure the moment `run_stream_sync` returns. |
| Writer thread spawned after install | Spawned first, so a subscriber is never installed without its drain. |
| Queued scenario could stop being queued at auto threads | The busy requests use the long prompt. The report counts queued cancels that reached the backend; the expected count is 0. |
| Report: label parse, stream filter, F7 not evaluated | `rsplit` tag parse, `stream` filter, trace on/off throughput ratio and output-hash checks printed. |

**Deltas against `1032d1b`:**
- **Files:** `backend.rs` 600 → 600; `server.rs` 362 → 358.
- **Functions:**
  - `worker_streaming::execute` 58 → 56;
  - `run_stream` 46 → 46;
  - `handle_inference_controlled` 66 → 66;
  - `handle_connection` 209 → 205.
- **New code:** all new functions are 40 lines or fewer, and all new files 250 lines or fewer.
