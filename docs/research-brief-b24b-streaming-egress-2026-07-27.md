# Research Brief — B-24b: Streaming Egress PII Sanitization

**Date**: 2026-07-27
**Analyst**: The Qor-logic Analyst
**Target**: B-24b — close B-24 F1: the streaming path bypasses egress PII
sanitization. Implement in-runtime detokenization + a streaming-safe windowed
sanitizer so streamed output is redacted like the non-streaming path.
**Scope**: architecture decision + scoping. Phase 1 cycle #3 (unblocked by B-28).

---

## Executive Summary

The building blocks exist: GGUF `detokenize` (`backend.rs:212`, via `token_to_piece`
with a UTF-8 decoder) and `SecurityPipeline::sanitize_output(&str)` (`pipeline.rs:113`).
The missing piece is a **streaming stage** that detokenizes generated tokens into
text, runs the sanitizer over an accumulating buffer, and releases only the portion
that no future token can change — then emits **sanitized text** frames instead of
raw token ids. The correctness crux is the **holdback window**: PII regexes are
`\b`-anchored but some span multiple words (Address, month-name DOB), so releasing to
the last whitespace can leak. Three design forks are put to the operator.

## Findings (verified)

### F1 — the two primitives are present
- `engine/gguf/backend.rs:212` `detokenize(&[LlamaToken]) -> String` — uses
  `token_to_piece` with an `encoding_rs` UTF-8 decoder (handles multi-byte splits).
- `security/pipeline.rs:113` `sanitize_output(&str) -> SanitizedOutput` — the exact
  egress control the non-streaming path uses; reusable on windowed text.

### F2 — the wire already carries text
- `ipc/protocol_types.rs`: `StreamChunk` has `text: Option<String>` +
  `token_with_text` / `final_token_with_text` constructors. Emitting sanitized text
  needs **no** wire-format change (B-24a kept the format; B-24b populates `text`).

### F3 — holdback must be capped, not boundary-only (correctness)
- `security/pii_patterns.rs`: matches are `\b…\b`. Multi-word patterns exist —
  Address `\b\d+\s+[A-Za-z\s]+(?:Street|Ave|…)\b` and month-name DOB
  `\b(?:Jan|…)[a-z]*\s+\d{1,2},?\s+\d{4}\b` — so "release up to the last whitespace"
  would emit `123 Main ` before `Street` arrives and **leak** the street address.
- Therefore the release rule must **hold back a trailing window of ≥ H characters**
  (H ≥ the longest realistic PII span) and re-sanitize the buffer as tokens arrive;
  only the prefix older than H is final. On the terminal frame, sanitize + flush the
  remaining buffer. `[A-Za-z\s]+` is technically unbounded, so any finite H carries a
  documented residual risk (a PII string longer than H, split exactly at the
  boundary); H≈96–128 covers all fixed patterns and realistic addresses.

### F4 — detokenization strategy
- Simplest correct: re-run `detokenize(&token_buffer)` over the accumulated token
  slice each step (encoding_rs handles UTF-8; O(n²) over generation length, fine for
  bounded outputs). Alternative: maintain the `encoding_rs` decoder incrementally
  (O(n), more state). Recommend re-detokenize-buffer for v1 (correctness first).

## Design forks (operator)

1. **Where the sanitizing stage lives**:
   (a) **inside `run_stream_sync`** (the producer owns the GGUF model, so detok is
   local) — emits sanitized-text `StreamItem`s; or (b) a **facade wrapper** around the
   raw token `TokenStream`. *Recommend (a)* — detok needs the model; keeps the raw
   token stream internal and never exposes unsanitized tokens.
2. **What the stream emits**:
   (a) **sanitized text** frames only (raw token ids never leave the runtime — the
   whole point of egress enforcement); or (b) both. *Recommend (a)*: the consumable
   contract becomes "runtime streams sanitized text." (Requires extending
   `StreamItem` with a text-carrying token or a parallel sanitized-text stream — a
   small B-24a-style protocol addition.)
3. **Holdback window H + policy**: fixed cap (recommend **H = 128 chars**) with
   re-sanitize-on-arrival + flush-on-terminal, accepting the documented residual risk
   for pathologically long addresses. Alternative: H = 128 **and** never release
   across an unterminated digit/alnum run (belt-and-suspenders). *Recommend H=128 +
   alnum-run guard.*

## Blueprint Alignment

| Claim | Finding | Status |
|-------|---------|--------|
| Egress PII sanitized on all consumable surfaces | streaming emits raw tokens, unsanitized | DRIFT (B-24 F1 — this cycle) |
| No wire-format change needed | `StreamChunk.text` already exists | MATCH |

## Recommendations

1. Sanitize inside `run_stream_sync` (fork 1a); emit sanitized text only (fork 2a).
2. Windowed sanitizer: accumulate tokens → re-detokenize → `sanitize_output` →
   release prefix older than H=128 chars (+ alnum-run guard) → flush on terminal.
3. Reuse `StreamTerminal` from B-24a; a mid-stream unrecoverable sanitizer state →
   `End(Error)`. (`Rejected` stays for ingress; egress redacts rather than rejects.)
4. Tests: multi-word PII split across token/window boundaries is redacted; a clean
   stream passes through; terminal flush redacts a PII tail; UTF-8 multibyte split is
   not corrupted.

## Risks

- **Latency**: text is delayed by ~H characters of generation. Acceptable for a
  security control; note it.
- **Residual leak**: PII longer than H split exactly at the boundary. Documented;
  mitigated by the alnum-run guard for the common numeric cases.

---

_Research complete. Design forks await operator confirmation (Review Boundary) before
planning; implementation is the windowed sanitizer + protocol addition._
