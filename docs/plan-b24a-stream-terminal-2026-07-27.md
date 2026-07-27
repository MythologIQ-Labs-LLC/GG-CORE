# Plan — B-24a: Typed Stream Terminal

**Date**: 2026-07-27
**Author**: Strategist
**Session ID**: 2026-07-27T-b24-streaming
**Risk Grade**: L2 (streaming protocol; no crypto, no network)
**Source**: B-24 decision brief (ledger Entry #117)

---

## Objective

Replace the implicit stream terminal (`is_final` bool / sender-drop, with errors
faked as `send(0, true)`) with an explicit typed terminal so a client can tell
**completion** from **mid-stream rejection** from **engine error**. Fixes B-24 F2.
No detokenization or sanitization here (that is B-24b, after B-28).

## Problem (verified)

- `engine/streaming.rs:7` `StreamingOutput { token: u32, is_final: bool }` — terminal
  is only `is_final` or channel-close.
- `scheduler/worker_streaming.rs:105` `send_error` sends `(0, true)` — an **error is
  indistinguishable from a real final token**. This is the core defect.
- `ipc/handler.rs:426 relay_stream` can only emit `StreamChunk::error` on
  cancellation; a mid-stream engine error arrives as a normal `final_token`.

## Design

New channel item (replaces `StreamingOutput`):

```rust
pub enum StreamItem {
    Token(u32),
    End(StreamTerminal),
}

#[derive(Debug, Clone)]
pub enum StreamTerminal {
    Complete,
    Rejected(String), // reserved for B-24b egress-abort; wired end-to-end now
    Error(String),
}
```

`TokenStreamSender`: replace `send(token, is_final)` with
`token(u32)` + `end(StreamTerminal)`. A well-formed stream is a run of `Token`
frames terminated by exactly one `End`. Dropping the sender without `End` is treated
by the receiver as `End(Error("stream dropped"))` (defensive).

## Scope (exact files)

1. `engine/streaming.rs` — new `StreamItem` / `StreamTerminal`; `token()`/`end()`
   sender methods; `next() -> Option<StreamItem>`; `collect()` returns
   `(Vec<u32>, StreamTerminal)` and stops on `End`.
2. `engine/gguf/backend.rs:116` — per token `sender.token(id)`; loop end
   `sender.end(Complete)`; on generation error `sender.end(Error(..))`.
3. `engine/gguf/generator.rs:97` — thread the same (wrapper).
4. `engine/inference.rs:236` — `run_stream_sync`: ensure a terminal is always emitted
   (map an early lookup/downcast failure to `end(Error(..))` before returning).
5. `scheduler/worker_streaming.rs:26,68,104` — `send_error` → `end(Error(..))`;
   success path → `end(Complete)`.
6. `ipc/handler.rs:441` `relay_stream` — match `StreamItem`: `Token`→`StreamChunk::token`,
   `End(Complete)`→`final_token`(sentinel/empty), `End(Rejected|Error)`→`StreamChunk::error`.
7. `ffi/*streaming*` — map terminal onto the FFI callback contract.
8. `python/streaming.rs` — map terminal onto `StreamingResult.error`/`is_final`
   (fields already exist).
9. Tests: `engine/streaming` unit tests (new terminal cases), `scheduler/worker_tests.rs`,
   `worker_security_tests.rs`, `queue_tests.rs` — update to `token()`/`end()`.

## Definition of Done

- [ ] No `send(0, true)`-style error faking remains; grep clean.
- [ ] `relay_stream` emits `StreamChunk::error` for `End(Error|Rejected)` and a clean
      final marker for `End(Complete)`.
- [ ] `cargo build --all-features` + `clippy --all-targets --all-features -- -D warnings`
      clean (default + gguf/onnx/ffi/python).
- [ ] `cargo test` green; new tests assert completion vs error terminals are distinct.
- [ ] `cargo fmt --check` clean.
- [ ] Razor: `streaming.rs` stays < 250 lines; every touched fn < 40.

## Non-goals / Boundaries

- No detokenization, no egress sanitization (B-24b, gated on B-28).
- No new IPC wire message types — reuse existing `StreamChunk::{token,final_token,error}`.
- `Rejected` variant is defined + wired through the terminal now but only *produced*
  by B-24b's egress-abort path; B-24a produces `Complete`/`Error`.

## Verification commands (CI parity)

```
cargo build -p gg-core --all-features
cargo clippy -p gg-core --all-targets --all-features -- -D warnings
cargo test -p gg-core
cargo fmt --check
```

## Rollback

Single-branch revert; streaming protocol is internal (no persisted state, no wire
schema change).
