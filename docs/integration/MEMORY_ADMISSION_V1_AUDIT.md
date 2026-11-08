# AUDIT REPORT

**Tribunal Date:** 2026-11-07
**Target:** memory admission, `docs/integration/MEMORY_ADMISSION_V1_PLAN.md` (sha256 `80e2fb1c71b3253422f01a690f035f94dbcd07066d74b68e3a823053ab3d18ea`)
**Risk Grade:** L2/L3
**Auditor:** The QoreLogic Judge
**VERDICT:** PASS

## Passes
- **Security: PASS.**
  - No limit is removed. Concurrency and total caps still bound every request.
  - Oversized loads are refused before backend I/O, with an explicit error.
  - The Job Object cap is still applied (host-proportional default, operator override).
  - Removing the engine-level weights-vs-budget check does not weaken in-process callers: FFI and Python already pass no budget (`run_cancellable` gives `None`), so only the worker path used it, and the worker keeps its reservation.
- **Ghost UI: PASS (not applicable).**
- **Razor: PASS.** `limits.rs` is at 243 lines, so resident accounting goes in `memory/resident.rs` and host RAM detection in `memory/host.rs`.
- **Dependencies: PASS.** No new crate; one `windows-sys` feature is added (`Win32_System_SystemInformation`) for `GlobalMemoryStatusEx`.
- **Orphans: PASS.** Every new module is wired through `memory/mod.rs`.
- **Macro architecture: PASS.** One shared memory counter covers both resident and working reservations, so there is no race between two separate atomics.

## Ledger
Deferred to merge (parallel v1 branches).

---

# AUDIT REPORT: Addendum A (concurrent loads, checked accounting, in-flight release)

**Tribunal Date:** 2026-11-08
**Target:** `MEMORY_ADMISSION_V1_PLAN.md` Addendum A (plan sha256 `5ff01128fea31ff6903549dbd889ec66626a7b2cb5f6b5595686651c8388e16b`)
**Risk Grade:** L2/L3
**Auditor:** The QoreLogic Judge
**VERDICT:** PASS

**Process note:** the code for this addendum was drafted while the defect was being investigated, before this tribunal sat. It was gated before any push: nothing in this addendum left the local branch until this verdict was recorded.

## Passes
- **Security: PASS.**
  - No limit is removed or relaxed. A load now reserves before backend allocation, which is strictly tighter than the earlier non-reserving check.
  - Arithmetic is checked, so the counter cannot wrap.
  - Every refusal path is fail-closed:
    - a refused registration drops the model before releasing its budget;
    - without a runtime, the budget stays charged.
  - The Job Object cap is unchanged.
  - No new input, IPC surface or privilege.
- **Ghost UI: PASS** (not applicable).
- **Razor: PASS.**
  - `limits.rs` is 248 lines, `lifecycle.rs` 225, `model_ops.rs` 225.
  - New sidecars: `lifecycle_resident.rs`, `resident_tests.rs`, `lifecycle_reservation_tests.rs`.
  - No function exceeds 40 lines; nesting is at most 3.
- **Dependencies: PASS.** None added.
- **Orphans: PASS.** Every new module is wired through `#[path]` from its parent.
- **Macro architecture: PASS.**
  - A single reservation primitive serves all three counters.
  - The reservation's ownership is explicit: it is carried by `PreparedModel`, then by `Resident`.
