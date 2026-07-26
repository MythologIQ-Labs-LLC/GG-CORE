# AUDIT REPORT — Gate Tribunal (B-25b FFI/Python reroute)

**Target**: docs/plan-b25b-ffi-python-reroute-2026-07-26.md (iteration 1)
**Date**: 2026-07-26
**Session**: 2026-07-26T1850-b25b
**Risk Grade**: L3
**Mode**: adversarial (independent fresh-context Judge subagent)

## VERDICT: PASS

Every Locked Decision grep-verified against the real tree. Reroute is
infrastructure-accurate and security-sound.

- `Runtime::infer` scans BEFORE the engine (runtime_facade.rs:66,73-76) → an
  injection prompt returns SecurityRejected with no model loaded (LD-4 valid).
- `CoreRuntime { inner: Arc<Runtime>, tokio }` (ffi/runtime.rs:18-21) → the
  `rt.tokio.block_on(rt.inner.infer(..))` reroute is reachable.
- Error mappings ALREADY exist and are reused, no new lines: ffi/error.rs:127-141
  (ModelNotLoaded→ModelNotFound, SecurityRejected→SecurityRejected, exhaustive);
  python/exceptions.rs:48-52 (all variants → gg_core.InferenceError).
- Ignored acceptance test present (ffi_test.rs:476), asserts ModelNotFound;
  passes after reroute (infer returns ModelNotLoaded without a worker).
- Default SecurityConfig blocks injection (security/mod.rs:52-63,73-79).
- Streaming stays single-callback full-output (non-regressive; real per-token =
  B-24). Razor: all touched files net-shrink, stay ≤250. No bypass; leak-safe
  constant rejection; fail-closed. create_runtime_and_session spawns no worker,
  so the un-ignored test won't hang.

### Advisories (baked into implementation)
1. Injection test MUST use a real high-risk phrase (e.g. "ignore all previous
   instructions", score ≥ BLOCK_RISK_THRESHOLD 50) — not an innocuous string.
2. FFI error match arm MUST become `Err(code) => code` (return the mapped
   CoreErrorCode), not the old `Err(e) => InferenceFailed`; `From` already fires
   `set_last_error` — do not re-call.
3. `core_infer_bounded` MUST retain its `BufferTooSmall` buffer-copy Ok-path;
   swap only the block_on body.

### Next action
`/qor-implement` authorized (Phases 1-3). Commit locally; push + PR at operator
direction.
