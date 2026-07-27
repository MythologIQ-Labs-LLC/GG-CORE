# Plan — B-28: Real Subword Tokenizer (ONNX path)

**Date**: 2026-07-27
**Author**: Strategist
**Session ID**: 2026-07-27T-b28-tokenizer
**Risk Grade**: L2 (adds an offline dependency; no crypto/network)
**Source**: B-28 research brief (ledger Entry #119) + operator decisions

## Operator decisions (baked in)

- **Vocab path**: sibling convention — `tokenizer.json` in the same directory as the
  `.onnx` model file.
- **Absent-tokenizer policy**: graceful named fallback — `HashFallbackTokenizer` with
  a `tracing::warn!` + telemetry when no `tokenizer.json` is found (non-breaking).

## Objective

Replace the hash stub `simple_tokenize` with a real WordPiece tokenizer for the ONNX
embed/classify paths, loaded offline from a local `tokenizer.json`, without breaking
existing behavior when no tokenizer is present.

## Design

New `engine/onnx/tokenizer.rs`:

```rust
pub(super) enum OnnxTokenizer {
    WordPiece(Box<tokenizers::Tokenizer>), // real, from tokenizer.json
    HashFallback,                          // degraded, explicitly named
}

impl OnnxTokenizer {
    /// Resolve a tokenizer for a model file: load a sibling `tokenizer.json`
    /// (offline, `from_file`); on miss/parse-error, warn + telemetry + fall back.
    pub(super) fn for_model(model_path: &std::path::Path) -> Self { ... }

    pub(super) fn encode(&self, text: &str) -> Vec<i64> { ... }
}
```

- `WordPiece::encode`: `tok.encode(text, true)?.get_ids()` → `Vec<i64>`; special
  tokens ([CLS]/[SEP]) come from the tokenizer's post-processor. On encode error,
  log + return the fallback encoding (never panic on the inference path).
- `HashFallback::encode`: the current `simple_tokenize` logic, moved verbatim and
  honestly named.

## Dependency

`Cargo.toml`: `tokenizers = { version = "0.21", default-features = false, optional = true }`;
add `"tokenizers"` to the `onnx` feature. `http` stays off (offline; verified F2).

## Scope (exact files)

1. `core-runtime/Cargo.toml` — add `tokenizers` dep + `onnx` feature member.
2. `core-runtime/src/engine/onnx/tokenizer.rs` — NEW `OnnxTokenizer` (< 250 lines).
3. `core-runtime/src/engine/onnx/mod.rs` — declare `mod tokenizer`; loaders resolve
   `OnnxTokenizer::for_model(path)` and pass it into `with_model`.
4. `core-runtime/src/engine/onnx/embedder.rs` — add `tokenizer` field; `embed_text_onnx`
   calls `self.tokenizer.encode(text)`; move the hash body into `OnnxTokenizer`; keep a
   thin `simple_tokenize` shim only if still referenced by tests, else remove.
5. `core-runtime/src/engine/onnx/classifier.rs` — add `tokenizer` field; `classify`
   calls `self.tokenizer.encode(text)`.
6. `core-runtime/src/telemetry/*` — a counter for tokenizer-fallback (reuse existing
   metric surface if present; else a minimal `record_tokenizer_fallback`).
7. Tests: `tokenizer.rs` unit tests — HashFallback determinism + `[CLS]/[SEP]`; a
   WordPiece round-trip built via the `tokenizers` API into a `tempfile`, proving the
   offline `from_file` path and real vocab ids; `for_model` on a dir without
   `tokenizer.json` returns HashFallback.
8. **B-32**: `core-runtime/src/cli/mod.rs` — serialize the two `GG_CORE_SOCKET_PATH`
   env tests behind a shared `Mutex` (a module-level `static ENV_LOCK`), removing the
   parallel-runner race.

## Definition of Done

- [ ] `simple_tokenize` no longer called from the inference path; hash logic lives in
      `OnnxTokenizer::HashFallback`.
- [ ] `for_model` loads a sibling `tokenizer.json` when present; warns + falls back
      when absent (no panic, no silent-wrong).
- [ ] `cargo build --features onnx` + `clippy --all-targets --features onnx -- -D warnings`
      clean; default + gguf + ffi + python legs still clean.
- [ ] `cargo test` green incl. the new tokenizer tests; the two `cli` env tests no
      longer race (B-32).
- [ ] `cargo fmt --check` clean; Razor: `tokenizer.rs` < 250 lines, fns < 40.
- [ ] Offline preserved: no `http`/`hf-hub` in the tree (`tokenizers` `default-features
      = false`); grep the lock for `hyper`/`reqwest` introduced by tokenizers → none.

## Non-goals

- Manifest-driven tokenizer selection (B-29).
- Streaming egress sanitization (B-24b) — though B-28 unblocks its faithful detok.
- Changing the ONNX model loading or `simple_eval` path.

## Verification commands

```
cargo build -p gg-core --features onnx
cargo clippy -p gg-core --all-targets --features onnx -- -D warnings
cargo clippy -p gg-core --all-targets --features gguf,ffi,python -- -D warnings
cargo test -p gg-core --features onnx
cargo test -p gg-core   # default (incl. cli env tests / B-32)
cargo fmt --check
```

## Rollback

Single-branch revert; new dependency is optional + offline; no persisted state.
