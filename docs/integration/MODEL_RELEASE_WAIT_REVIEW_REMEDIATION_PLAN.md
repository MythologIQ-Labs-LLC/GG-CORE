# Model release-wait review remediation: L2 addendum to #183

**Status:** implementation candidate, **not yet governed or qualified**. Independent L2 audit required before integration/merge.
**Source:** draft #183 `cddb8bbddda3853f9aac3684e2f3b4a70858f0c1`.
**Candidate branch:** `fix/model-release-review-remediation-20261112-no-ci`.
**Scope:** only the two verified behavioral issues in the prior independent review. No new model lifecycle authority, native backend, dependencies, network APIs or release.
**Separate owner disposition:** resolved 2026-11-13. `IpcMessage` becomes `#[non_exhaustive]` (see Owner dispositions), recorded on #112.

## Finding R1: reply-order race

Previously, `Wait::run` set `pending=false` immediately after `wait_released` completed, **before** awaiting the actual `ConnWriter::write_frame`. A subsequent wait could be admitted on the same connection while the previous response remained queued behind a stalled writer. Because the `ModelReleaseWaitResponse` wire body carries only a model ID, not a unique request ID, this could create uncertain response correlation.

**Change:** hold `pending=true` until the first response's write attempt finishes (success, IO failure or stall), then clear the gate. The writer already poisons the connection on IO failure/stall; the waiter is terminated on disconnect. Retain authentication and one pending waiter per connection. No IPC schema changes.

**Regression:** `server_release_order_tests.rs` holds the live writer lock while an authenticated `model_release_wait_request` completes. Test-only event notifications observe the moment a first result is ready and a second request is rejected; no arbitrary sleeps are needed. It must observe exactly one release response for the first model, one 409 for a different second model, and then a successful third wait after the writer drains.

**Risk:** a 409 frame may itself wait for the shared writer and can arrive before or after the first successful wait response. This change guarantees only that **two successful waits cannot overlap**, not FIFO order of all frame types. The wire 409 lacks a request ID, so clients must not pipeline release waits; independent concurrent waits require separate authenticated connections. Ensure stall poison closes the connection as before. Verify release-wait `LIVE_WAITS` and per-connection `pending` are fully cleared after completion, never permitting a second pending worker on the same connection.

## Finding R1a: duplicate-rejection backpressure can block cancel (adversarial review)

**Status: source-confirmed risk; NOT fixed by the existing implementation candidate.** In `ipc/server_release.rs::start`, the duplicate-wait branch does `writer.write_frame(409).await`. The server's `handle_connection` loop itself awaits `release::start`, so a blocked error write stalls **the only reader task** and prevents the next `cancel_request` on that same connection from being serviced until the error write completes or reaches the writer stall timeout. This is different from the first release wait, which executes in a spawned task. The existing `server_release_tests::cancel_is_served_during_a_wait_which_then_reports_released` drains the 409 *before* it sends cancellation, and the new `server_release_order_tests` drives `start` in a separate task; neither establishes cancellation progress under actual reader backpressure.

**Safe design alternatives require L2 review, rather than a casual async patch:**
- If duplicate rejection is moved to a bounded spawned responder, allow **at most one pending duplicate-error writer per connection**. Define what happens on subsequent duplicate requests (close/refuse deterministically); otherwise an abusive client can queue unlimited tasks behind the connection writer.
- Preserve `ConnWriter`'s all-or-nothing *ownership* of a frame until it finishes or poisons the connection. Do **not** wrap an in-progress `write_frame` in a `tokio::select!` cancellation branch: dropping that future after a partial frame could leave the connection unpoisoned and corrupt the next response.
- Test a real authenticated `handle_connection` with intentionally backpressured outgoing frames: first wait active, second wait gets queued 409, then same-connection cancel, while verifying **backend stop progress before draining the 409**, eventual exactly-once terminal response, bounded queued responders, and cleanup after disconnect. Also assert a connection close during a reply waiting on a held writer does not strand release tasks beyond the declared writer-stall bound.
- Alternatively adopt a documented fail-closed protocol policy that terminates the connection on overlapping wait requests, preserving cancellation of its in-flight work without waiting for the rejected reply; that changes the previously accepted 409/keep-open contract and therefore also requires explicit compatibility review.

