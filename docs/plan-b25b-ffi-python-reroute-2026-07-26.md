# Plan: B-25b — FFI/Python inference reroute through the secure façade

**change_class**: feature
**doc_tier**: standard
**iteration**: 1
**risk_grade**: L3 (changes inference behavior + security enforcement on the
consumable FFI/Python surfaces; public binding API)
**high_risk_target**: false
**originating_research**: docs/research-brief-b25-ci-legs-ffi-python-2026-07-26.md
(Entry #104) + the B-25b implementation contract gathered this session.

**terms_introduced**: none new.

**boundaries**:
- limitations:
  - **Real per-token streaming is out of scope.** `core_infer_streaming` carries
    a `text: *const c_char` callback and currently delivers the full output in a
    single callback; the reroute preserves that (one callback with the full
    `Runtime::infer` output) — it fixes the deadlock and adds security
    enforcement, but true token streaming needs detokenization (B-24) and stays
    deferred.
  - No new CI legs (the ffi/python legs exist and are green from B-25).
- non_goals:
  - No change to `Runtime::infer`/`infer_stream` (shipped in the façade cycle).
  - No worker spawned in FFI/Python init (the façade calls the engine directly;
    that is the design).
  - No `security/`, `engine/`, or scheduler changes.
- exclusions:
  - `ffi/error.rs` and `python/exceptions.rs` mappings already handle
    SecurityRejected (façade cycle) — not re-touched.

## Open Questions

None blocking. Contract verified against the current tree: `Runtime::infer`
(runtime_facade.rs:60-81), `CoreRuntime { inner: Arc<Runtime>, tokio }`
(ffi/runtime.rs:18-21), the five deadlocking bodies, and the existing error
mappings (ffi/error.rs:127-141; python/exceptions.rs:47-52).

## Design summary

Each FFI/Python inference entry point currently enqueues to `request_queue` and
awaits a worker that binding-init never spawns → **deadlock**. Replace that
boilerplate with a direct call to the shipped, security-enforcing
`Runtime::infer` façade. This fixes the deadlock AND routes every consumable
call through the same ingress-scan → engine → egress-sanitize pipeline the
embedded surface uses. Removing the enqueue boilerplate net-shrinks the files.

## Locked Decisions

- **LD-1 — Reroute to `Runtime::infer`, drop the queue/await.** In each of
  `core_infer` (ffi/inference.rs:72-88), `core_infer_bounded`
  (ffi/inference.rs:212-227), `core_infer_streaming` (ffi/streaming.rs:125-141),
  `Session::infer` (python/session.rs:79-97), `AsyncSession::infer`
  (python/session.rs:198-222): replace `enqueue_with_response(..).await` +
  `rx.await` with `rt.inner.infer(model, prompt, &params).await` (FFI, inside the
  existing `rt.tokio.block_on`) / `self.runtime.infer(..).await` (Python). The
  bounded variant keeps its `BufferTooSmall` copy path; streaming keeps its
  single `invoker.invoke(&output, true, None)` callback.
- **LD-2 — Error mapping is already in place; reuse it.** FFI:
  `From<engine::inference::InferenceError> for CoreErrorCode` maps
  `ModelNotLoaded → ModelNotFound` and `SecurityRejected → SecurityRejected`
  (ffi/error.rs:132,138). Python: `From<RuntimeInferenceError> for PyErr`
  surfaces all variants (incl. SecurityRejected) as `gg_core.InferenceError`
  (python/exceptions.rs:47-52) — so the reroute uses `.map_err(PyErr::from)` /
  `?`, adding no per-variant match and no lines.
- **LD-3 — Un-ignore the acceptance test.** Remove `#[ignore]` from
  `tests/ffi_test.rs::test_infer_on_unloaded_model_returns_error`; after reroute,
  `core_infer` → `Runtime::infer` → `engine.run` → `ModelNotLoaded` →
  `CoreErrorCode::ModelNotFound` (no worker needed). The test's existing
  assertion `code == ModelNotFound` now passes.
- **LD-4 — Security enforcement is now testable on the consumable surface.**
  `Runtime::infer` scans BEFORE the engine, so an injection prompt returns
  `SecurityRejected` even with no model loaded (default `SecurityConfig` blocks).
  Add an FFI test asserting `core_infer(.., INJECTION_PROMPT, ..) ==
  CoreErrorCode::SecurityRejected` — proving the consumable bindings are now
  security-enforced (the whole point of the cycle).

## Phase 1: FFI reroute

### Affected Files

- `core-runtime/tests/ffi_test.rs` — un-ignore the acceptance test (LD-3); add
  `test_infer_rejects_injection_prompt` asserting `CoreErrorCode::SecurityRejected`
  (LD-4); a smoke test that `core_infer_bounded` returns (no hang)
- `core-runtime/src/ffi/inference.rs` — reroute `core_infer` + `core_infer_bounded`
  (net −~12 lines; stays ≤250)
- `core-runtime/src/ffi/streaming.rs` — reroute `core_infer_streaming` (single
  callback with full output)

### Changes

`core_infer` block body becomes:
```rust
let result = rt.tokio.block_on(async {
    rt.inner
        .infer(model_str, prompt_str, &rust_params)
        .await
        .map_err(CoreErrorCode::from)  // sets last_error + maps variant
});
```
(Use the `From` impl so `SecurityRejected`/`ModelNotLoaded` map correctly and
`set_last_error` fires.) `core_infer_bounded` mirrors this and keeps the
buffer-copy + `BufferTooSmall` path. `core_infer_streaming` calls `infer`, then
`invoker.invoke(&r.output, true, None)` on success / the error callback on error.

### Unit / Integration Tests

- `tests/ffi_test.rs::test_infer_on_unloaded_model_returns_error` (un-ignored) —
  `core_infer` with a real runtime + unloaded model returns
  `CoreErrorCode::ModelNotFound` (no deadlock). B-25b's headline acceptance.
- `tests/ffi_test.rs::test_infer_rejects_injection_prompt` — `core_infer` with an
  injection prompt returns `CoreErrorCode::SecurityRejected` (security enforced
  on the FFI surface; proves the reroute goes through the pipeline).
- `tests/ffi_test.rs::test_infer_bounded_returns_without_hang` — `core_infer_bounded`
  on an unloaded model returns `ModelNotFound` (no deadlock).

## Phase 2: Python reroute

### Affected Files

- `core-runtime/src/python/session.rs` — reroute `Session::infer` +
  `AsyncSession::infer` to `runtime.infer(..)`, error via `PyErr::from`
  (net-neutral/shrink; stays ≤250)
- `core-runtime/tests/python_binding_test.rs` — add a `#[cfg(feature="python")]`
  test that `Session::infer` on an injection prompt raises the security error
  (where the harness allows constructing a Session; else assert at the Rust
  boundary)

### Changes

`Session::infer` block: `self.runtime.infer(model_id, prompt, &rust_params).await
.map_err(PyErr::from)`. `AsyncSession::infer`: same inside the
`future_into_py` async move, after the existing auth `validate`.

### Tests

- Python injection-rejection test (feature-gated) OR a Rust-level assertion that
  the error maps to the InferenceError exception, per what the binding-test
  harness supports.

## Phase 3: Governance

### Affected Files

- `docs/FEATURE_INDEX.md` — F-39 (FFI) / F-40 (python): note inference is now
  rerouted through the secure façade (deadlock fixed, security-enforced)
- `docs/BACKLOG.md` — mark B-25b done; note real-token FFI streaming remains a
  follow-up (ties to B-24)
- `docs/ARCHITECTURE_PLAN.md` — data-flow: the consumable FFI/Python surfaces now
  enforce the SecurityPipeline via `Runtime::infer` (both delivery surfaces
  unified on one enforced entry point)

## Feature Inventory Touches

| entry_id | operation | test_path | test_descriptor |
|---|---|---|---|
| F-39 | MODIFIED | core-runtime/tests/ffi_test.rs | core_infer no longer deadlocks (returns ModelNotFound on unloaded model) and returns SecurityRejected on an injection prompt — proves FFI inference is rerouted through the secure façade |
| F-40 | MODIFIED | core-runtime/tests/python_binding_test.rs | Python Session::infer routes through Runtime::infer; injection surfaces the security error |

## Definition of Done

### Deliverable: FFI/Python secured inference (deadlock fixed)
- **D1**: Every consumable inference entry point runs through `Runtime::infer` —
  no deadlock, security-enforced; one entry point serves both delivery surfaces.
- **D2**: 5 entry points rerouted; ffi/error + python/exceptions mappings reused;
  ignored acceptance test un-ignored.
- **D3**: FEATURE_INDEX F-39/F-40 updated; BACKLOG B-25b done; ARCHITECTURE_PLAN
  data-flow note.
- **D4**: `test_infer_on_unloaded_model_returns_error` (un-ignored) +
  `test_infer_rejects_injection_prompt` + `test_infer_bounded_returns_without_hang`
  pass under the ffi feature test leg; the python injection test passes under the
  python feature; the ffi feature test suite completes with no hang.

## CI Commands

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings` (default)
- `cargo test --workspace` (default)
- Feature legs (existing CI matrix + local): clippy `--all-targets -- -D warnings`
  and the test suite under each of the ffi and python features.
