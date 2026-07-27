# AUDIT REPORT — B-28: Real Subword Tokenizer (ONNX path)

**Session ID**: 2026-07-27T-b28-tokenizer
**Auditor**: Judge (independent pass)
**Target**: docs/plan-b28-tokenizer-2026-07-27.md
**Risk Grade**: L2
**Verdict**: **PASS**

---

## Checks

### 1. Offline / forbidden-deps constraint (constitutional, highest risk) — PASS (empirical)
Added `tokenizers = { version = "0.21", default-features = false, optional = true }`
and inspected the resolved tree: `cargo tree --features onnx` pulls `tokenizers
v0.21.4` and **no** network/TLS crate — grep for `reqwest|hyper|hf-hub|ureq|native-tls|
rustls` over the onnx tree returns NONE. `http` (the only Hub path, gated behind that
feature) is not enabled, so `from_pretrained` is not even compiled. `from_file` is
local-only. C.O.R.E. offline boundary preserved with evidence.

### 2. Defect is real — CONFIRMED
`simple_tokenize` (`embedder.rs:120`) emits hash-bucket ids; both embed and classify
paths consume it. Replacing it with real vocab ids is correct and necessary.

### 3. Design soundness — PASS
`OnnxTokenizer` enum (WordPiece | HashFallback) with `for_model(path)` sibling-convention
resolution + graceful warn-and-fallback matches the operator decisions. `tokenizers::
Tokenizer` is `Send + Sync`, so it is safe inside `Arc<dyn OnnxModel>`. Inference path
never panics (encode error → fallback encoding, logged).

### 4. Non-breaking — PASS
Absent `tokenizer.json` → `HashFallback` (prior behavior, honestly named + logged), so
existing ONNX tests/usages keep working; no fail-loud regression.

### 5. Razor + scope — PASS
New `tokenizer.rs` is a fresh small file (< 250). Loaders gain one resolution call.
B-32 (serialize the two `cli` env tests) is a bounded, in-scope hygiene fold-in.

### 6. Test adequacy — PASS (with mandate)
Plan requires: HashFallback determinism; a WordPiece round-trip built via the
`tokenizers` API into a tempfile (proves offline `from_file` + real vocab ids);
`for_model` miss → HashFallback. IMPLEMENT must actually add these (not just assert
compilation).

## Verdict

**PASS.** Proceed to IMPLEMENT. Carry two mandates: (a) after wiring, re-confirm the
lock has no network crate from tokenizers; (b) the WordPiece test must exercise a real
`from_file` load, not only the fallback.