**Qualification gate:** R1a must be remediated and independently validated, or the owner must explicitly accept and document the narrower contract and the measured stall bound. Do not call this branch cancellation-safe under all duplicate-wait / non-reading-client states as currently written.

## Finding R2: false CLI success for a timed-out release wait

Original `MODEL_RELEASE_V1_PLAN.md` §3.4 **explicitly specified exit 0 for both `released:true` and `released:false`**. That can mislead a shell automation that issued `models unload ID --wait-ms N` specifically to ensure reclaim before loading another model. Changing it is a **governance and CLI contract deviation**, not an unchanged implementation detail.

**Proposed changed contract:**

| CLI operation/result | Exit status |
| --- | ---: |
| `models unload ID` without `--wait-ms` | original unchanged |
| `models unload ID --wait-ms N`, `released:true` | 0 |
| `models unload ID --wait-ms N`, `released:false` | 4 |
| Unload/auth/protocol failure | 1 |
| IPC connection failure | 3 |

When pending, the CLI prints the remaining instance count to stderr; the same unmodified IPC response continues to carry `released:false, pending_instances`. The CLI validates that the unload acknowledgement and release receipt refer to the requested model, and that `released` exactly agrees with whether `pending_instances` is zero. Mismatched or contradictory replies fail closed with exit `1` and cannot claim memory release. The existing plain unload semantics and public function signatures are not changed. Update CLI help and protocol documentation. **L2 auditor and owner must accept this CLI behavior change before promoting it.**

## Required audit and qualification gates

1. **Independent L2 audit on the exact revised plan + implementation diff.** Check deterministic rejection and response ordering, authorization and release-state trust, pending clearing after error/stall/disconnect, and the explicitly altered CLI exit semantics. VETO means no integration until remediated and re-audited.
2. **Source tests:** rustfmt; Clippy warnings-as-errors for default, advanced, ffi, onnx and gguf; default/advanced/full (onnx,gguf,ffi) test suites. The new tests run without real model fixtures; the existing real-model suite must run with `GG_CORE_REQUIRE_REAL_MODEL=1` on the qualifying host.
3. **Packaged IPC:** authenticated concurrent model hold → unload → timeout pending (exit **4**) → release → retry wait/confirm exit **0**. Check wrong-token refusal, one terminal reply per operation, re-entry after previous frame, cancellation during wait, slot/ResidentGuard release and no ghost `unloading` entry. Retain measured bytes/handles acceptance from #183.
4. **Integration with current #195/#193:** Do not assume a clean merge from PR badges. On the offline build host make a disposable merge tree (order: qualified #195, qualified #193, then #183+this remediation), capture exact source SHAs, run all gates and compare compatibility and resource behavior. Preserve ancestor `55d9fd0` and C2 binary unchanged.
5. **External Rust source compatibility (resolved by owner disposition 2, below):** #183 introduces variants into public exhaustive `IpcMessage`. The existing downstream compile regression does **not** test exhaustively matching that enum. Resolve #112 policy intentionally, with an external-consumer compile receipt, before asserting v1 source compatibility. Do not quietly mark this point as fixed by the behavioral remediation.
6. **Authority:** do not open an overlapping second PR, run paid GitHub Actions, merge protected `main`, sign, tag, publish or change CoreForge pins without explicit authorization. This document is not a claim of PASS, native test execution, or release GO.

**Completion condition:** revised L2 PASS; independent source review; verified real binary tests and integration receipts for one exact immutable head; owner acceptance of any remaining public-contract deviation; then fold the qualified diff into existing #183 rather than creating a competing PR.

