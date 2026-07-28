# AUDIT REPORT — B-24b: Streaming Egress PII Sanitization

**Session ID**: 2026-07-27T-b24b
**Auditor**: Judge (independent pass)
**Target**: docs/plan-b24b-streaming-egress-2026-07-27.md
**Risk Grade**: L3 (egress security path)
**Verdict**: **PASS**

---

## Checks

### 1. Threat closure — the plan actually closes F1 — PASS
Sanitizing inside `run_stream_sync` and emitting sanitized `Text` only means raw
token ids never leave the runtime on the secured path; a client cannot reconstruct
unsanitized output. This is the correct enforcement point (the producer owns the
detokenizer).

### 2. Holdback correctness argument — PASS (with test mandate)
The release rule (cut = len − H, backed off a char boundary not inside an
alphanumeric run, whole-buffer re-sanitize) makes any PII fully within `[0, cut)`
final, because the cut trails the growing end by ≥ H and never bisects a numeric run.
The residual risk (PII longer than H split at the cut) is explicitly documented.
**Mandate**: IMPLEMENT must include adversarial tests that split a multi-word address
and a month-name DOB across `push` calls and across the H boundary, and assert
redaction — not just a happy-path test.

### 3. Signature ripple — PASS (flagged)
`run_stream_sync` gains `&SecurityPipeline`; the ripple reaches `infer_stream`
(facade), `worker_streaming::run_stream`, and any test caller. IMPLEMENT must update
all call sites; the "no worker/no pipeline" case (Python binding path) must pass
`None`/skip sanitization coherently (it already routes through `infer`, not
`infer_stream`, so likely unaffected — verify).

### 4. UTF-8 safety — PASS
Re-detokenizing the whole token buffer via `encoding_rs` (as `detokenize` already
does) yields valid UTF-8; releasing only on `char_indices` boundaries prevents
mid-codepoint cuts. Test mandate: a multibyte (e.g. emoji / accented) split.

### 5. Razor + constitutional — PASS
New `stream_sanitizer.rs` is a focused file (< 250). No network, no new forbidden
dep (reuses `SecurityPipeline` + regex already present). `StreamItem::Text` is a
minimal protocol addition consistent with B-24a.

### 6. Terminal semantics — PASS
Flush-on-terminal sanitizes the tail; `End(Error)` for an unrecoverable sanitizer
state; `Rejected` stays ingress-only (egress redacts rather than rejects). Consistent
with B-24a's terminal.

## Verdict

**PASS.** Proceed to IMPLEMENT. Carry the three test mandates (multi-word PII across
boundary; UTF-8 multibyte split; terminal-flush redaction) — a happy-path-only test
suite is insufficient for an L3 redaction control.
