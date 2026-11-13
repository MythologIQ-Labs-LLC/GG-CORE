# Model release-wait review remediation: L2 addendum to #183

**Status:** implementation candidate, **not yet governed or qualified**. Independent L2 audit required before integration/merge.
**Source:** draft #183 `cddb8bbddda3853f9aac3684e2f3b4a70858f0c1`.
**Candidate branch:** `fix/model-release-review-remediation-20261112-no-ci`.
**Scope:** only the two verified behavioral issues in the prior independent review. No new model lifecycle authority, native backend, dependencies, network APIs or release.
**Separate owner disposition:** source-compatibility change to public exhaustive `IpcMessage` still unresolved, tracked on #112.

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
5. **External Rust source compatibility:** #183 introduces variants into public exhaustive `IpcMessage`. The existing downstream compile regression does **not** test exhaustively matching that enum. Resolve #112 policy intentionally, with an external-consumer compile receipt, before asserting v1 source compatibility. Do not quietly mark this point as fixed by the behavioral remediation.
6. **Authority:** do not open an overlapping second PR, run paid GitHub Actions, merge protected `main`, sign, tag, publish or change CoreForge pins without explicit authorization. This document is not a claim of PASS, native test execution, or release GO.

**Completion condition:** revised L2 PASS; independent source review; verified real binary tests and integration receipts for one exact immutable head; owner acceptance of any remaining public-contract deviation; then fold the qualified diff into existing #183 rather than creating a competing PR.
