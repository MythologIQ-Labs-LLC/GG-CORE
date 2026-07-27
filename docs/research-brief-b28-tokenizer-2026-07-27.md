# Research Brief — B-28: Real Subword Tokenizer for the ONNX Path

**Date**: 2026-07-27
**Analyst**: The Qor-logic Analyst
**Target**: B-28 — replace `simple_tokenize` with a real WordPiece/subword tokenizer
for the ONNX classify/embed paths, honoring GG-CORE's offline constraint.
**Scope**: architecture decision + scoping. Phase 1 cycle #2.

---

## Executive Summary

The ONNX path tokenizes with a **hash-based fake** (`embedder.rs:120`): each
whitespace word is hashed into an arbitrary id in `[1000, 30000)`, unrelated to any
real vocabulary. The ONNX model therefore receives meaningless `input_ids` and its
embeddings/classifications are noise. The fix is the HuggingFace `tokenizers` crate
loading a real `tokenizer.json` via `Tokenizer::from_file` — which is **fully offline**
(the `http`/Hub feature is off by default). Two design sub-decisions remain (vocab
path convention; absent-tokenizer fallback policy) and are put to the operator.

## Findings (verified)

### F1 — `simple_tokenize` is a hash stub, not a tokenizer
- `engine/onnx/embedder.rs:120-130`: `ids = [101]; for word in split_whitespace {
  push(hash(word) % 29000 + 1000) }; push(102)`. Ids are hash buckets, not vocab
  ids; `[CLS]=101`/`[SEP]=102` are hard-coded BERT specials.
- Consumers: `embedder.rs:72` (`embed_text_onnx`) and `classifier.rs:77`
  (`classify` via `super::embedder::simple_tokenize`). Both feed
  `build_transformer_inputs` → `candle_onnx::simple_eval`.
- **Consequence**: outputs are deterministic but semantically meaningless — the
  ONNX backend "works" (no error) while producing garbage (a silent-wrong class,
  worse than fail-loud).

### F2 — `tokenizers` crate satisfies the offline constraint (verified)
- Authoritative: *"The HTTP feature, which is **disabled by default**, enables
  downloading tokenizers via HTTP and makes `Tokenizer::from_pretrained`
  accessible."* (huggingface/tokenizers README, Features > http).
- Therefore `Tokenizer::from_file("tokenizer.json")` loads purely from local disk
  with **no** network path compiled in unless `http` is explicitly enabled.
  `from_pretrained` (the only Hub path) does not even exist without `http`.
- API: `Tokenizer::from_file(path)? ; let enc = tok.encode(text, true)? ;
  enc.get_ids() -> &[u32]` (special tokens added by the tokenizer's post-processor,
  so the hard-coded 101/102 go away).
- Offline + pure-Rust config: `tokenizers = { version = "0.21",
  default-features = false, optional = true }` — drops `onig` (C) and `esaxx_fast`
  (C++) and never pulls `http`/`hf-hub`. BERT WordPiece needs no regex engine
  (BertPreTokenizer is whitespace+punctuation), so `default-features = false` is
  sufficient.

### F3 — the loader has a natural seam for the tokenizer path
- `engine/onnx/mod.rs:71,100`: `load_onnx_model` / `load_onnx_classifier` take
  `path: &Path` (the `.onnx` file) + a currently-**unused** `_config: &OnnxConfig`.
  The tokenizer path can arrive either by **convention** (a `tokenizer.json` sibling
  of the model file) or via an `OnnxConfig.tokenizer_path` field. No new loader
  argument is strictly required.

### F4 — B-32 flaky test folds into this cycle
- `cli/mod.rs:57-73`: `test_get_socket_path_default` (`remove_var`) and
  `..._from_env` (`set_var`) race on the shared process env under Rust's parallel
  in-process test runner (observed failing once on the macOS CI leg for PR #82).
  One-line class of fix: serialize the two via a shared `Mutex`/`serial_test`.

## The Decision (offline dependency) — RESOLVED by evidence

Adopt `tokenizers` with `default-features = false` (no `http`, no C deps), loaded
via `Tokenizer::from_file`. This is offline by construction (canonical constraint
satisfied; F2). No `from_pretrained` capability is compiled in.

## Open sub-decisions (operator)

1. **Vocab path**: (a) **convention** — `tokenizer.json` sibling of the `.onnx`
   file (matches HF layout; zero API change); (b) explicit `OnnxConfig.tokenizer_path`.
   *Recommend (a)* for B-28, with (b)/manifest deferred to B-29.
2. **Absent-tokenizer policy**: (a) **fail-loud** — ONNX inference requires a real
   `tokenizer.json`, delete the hash stub; (b) **graceful fallback** — keep the hash
   path as an explicitly-named degraded `HashFallbackTokenizer` with a logged
   warning + telemetry when no `tokenizer.json` is found. *Recommend (b)* (non-
   breaking; existing tests keep working; honest naming replaces the "simple"
   euphemism) — but (a) is more aligned with C.O.R.E. fail-loud if you prefer.

## Recommended scope for B-28 (once decided)

- Add `tokenizers` (offline config) to the `onnx` feature.
- Introduce an `OnnxTokenizer` seam: `WordPieceTokenizer` (from `tokenizer.json`) +
  the renamed fallback; embedder/classifier call it instead of `simple_tokenize`.
- Loader resolves the tokenizer by the chosen convention.
- Tests: a tiny committed `tokenizer.json` fixture → assert real vocab ids +
  `[CLS]/[SEP]` from the tokenizer, and round-trip on a known string.
- Fold **B-32**: serialize the two `cli` env tests.

## Blueprint Alignment

| Claim | Finding | Status |
|-------|---------|--------|
| ONNX path tokenizes correctly | hash stub → garbage ids | DRIFT (the B-28 defect) |
| Offline / no-network preserved | `tokenizers` `http` off by default; `from_file` local-only | MATCH (with `default-features=false`) |

## Recommendations

1. Adopt the offline `tokenizers` config (decided).
2. Operator picks sub-decisions 1 (path) + 2 (fallback); recommend convention + graceful fallback.
3. Implement per the scope above, folding in B-32.

---

_Research complete. Offline feasibility verified; the two policy sub-decisions await
operator confirmation (Review Boundary) before planning._
