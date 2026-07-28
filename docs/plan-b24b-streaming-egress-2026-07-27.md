# Plan — B-24b: Streaming Egress PII Sanitization

**Date**: 2026-07-27
**Author**: Strategist
**Session ID**: 2026-07-27T-b24b
**Risk Grade**: L3 (egress security path; a bug is a PII leak)
**Source**: B-24b research brief (ledger Entry #121) + operator decisions

## Operator decisions (baked in)

1. Sanitize **inside `run_stream_sync`** (producer owns the GGUF detokenizer; raw
   token ids never leave the runtime).
2. Stream emits **sanitized text only**.
3. Holdback **H = 128 chars + alnum-run guard**; re-sanitize on arrival; flush on terminal.

## Objective

Close B-24 F1: streamed output must be egress-PII-sanitized like the non-streaming
path, with the runtime detokenizing and redacting before any text reaches a client.

## Design

### Protocol addition (small, B-24a-style)
Extend the streamed frame to carry sanitized text:

```rust
pub enum StreamItem {
    Token(u32),        // retained for internal/non-sanitized callers/tests
    Text(String),      // NEW: sanitized text chunk (client-facing)
    End(StreamTerminal),
}
```

`run_stream_sync` emits `Text` (never `Token`) when a `SecurityPipeline` is present;
`relay_stream` maps `Text` → `StreamChunk::token_with_text`/`final_token_with_text`
(text populated, no raw token id).

### Windowed sanitizer (new `security/stream_sanitizer.rs`)

```rust
pub(crate) struct StreamSanitizer<'a> {
    pipeline: &'a SecurityPipeline,
    holdback: usize,        // H = 128
    released: usize,        // chars already emitted
    buffer: String,         // full detokenized text so far
}
impl StreamSanitizer<'_> {
    /// Push newly-detokenized full text; return any newly-releasable *sanitized*
    /// prefix (text older than H chars and not mid-alphanumeric-run).
    fn push(&mut self, full_text: &str) -> Option<String>;
    /// Terminal: sanitize + return the entire remaining tail.
    fn flush(&mut self) -> Option<String>;
}
```

- Release rule: candidate cut = `buffer.len() - H`; back the cut up to the nearest
  char boundary that is **not** inside an alphanumeric run (avoid splitting a number
  mid-match). Sanitize the whole buffer with `sanitize_output`; emit the sanitized
  substring between `released` and the cut; advance `released`.
- Correctness: any PII fully within `[0, cut)` is complete (cut is ≥ H behind the
  growing end and not mid-run), so its redaction is final. Residual risk: PII longer
  than H split at the cut — documented.

### Detokenization
`run_stream_sync` accumulates `Vec<LlamaToken>`; each step re-runs the generator's
`detokenize(&buffer)` (encoding_rs handles multi-byte UTF-8), feeds the full text to
`StreamSanitizer::push`. (v1 re-detokenizes the buffer; O(n²) over bounded output.)

## Scope (exact files)

1. `engine/streaming.rs` — add `StreamItem::Text(String)`; sender `text()`.
2. `security/stream_sanitizer.rs` — NEW `StreamSanitizer` (< 250 lines).
3. `security/mod.rs` — export it.
4. `engine/inference.rs` `run_stream_sync` — thread an optional `&SecurityPipeline`;
   accumulate tokens, detokenize, drive `StreamSanitizer`, emit `Text` frames + flush
   on terminal. (Signature gains the pipeline; callers updated.)
5. `runtime_facade.rs` `infer_stream` — pass `self.security` into `run_stream_sync`.
6. `scheduler/worker_streaming.rs` — pass the pipeline through the streaming worker.
7. `ipc/handler.rs relay_stream` — map `StreamItem::Text` → `StreamChunk` text ctor.
8. Tests: `stream_sanitizer.rs` unit tests (multi-word PII split across pushes
   redacted; clean passthrough; terminal-flush redaction; alnum-run guard; UTF-8
   multibyte split intact) + a streaming integration test asserting a PII prompt's
   streamed output is redacted end-to-end.

## Definition of Done

- [ ] Streamed output through `infer_stream` (with security enabled) is PII-redacted;
      raw token ids never emitted on that path.
- [ ] Multi-word PII (address, month DOB) split across token/window boundaries is
      redacted (adversarial tests prove it).
- [ ] `cargo build/clippy --all-targets --all-features -- -D warnings` clean;
      `cargo test` green incl. new tests; `fmt --check` clean.
- [ ] Razor: `stream_sanitizer.rs` < 250 lines, fns < 40.
- [ ] Latency note (text delayed ~H chars) documented; residual-risk documented.

## Non-goals

- FFI/Python per-token streaming (still full-output; separate follow-up).
- ONNX streaming (ONNX path is non-streaming).
- Changing the PII pattern set.

## Verification commands

```
cargo build -p gg-core --all-features
cargo clippy -p gg-core --all-targets --all-features -- -D warnings
cargo test -p gg-core --features gguf
cargo test -p gg-core
cargo fmt --check
```

## Rollback

Single-branch revert; internal streaming protocol; no persisted state/wire-schema change.
