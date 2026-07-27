# Research Brief — B-24: Streaming Egress Sanitization & IPC Terminal Protocol

**Date**: 2026-07-27
**Analyst**: The Qor-logic Analyst
**Target**: B-24 — decide detokenize-in-runtime vs client-side contract for streaming
egress PII sanitization; resolve the indistinguishable rejection/completion terminal.
**Scope**: architecture decision + scoping. Phase 1 cycle #1 of the backlog sweep.

---

## Executive Summary

Both defects B-24 names are confirmed in code. Streaming (`infer_stream`) scans
ingress but explicitly leaves **egress token sanitization out of scope**
(`runtime_facade.rs:87`), and the channel carries raw `u32` token IDs
(`engine/streaming.rs:8`) — so the streaming surface is a **PII-sanitization
bypass** relative to the enforced non-streaming path (`pipeline.rs:113`
`sanitize_output(&str)`). Separately, the only stream terminal is `is_final`/sender-
drop, with **no** way to distinguish normal completion from mid-stream security
rejection or engine error. The security boundary forces the direction
(**detokenize-in-runtime**), but full remediation is a multi-cycle effort with a
soft dependency on B-28 (real tokenizer). Recommendation: **split B-24** and
**resequence B-28 earlier**.

## Findings (verified)

### F1 — Streaming egress is a sanitization bypass (security, high)
- `runtime_facade.rs:85-116` `infer_stream`: ingress `scan_prompt` runs; on allow,
  `run_stream_sync` streams tokens directly to the caller. Doc-comment line 87:
  *"Egress token sanitization is out of scope."*
- `engine/streaming.rs:7-10` `StreamingOutput { token: u32, is_final: bool }` — the
  frame is a **token id**, never text.
- `pipeline.rs:113` `sanitize_output(&self, output: &str)` — the egress PII control
  operates on **text**. It cannot see a `u32` stream.
- **Consequence**: a model can emit PII (SSN, email, key material) over the streaming
  surface and it is never redacted, while the same output via `infer` would be. This
  contradicts the C.O.R.E. security boundary (egress sanitization is a production
  control, wired in B-25b) for every consumable streaming caller (FFI
  `core_infer_streaming`, Python `AsyncSession`).

### F2 — Rejection/error is indistinguishable from completion (protocol, medium)
- `engine/streaming.rs:49` `send(token, is_final)`; `:57` `close(self)` drops the
  sender. The terminal is either `is_final: true` or channel-close.
- There is **no variant** for "rejected mid-stream" or "engine error". A security
  abort or a backend failure both present to the client as a silent end — a client
  cannot tell a complete answer from a truncated/aborted one. This is both a
  correctness bug and a security-signalling gap (a redaction-triggered abort looks
  like success).

### F3 — Detokenization quality couples to B-28
- Streaming-safe egress sanitization requires the runtime to **detokenize** the id
  stream into text and run the sanitizer over an accumulated/windowed buffer (PII
  spans multiple sub-word tokens, so per-token sanitization is impossible). Faithful
  detokenization needs the real subword tokenizer tracked as **B-28**; the current
  naive `simple_tokenize` path would mis-render text and could both leak (missed
  match) and over-redact. Doing B-24's detokenization half well wants B-28 first.

## The Decision

**Detokenize-in-runtime — ADOPTED. Client-side-contract — REJECTED.**

| Option | Egress PII control | Verdict |
|--------|--------------------|---------|
| Detokenize-in-runtime: runtime buffers detokenized text, sanitizes over a sliding window with holdback, streams sanitized **text** chunks + typed terminal | Enforced on the streaming surface, same guarantee as `infer` | **ADOPT** — required by the security boundary |
| Client-side contract: runtime streams raw `u32`, client detokenizes | Impossible — runtime never forms text, cannot sanitize | **REJECT** — makes streaming a permanent sanitization bypass; violates C.O.R.E. |

Two canonical signals control this (non-mundane decision, resolved by artifact):
(1) the SecurityPipeline egress sanitizer is a production control on the consumable
surface (B-25b); (2) CLAUDE.md's security boundary requires egress redaction. A
consumable surface that cannot enforce it is out of contract.

## Recommended Re-scope & Resequence

B-24 as written is a multi-cycle epic. Split it:

- **B-24a — Stream terminal protocol** (bounded, no tokenizer dependency, do first):
  replace the implicit terminal with a typed terminal frame
  (`Complete | Rejected{reason} | Error{msg}`) carried across `TokenStream`, the IPC
  streaming handler, FFI `core_infer_streaming`, and Python streaming. Fixes F2.
  Risk L2.
- **B-28 — Real subword tokenizer** (resequence *before* B-24b): prerequisite for
  faithful detokenization. Risk L2.
- **B-24b — Streaming egress sanitization** (after B-28): in-runtime detokenization +
  streaming-safe windowed sanitizer with holdback; streams sanitized text. Fixes F1.
  Risk L3.

**Proposed Phase 1 order (revised):**
`B-24a → B-28 → B-24b → B-29 → B-07 → B-16`.

## Blueprint Alignment

| Claim | Finding | Status |
|-------|---------|--------|
| Egress PII sanitization enforced on consumable surfaces | streaming surface bypasses it | DRIFT (this is the B-24 defect) |
| Streaming honors the same security contract as `infer` | ingress yes, egress no; terminal unsignalled | DRIFT |

## Recommendations

1. Adopt detokenize-in-runtime (above).
2. Approve the split (B-24a/B-24b) and the resequence (B-28 before B-24b).
3. Start implementation at **B-24a** (bounded, unblocks a correct client contract).

---

_Research complete. The decision and re-scope are advisory; execution awaits operator
confirmation of the split/resequence (Review Boundary)._