## Owner dispositions (2026-11-13, recorded from the owner's answers in session)
1. **R1a: accept the bound and add a test.** A same-connection cancel can wait behind a blocked inline reply for at most `GG_CORE_STREAM_STALL_MS` (default 5 s, minimum 100 ms).
   - **Restated after audit L2-R2-1 (finding 4).** "At most the bound" holds for a client that stops reading entirely. An inline write first waits behind frames already queued for the connection, each bounded by the stall. So a client that keeps reading slowly can delay its own control frames by the number of queued frames times the bound. Once the bound is reached, a queued `cancel_request` is never read: teardown cancels the request, and no `cancel_response` is sent.
   - This applies to every inline reply the reader writes, not only the duplicate-wait 409: `cancel_response`, 400/401/409 errors and ordinary handler responses. It is a pre-existing connection-wide property, not one introduced by #183.
   - When the bound is reached, `ConnWriter` poisons the connection. The reader loop exits on `write_half.closed()`, and the exit path cancels every active request and calls `release_waits.close()`.
   - No spawned 409 responder is added.
2. **Finding 3: `IpcMessage` becomes `#[non_exhaustive]` in this change.** This is a single source break before v1.0.0.
   - `main` already broke exhaustive matches against `v0.10.0-rc.1` by adding `TypedInferenceRequest` and `TypedInferenceResponse` (#171). #183 adds `ModelReleaseWaitRequest` and `ModelReleaseWaitResponse`.
   - After this change, adding a variant is source-compatible for external crates, which must use a wildcard arm. The attribute does not restrict constructing variants.
   - Recorded on #112.
3. **R2: exit 4 accepted.** `models unload ID --wait-ms N` exits 0 only when `released:true`. It exits 4 when the wait expires with memory still held, and 1 on a wrong-model or contradictory receipt. Plain `models unload` is unchanged.

## Revision 2 (implementation scope after the dispositions)
- **R1a test** (new, in the release tests, driven through the real `handle_connection`):
  - **Setup:** a duplex stream with a small outbound buffer and a client that stops reading; `stream_stall` set to 300 ms through `RuntimeConfig.ipc_server`. The client authenticates and starts a held non-streaming request on the `Blocking` stub model. It starts a release wait for that model, then floods duplicate waits until the outbound buffer is full and the reader blocks on a 409. Finally it sends `cancel_request` for the held request.
  - **Assertion:** within the stall bound plus a margin (≤ 3 s), the connection task ends with the writer poisoned. The held request is cancelled: the model's instance is released and `lifecycle` shows no `unloading` ghost. `LIVE_WAITS` returns to its prior value. No task outlives the connection.
  - **Recorded measurement:** time from the start of the duplicate-wait flood (the client has stopped reading) to connection end, at the configured 300 ms stall. The default 5 s bound is documented, not measured. The mechanism is the same, at about 5 s instead of 0.3 s.
- **`#[non_exhaustive]` on `IpcMessage`:**
  - Every exhaustive `match` on `IpcMessage` outside `core-runtime/src` must gain a wildcard arm: integration tests, benches and any other workspace crate, found by compiling.
  - *(Superseded by audit condition C4: the doctest pair in `ipc::protocol_compat` plus an in-crate exhaustive guard replace this test.)* `tests/public_api_compat_test.rs` gains `ipc_message_is_non_exhaustive_for_external_crates`, an external-crate match with a wildcard arm. It compiles only because the arm exists; a comment states the policy.
  - **Docs:** `IPC_PROTOCOL_SCHEMA.md`, `RELEASE_SUPPORT_AND_MAINTENANCE.md` (Rust source compatibility note) and `CHANGELOG.md` record the one-time break and the policy.
- **R1 and R2:** the existing candidate code on this branch is kept as is (head `221289b7`), subject to the gates below.
- **Gates (replace items 1–2 above):**
  - L2 audit of this revision;
  - fmt and clippy `-D warnings` for 5 feature sets;
  - the default, advanced and full suites with `GG_CORE_REQUIRE_REAL_MODEL=1` on the offline VM;
  - the release build;
  - the packaged CLI exit-code check (0 when released, 4 when pending), using the existing #183 packaged acceptance harness where available;
  - independent review.
- **Unchanged:** no wire-format change, no new dependencies, no change to authentication, ancestry from `cddb8bbd` preserved, and no second PR. The qualified diff is folded into #183.

## Audit L2-R2-1 (independent Judge, 2026-11-13): PASS WITH CONDITIONS
- **Verified:**
  - The R1 fix is correct on every path (closed before reply, encode failure, write Ok/Io/Stalled/Poisoned). A task outlives its connection only while its reply write is in progress, which the writer stall bounds.
  - The order test is deterministic and exercises the race.
  - The R2 CLI mapping and receipt validation are correct, and plain unload is unchanged.
  - The R1a facts hold against the source.
  - Scope is clean.
- **Conditions and disposition:**

| Condition | Required | Disposition |
|---|---|---|
| C1 | Razor | The order test is split into helpers under 40 lines. The `cli_parser.rs` help text moves to `cli_help.rs` (229 lines). |
| C2 | Restate the R1a bound | Done above and in `IPC_PROTOCOL_SCHEMA.md`. |
| C3 | Fix the R1a test design | Done (see below). |
| C4 | Prove `#[non_exhaustive]` | Done (see below). |
| C5 | Doc corrections | The `server_release.rs` doc gains the writer-stall qualification; `MODEL_RELEASE_V1_PLAN.md` §3.4 gets a supersession note; `CHANGELOG.md` and `RELEASE_SUPPORT_AND_MAINTENANCE.md` are updated; the #112 status lines are corrected. |
| C6 | Owner dispositions untouched | Untouched. |

- **C3, the R1a test:** new `server_release_stall_tests.rs` with a shared `server_release_daemon.rs` (`daemon_with(stall)`).
  - **Setup:** the test drops `holder`, unloads, and reads the unload reply before it stops reading. It uses a 256-byte duplex and a stall of 300 ms. The flood and the cancel come from a spawned task that tolerates write errors.
  - **Assertions:** the connection ends within 3 s and no earlier than the stall; `releasing_models()` is empty; `LIVE_WAITS` returns to 0; only 409 frames reach the client, with no `cancel_response`.
- **C4, `#[non_exhaustive]`:** new module `ipc::protocol_compat`.
  - A passing doctest uses a wildcard match.
  - A `compile_fail,E0004` doctest matches every current variant with no wildcard. Doctests compile as an external crate.
  - **Fuzz crate check:** `core-runtime/fuzz` targets do not reference `IpcMessage`. Benches only construct variants. A scan of `tests/` found no exhaustive match; the compile confirms it.

## Independent review (2026-11-13): APPROVE WITH CONDITIONS, no production defects
- **Verified:** `pending` is cleared and `LIVE_WAITS` is balanced on every path; the order test is deterministic; the extractions are verbatim; the CLI exit codes reach the shell; the doctest's 28 variants match `IpcMessage` exactly; `flood.abort()` is safe; the docs match the source.
- **Fixes:**
  1. The stall test no longer sleeps 300 ms. It waits until request 7 holds `m`, observed as the strong count on the daemon's holder, and drops the holder only after the release wait has started.
  2. An in-crate `#[cfg(test)]` exhaustive match in `protocol_compat` breaks the build when a variant is added. That keeps the `compile_fail,E0004` doctest attributable to `#[non_exhaustive]` rather than to a stale list.
  3. The stall test asserts `elapsed < stall + 1 s`, so one stall and not several.
  4. The stall test asserts 1 to 3 whole 409 frames. With a 256-byte buffer the reader blocked on the fourth, so the cancel was never read. The comment is reworded.
  5. The superseded Revision 2 bullet and the measurement description are corrected.
  6. The stall test is declared from `server_release.rs`, so `server.rs` grows by 4 lines instead of 8. The 3 lines added to `protocol_types.rs` (the attribute and its doc) are accepted.
