# Research Brief

**Date**: 2026-07-25T13:54:18-04:00
**Analyst**: The Qor-logic Analyst
**Target**: Microsoft Presidio (PII detection) as a comparative reference for
GG-CORE's security egress path, and the offline/Rust-native route to
"Presidio-grade" detection.
**Scope**: Can Presidio be used (linked, sidecar, or reimplemented)? What is
the honest gap vs GG-CORE's current pure-Rust detector, and what closes it
without breaking the offline/sandboxed/no-network charter?
Step 2.5: target is an external library, not a GH issue — pre-check skipped.

---

## Executive Summary

Presidio cannot enter GG-CORE's sandbox — it is Python + spaCy with only an
in-process Python API or a Flask **HTTP** server, and GG-CORE forbids in-process
Python (PyO3 runs the other way) and all network/localhost ports. But
"Presidio-grade" detection is reachable offline by (1) porting Presidio's
model-free *pattern + context-word + validator* design into Rust, and (2)
adding an ONNX NER model behind GG-CORE's existing candle-onnx path. The one
genuine capability gap in GG-CORE's regex-only detector — PERSON / prose
LOCATION / NRP names, which no regex can catch — is a model gap, closable only
with an NER model, and gated on replacing the naive whitespace-hash tokenizer
(`engine/onnx/embedder.rs:120`) with a real subword tokenizer, which ties
directly to issue #72.

## Findings

### GG-CORE baseline (local, authoritative)

- Detector is pure-Rust regex + Luhn + NFKC over 13 types, with per-match
  `confidence: f32` (`pii_patterns.rs:7-33`, `calculate_confidence` :36-75,
  `luhn_check` :78-97, `remove_overlaps` :100-118; `detect()`
  `pii_detector.rs:92-121`). **No NER, no context-word scoring, no
  international IDs beyond the US-centric set.**
- ONNX path exists but uses a naive hash tokenizer
  (`engine/onnx/embedder.rs:120` `simple_tokenize`) — unusable for a real
  transformer model; `candle_onnx::simple_eval` at :75 is CPU-only, which
  matches GG-CORE's offline profile.

### Presidio architecture & method (external, cited)

- **presidio-analyzer** is the only comparable component. It combines: regex
  `PatternRecognizer`s, a `LemmaContextAwareEnhancer` (context words in a ±5
  token window add `context_similarity_factor = 0.35`, floor
  `min_score_with_context_similarity = 0.4`, cap 1.0), NER via an `NlpEngine`
  (spaCy `en_core_web_lg` ≈ 588 MB default), and checksum `validate_result()`
  (Luhn/IBAN/national-ID → score 1.0). MIT license, ~10.2k stars.
- Coverage GG-CORE lacks entirely: **PERSON, LOCATION, NRP have no regex path
  — NER-only.** Plus dozens of international IDs (IBAN, CRYPTO, national IDs
  for ~20 countries) that are regex+checksum-tractable.
- Independent 2026 benchmark: Presidio avg F1 ≈ 0.481 across mixed datasets
  (email 0.96+, CoNLL names 0.78 via NER, messy sets ~0.30), ~15 ms/sample —
  NER is the cost center. Presidio optimizes for **recall over precision (F2)**:
  redaction should over-catch rather than leak.

### Deployment / integration reality

- Presidio exposes only (a) in-process Python import, (b) Flask **HTTP/REST**
  on TCP :5002. **No C ABI, no gRPC, no pipe/socket-native transport.**
- GG-CORE's only sandbox-legal channel is authenticated named-pipe/Unix-socket
  IPC; HTTP/REST/localhost ports are forbidden (CLAUDE.md). A Presidio sidecar
  would therefore require a forbidden HTTP surface **and** vendoring a
  Python+spaCy runtime past the "no in-process Python" rule.

### Offline Rust-native path

- **ONNX NER via candle-onnx (recommended):** `dslim/distilbert-NER`
  (Apache-2.0, 66M params, CoNLL F1 0.9217, PER/LOC/ORG/MISC) has a
  pre-converted ONNX export; DistilBERT ops are supported in candle-onnx.
  Requires the `tokenizers` crate (v0.23.1, Apache-2.0, `http` off by
  default → offline) loading a vendored `tokenizer.json` for WordPiece +
  offset mapping. **Avoid Piiranha** (mDeBERTa-v3): superior PII coverage but
  `cc-by-nc-nd-4.0` (non-commercial, no-derivatives — ONNX conversion is a
  barred derivative) and uncertain candle-onnx operator support.
