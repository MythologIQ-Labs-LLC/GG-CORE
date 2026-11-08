# Memory admission: account resident weights once, admit requests by working memory (v1.0.0)

**Risk grade: L2/L3.** This changes resource admission and the default size of the Windows Job Object containment cap. **Gate: `/ql-audit` PASS required before implementation.**
**Base:** `main` (`fd9ee795`). The typed path (#172/#173) gets the same engine change in a follow-up commit on those branches.

## Defect (measured 2026-11-07, issue #161 / #171)

Two separate problems:

1. **Each request is charged the model's whole size.**
   - `worker::acquire_guard` reserves the model's **entire resident weight size** (`model_memory_usage`) for every request.
   - It checks that reservation against `GG_CORE_MAX_MEMORY_PER_CALL`, which defaults to 1 GiB.
   - `InferenceEngine::infer_controlled`, and the typed path, repeat the same weights-vs-per-call comparison.
   - Result: any model over 1 GiB loads fine, then has **every** request refused. Phi-3.5-mini (2.39 GB) is one example.
2. **Resident weights are never accounted, and the total cap is small.**
   - The Job Object cap is `GG_CORE_MAX_TOTAL_MEMORY`, which defaults to 2 GiB.
   - Because resident weights aren't accounted anywhere, a load that won't fit fails inside llama.cpp with an opaque `null result from llama cpp`, not an explicit admission error.

## Design

- **Two kinds of memory, accounted separately:**
  - *Resident* bytes: model weights, charged **once** when the model loads and released when it unloads.
  - *Working* bytes: per-request reservations, charged for the duration of a request.
- **Total bound:** `resident + working + request <= max_total_memory`.
- **Per-request reservation:**
  - The worker reserves `max_memory_per_call` for each request, for generation, streaming and typed alike.
  - That setting now means the operator's worst-case working memory for one request (KV cache plus activations), not the model's weights.
  - The per-call check therefore always passes; the meaningful bound is the total. This is deliberately conservative.
- **No double counting:** the engine no longer compares model weights against the per-call budget. It still passes the budget to the backend configuration.
- **Load admission:**
  - Before any backend I/O, `prepare_model` checks that the model's file size fits in the remaining total headroom. If it doesn't, it fails with an explicit error that names `GG_CORE_MAX_TOTAL_MEMORY`.
  - `ModelLifecycle::load` then reserves the backend-reported `memory_usage()` as resident memory. That reservation is released by `unload`.
  - If the reservation fails, the model is not registered and is dropped.
- **Defaults** (environment variables still override):
  - `GG_CORE_MAX_TOTAL_MEMORY` defaults to the larger of 2 GiB and **50% of physical RAM**.
  - `GG_CORE_MAX_MEMORY_PER_CALL` stays at 1 GiB, now meaning working memory.
  - The Job Object cap still equals the total, so containment remains bounded. It now scales with the host instead of 2 GiB.
  - Physical RAM is read with `GlobalMemoryStatusEx` on Windows (an existing `windows-sys` dependency with one added feature) and `sysconf(_SC_PHYS_PAGES)` on Unix (`libc`, already a dependency).
  - If RAM can't be read, the default stays at 2 GiB.

## Security and containment review

- **No limit removed:**
  - The concurrency cap and the total cap still bound everything.
  - An oversized load is now refused **before** the backend allocates memory.
- **Containment unchanged in kind:** the Job Object cap is still applied, only with a host-proportional default. An operator can still set an explicit lower value.
- **No new external input:** file sizes come from the already path-confined, no-follow `validate_path` / `load_metadata`.

## Tests

- **`limits.rs`:**
  - resident bytes count toward the total;
  - a resident reservation fails when it would exceed the total;
  - a dropped guard releases its memory;
  - the headroom check;
  - a working reservation fails when resident plus working would exceed the total.
- **Worker:**
  - a model larger than `max_memory_per_call` is **not** refused per call. This regression test replaces the current test, which asserts the old behaviour.
  - total exhaustion is still refused.
- **Engine:** generation with a model larger than the budget proceeds. The existing tests that encode the old behaviour are rewritten to the new semantics.
- **Lifecycle:**
  - a load that exceeds the total is refused and the model is not registered;
  - unload releases the resident reservation;
  - a reload succeeds.
- **Config:** the default total is at least 2 GiB, and an environment override wins.
- **Real models:** a sandbox rerun with **default** limits: Phi-3.5 and switching across three GGUF families with the ONNX models resident.

## Out of scope

- Measuring true per-request KV memory from model metadata. That would be a later refinement that tightens the reservation below `max_memory_per_call`.
- GPU memory accounting.

Razor: files at or under 250 lines (`limits.rs` is 243 lines now, so the resident accounting goes in a new `memory/resident.rs`), functions at or under 40 lines, nesting depth 3 or less.

## Addendum A (2026-11-08): concurrent loads, checked accounting, in-flight release

**Risk grade:** L2/L3, the same as above. It strengthens existing admission and adds no new interface or privilege.

### Defects found in the first revision
1. **Concurrent loads could exceed the total.**
   - `prepare_model_admitted` checked headroom (`check_resident_headroom`) without reserving anything.
   - Two authorized loads that each fit alone, but not together, both passed the check and both allocated in the backend.
   - Only one could then register. Meanwhile the process could reach the Windows Job Object cap inside llama.cpp or candle.
2. **Unchecked arithmetic and a fetch-add-then-rollback pattern.**
   - `try_acquire` computed `prev + bytes` unchecked: it panics in debug and wraps in release when `GG_CORE_MAX_MEMORY_PER_CALL` is configured huge.
   - `reserve_resident` and `try_acquire` published the over-limit value before rolling it back. A concurrent reservation that would otherwise fit could be refused spuriously.
3. **Unload released the budget while memory was still allocated.**
   - `unload` dropped the guard immediately.
   - An in-flight request can still hold its `Arc<dyn Model>`, so the weights stay allocated until that request ends.
   - A following load was admitted against bytes that were not free yet.

### Design
- **Checked reservation:** a single primitive, `try_reserve(counter, bytes, limit)`.
  - It is a `fetch_update` compare-and-swap using `checked_add(..) <= limit`.
  - It never wraps and never publishes an over-limit value.
  - Memory reservations and concurrency-slot reservations both use it.
- **Reserve before allocation:**
  - `prepare_model_admitted` reserves the model's file size before backend dispatch.
  - The reservation travels in `PreparedModel`. It is released if the backend fails or the prepared model is dropped, for example when the caller is cancelled. The model field drops first.
  - `ModelLifecycle::load_reserved` resizes the reservation to the backend-reported `memory_usage()`. Shrinking always succeeds. Growing is admitted against the total; if it is refused, the model is dropped first and then the reservation.
  - `load` (no reservation) behaves as before.
- **Release only when memory is freed:**
  - The lifecycle keeps its own `Arc` to the model alongside the guard.
  - On unload, if other references still exist, a task polls every 25 ms until only the lifecycle's reference remains, then drops the model and then the guard. Requests are bounded by their deadlines.
  - Without a Tokio runtime the budget stays charged, which fails closed.
- **Removed:** `check_resident_headroom`, which the reservation replaces.

### Tests
- **Checked overflow:** at `usize::MAX`, for both the resident and working paths.
- **Boundaries:** exactly-at-total and zero bytes.
- **Resize:** grow, refused grow (reservation unchanged), shrink.
- **Concurrency:** 16 threads racing for resident reservations; 12 for working reservations plus slots.
- **Pre-allocation:** a second concurrent load is refused before the backend; 8 parallel prepares never exceed the total.
- **Rollback and abandonment:**
  - A backend failure rolls back its reservation.
  - An aborted task holding a `PreparedModel` releases its reservation.
  - A duplicate id releases the losing reservation.
- **In-flight unload:** release is deferred until the request ends, and a following load is refused until then.

### Residual risks (not changed by this addendum)
- **File size is an estimate, not a bound.**
  - The KV cache and compute buffers allocated per request are covered by the `max_memory_per_call` working reservation. That reservation is not derived from the model's metadata.
  - An operator `GG_CORE_MAX_MEMORY_PER_CALL` below the real per-context need can still make llama.cpp fail at the Job Object cap. The failure is fail-closed (an error is returned).
