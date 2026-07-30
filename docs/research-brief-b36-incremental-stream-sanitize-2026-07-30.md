# Research Brief — B-36: Incremental Streaming Egress Sanitize

**Date**: 2026-07-30
**Analyst**: The Qor-logic Analyst
**Target**: B-36 — remove the O(n²) re-sanitize in `StreamSanitizer` (each `push`
re-sanitizes the entire accumulated buffer; B-35 proved per-call `sanitize_output` is
linear). Third cycle of the optimization initiative; profile-gated by B-35 (now armed).
**Scope**: `core-runtime/src/security/stream_sanitizer.rs` (178 lines) — the algorithm, the
byte-stability invariant that enables incrementalization, and the equivalence guard.

---

## Executive Summary

`StreamSanitizer::push(full_text)` calls `pipeline.sanitize_output(full_text)` on the whole
buffer every token (`stream_sanitizer.rs:43`). With B-35's confirmed linear per-call sanitize
(~53 ns/byte), N pushes over an N-byte stream cost O(N²). The module already documents the
property that removes the redundancy: *"the settled prefix (before the cut) contains only
completed matches, so it is byte-stable across pushes"* (`:13-14`). Therefore each push need
only sanitize a **bounded raw tail** — `full_text[raw_released..]` — not the whole buffer,
making the stream O(N). The equivalence to the whole-buffer sanitize rests on the SAME
assumption the current code already relies on (`HOLDBACK` exceeds the longest PII match;
documented residual B-24b, ledger #121), so the safe way to ship it is with a **differential
test** asserting the incremental output equals the full-resanitize output across many
randomized streams. Recommendation: implement the bounded-tail design + the differential
equivalence test as the binding acceptance gate. Single cycle.

## Findings (verified)

### F1 — the O(n²) mechanism
- `push` (`:42-51`): `let sanitized = self.pipeline.sanitize_output(full_text).output;` — the
  argument is the FULL detokenized text so far, re-scanned every token. `flush` (`:54`) does
  the same once. B-35 (`security_overhead` bench, ledger #157): `sanitize_output` is
  ~53 ns/byte, **linear per call**. N tokens × O(N) per push = **O(N²)** over the stream —
  the superlinear case B-35 was gating.

### F2 — the byte-stability invariant is the enabler (already asserted)
- Module doc (`:10-14`): a PII match of length ≤ `HOLDBACK` ending after the cut started
  within `HOLDBACK` of the end, so it lies entirely in the withheld tail; the settled prefix
  before the cut therefore contains only completed matches and is **byte-stable across
  pushes**. `release_cut` (`:68`) places the cut `HOLDBACK` chars behind the end, backed up so
  it never splits an alphanumeric run (`:74`). Because the released prefix never changes,
  re-sanitizing it every push is pure waste — only the unreleased tail can still change.

### F3 — the bounded-tail design (O(N), same emitted bytes)
- Replace the sanitized-offset cursor `emitted` (`:27`, an index into the sanitized string)
  with a **raw-offset cursor** `raw_released` (bytes of `full_text` whose sanitized image is
  released). Each push: `let cut = release_cut(full_text, holdback)` applied to the **raw**
  buffer (it already takes a `&str` + returns a UTF-8-safe byte offset); if `cut <=
  raw_released` return `None`; else emit `sanitize_output(&full_text[raw_released..cut]).output`
  and set `raw_released = cut`. `flush`: emit `sanitize_output(&full_text[raw_released..])`.
  Per-push work is now O(`cut - raw_released` + `HOLDBACK`) = O(Δtokens + const) → **O(N)**.
- The slice boundaries `raw_released` and `cut` sit at non-alphanumeric positions
  (`release_cut`'s backup), so the regexes' `\b` anchors behave at the slice edges exactly as
  in the full buffer; a match fully inside a slice redacts identically.

### F4 — equivalence rests on the existing HOLDBACK assumption → GUARD WITH A DIFFERENTIAL TEST
- Sanitizing independent slices equals sanitizing the whole buffer **iff no PII match
  straddles a slice boundary**. That is exactly the current code's own guarantee (a match
  crossing `cut` would exceed `HOLDBACK` — the documented B-24b residual). So the bounded-tail
  design is equivalent to the same degree the current code is correct — no NEW residual is
  introduced, but the equivalence is subtle (redaction changes lengths; a partial match can be
  emitted then its completion redacted separately — behavior the whole-buffer code also
  exhibits). **The binding safety requirement**: a differential test that drives many
  randomized token streams (clean text interleaved with emails/phones/SSNs/credit-cards, split
  at adversarial offsets incl. mid-match and at the `HOLDBACK` boundary) through BOTH a
  reference whole-buffer sanitizer and the incremental one, asserting the concatenated releases
  are byte-identical AND never contain a raw PII token. This converts the equivalence claim
  into an executable gate.

### F5 — invariants to preserve (all currently tested)
- UTF-8 safety (`utf8_multibyte_is_not_corrupted`, `:146`): `release_cut` slices at
  `char_indices` boundaries; the raw cursor must too (it does — `cut` is a char-boundary byte
  offset). Multi-word split (`multi_word_pii_split_across_pushes_is_redacted`, `:123`): the
  full email appears only in push 2 and must never be emitted raw — preserved because the
  unreleased tail is always re-sanitized. Clean passthrough (`:165`) + flush-tail (`:113`):
  preserved. The three existing tests are the regression floor; the differential test (F4) is
  the new proof.

## Blueprint Alignment

| Optimization-brief expectation | Finding | Status |
|---|---|---|
| Incrementalize streaming sanitizer if B-35 confirms superlinear | B-35 confirmed O(n²) (F1); bounded-tail = O(N) (F3) | MATCH |
| Preserve the B-24b multi-word-PII-split guarantee | Same HOLDBACK assumption; no new residual (F4) | MATCH |
| Non-negotiable: never emit raw PII | Unreleased tail always re-sanitized; guarded by differential test (F4/F5) | MATCH |

## Recommendations

1. **B-36 deliverable**: rewrite `push`/`flush` to sanitize the bounded raw tail
   `full_text[raw_released..cut]` (cursor becomes raw-offset `raw_released`); apply
   `release_cut` to the raw buffer. Single self-contained file (`stream_sanitizer.rs`), no API
   change (`push`/`flush` signatures + `pub(crate)` visibility unchanged), no caller churn.
2. **Binding acceptance gate**: add a differential/property test (F4) asserting incremental ==
   whole-buffer output over randomized adversarial streams, plus a `security_stream_overhead`
   bench (or extend `security_overhead`) demonstrating the per-stream cost is now sub-quadratic.
   Keep the three existing behavior tests.
3. **Locally verifiable**: `stream_sanitizer.rs` is default-feature, buildable + testable on
   the Windows dev host (`cargo test stream_sanitizer`); CI-confirmed by the `test` job.

## Updated Knowledge (Shadow Genome)

**Optimize behind an asserted invariant, prove it with a differential.** The module already
*claimed* byte-stability of the settled prefix (`:13-14`); B-36 turns that claim into the
optimization's foundation. When an optimization's correctness rests on a subtle,
already-documented assumption (HOLDBACK > max match), the safe delivery is a differential test
against the un-optimized reference — not a hand argument — because the reference and the
optimized path must be byte-identical to the same degree the reference is correct.

---

_Research complete. B-36 = bounded-tail sanitize (O(n²)→O(n)) guarded by a differential
equivalence test. The differential test (F4) is the load-bearing safety requirement — no
new PII residual may be introduced._