- **rust-bert / redact-ner / ort: rejected** — all pull libtorch or the `ort`
  onnxruntime C++ runtime and/or download weights at runtime; both violate the
  no-network / minimal-native-surface posture. Stay on candle-onnx.
- **`pii-vault` v0.2.0 (MIT, 2026-04):** the closest existing Rust artifact —
  regex + **context-aware scoring**, 40+ types incl. IBAN/crypto, 15 nations,
  zero network. Young/thin but a viable dependency or pattern donor
  (MIT-compatible with GG-CORE Apache-2.0).
- **Eval:** span-level precision/recall/F1 per entity type is the
  Presidio-standard metric (`presidio-research`). Offline corpora vendorable
  into an air-gapped test set: `ai4privacy/pii-masking-openpii-1m` (CC-BY-4.0),
  presidio-research synthetic generator (generate once online → vendor static
  JSON), CoNLL-2003 (check terms). This is the concrete way to turn "we redact
  PII" into a tracked number for issue #52.

## Blueprint Alignment

| Blueprint / prior claim | Actual finding | Status |
|---|---|---|
| ARCHITECTURE_PLAN: `security/` does "PII redaction" | True but regex-only; misses NER-class PII (names, prose locations) — a structural gap, not tuning | DRIFT (capability overstated) |
| CLAUDE.md: no network, IPC named-pipe/socket only | Confirms Presidio sidecar (HTTP-only) is charter-illegal | MATCH (rules out sidecar) |
| CLAUDE.md: PyO3 = Python-calls-Rust | Confirms in-process Presidio (Rust-calls-Python) inverts the model — incompatible | MATCH (rules out in-process) |
| Issue #72: ONNX path needs real implementation + tokenizer | The NER route depends on exactly that tokenizer work — the two efforts converge | MATCH (dependency) |
| Ledger #99: egress PII redaction "enhanced security" | Honest bound: regex-grade; NER-class PII passes today. Should be measured, not asserted | DRIFT (self-correction) |

## Recommendations

1. **P1 — Build the offline eval harness first (issue #52 thread).** Vendor a
   static labeled corpus (ai4privacy openpii-1m CC-BY-4.0 + presidio-research
   synthetic JSON generated once then air-gapped); pure-Rust span-level
   precision/recall/F1-per-type over the existing `PIIDetector`. Measure the
   gap before closing it; gives every later change a regression baseline. No
   new runtime deps.
2. **P2 — Port Presidio's context-word scoring + international patterns in pure
   Rust (no model).** Add per-pattern `context_words`, a ±N-word window
   enhancer (`+0.35`, floor `0.4`, cap `1.0`), IBAN mod-97 validator (sibling
   to Luhn), crypto/extra phone/postal patterns. Evaluate `pii-vault` as
   dependency or pattern donor. Raises precision on the loose numeric patterns
   immediately; Section-4-Razor clean.
3. **P2/P3 — Add an ONNX NER model behind candle-onnx (closes the real gap;
   couples to #72).** Replace `simple_tokenize` with the `tokenizers` crate +
   vendored `tokenizer.json`; run `dslim/distilbert-NER` ONNX through
   `simple_eval`; map token labels back to char spans for redaction. This is
   the only sandbox-legal route to PERSON/LOCATION coverage. Gate on the eval
   harness proving the accuracy lift justifies the model weight + latency.
4. **Do NOT** link Presidio in-process, sidecar it over HTTP, or adopt
   rust-bert/ort/tch. Treat Presidio as a design reference and an accuracy
   benchmark, not a component.

## Updated Knowledge

- Shadow Genome Entry #6 added: regex-only PII redaction has a permanent
  NER-class blind spot (names, prose locations); "enhanced security" claims
  for the egress path must be qualified as regex-grade until measured against
  a labeled corpus.
- New reference facts: Presidio is HTTP/Python-only (no C/gRPC) → sidecar
  charter-illegal; `pii-vault` (MIT) is the closest Rust artifact;
  `dslim/distilbert-NER` (Apache-2.0) is the license-clean ONNX NER candidate;
  Piiranha is license-blocked (cc-by-nc-nd).

---

_Research complete. Findings are advisory — implementation decisions remain
with the Governor._
