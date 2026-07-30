# Plan: B-36 — Incremental Streaming Egress Sanitize (O(n²) → O(n))

**change_class**: feature

**doc_tier**: standard

**terms_introduced**: []

**boundaries**:
- limitations:
  - Optimizes `StreamSanitizer::{push, flush}` to sanitize only the bounded unreleased raw
    tail instead of the whole accumulated buffer, taking the per-stream cost from O(n²) to
    O(n). Observable output is unchanged — proven by a differential test against a preserved
    whole-buffer reference implementation.
- non_goals:
  - No change to `SecurityPipeline`, `OutputSanitizer`, the PII patterns, or `release_cut`'s
    cut-placement logic; no change to `HOLDBACK`; no change to the documented B-24b residual.
  - No API change: `push`/`flush` keep their signatures and `pub(crate)` visibility.
- exclusions:
  - No caller change (the streaming worker keeps calling `push(full_text)` / `flush(full_text)`).

## Open Questions

None. The bounded-tail design and its correctness gate (differential vs a whole-buffer
reference) were resolved in research (F3/F4). The enabling invariant — the settled prefix is
byte-stable across pushes — is the module's own documented property (`stream_sanitizer.rs:13-14`).

## Design Rationale (Simple Made Easy)

The current `push` re-sanitizes the entire buffer every token; B-35 proved sanitize is linear,
so the stream is O(n²). **The release/cut decision MUST stay on SANITIZED text** — deciding on
raw text splits internal-separator PII (credit-card `4111 1111 1111 1111`, address) at a space
and leaks it (empirically proven, ledger #162, `SG-StreamSanitizeRawCut`). So the optimization
keeps the OLD semantics verbatim — `release_cut` on the sanitized string, an `emitted`
sanitized-offset cursor — and only removes the *redundant work*: it caches the sanitized STABLE
PREFIX (`stable_san`, the sanitize of `full_text[0..stable_raw]` at a match-free boundary) and
per push reconstructs `san = stable_san + sanitize(&full_text[stable_raw..])`, re-sanitizing only
the bounded tail. Because `stable_raw` is a match-free boundary, this reconstruction is
byte-identical to `sanitize(full_text)`, so every existing behavior test passes unchanged and no
new PII residual is introduced. Rebasing `stable_raw` forward (when the tail exceeds a bound)
keeps the tail O(1), making the stream O(n). A boundary is committed only when a windowed
split-vs-joint sanitize check proves no match straddles it.

## Phase 1: Cached-stable-prefix reconstruction in `push`/`flush`

### Affected Files

- `core-runtime/src/security/stream_sanitizer.rs` — keep `emitted` (sanitized-offset cursor) and
  the `release_cut`-on-sanitized logic; add a cached stable prefix and bounded-tail
  reconstruction:
  - New fields `stable_raw: usize` (raw bytes whose sanitize is cached; a match-free boundary) and
    `stable_san: String` (== `sanitize(full_text[0..stable_raw]).output`). `new`/`with_holdback`
    init them to `0` / empty.
  - A private `sanitized(&self, full_text) -> String` = `stable_san.clone()` +
    `sanitize(&full_text[stable_raw..]).output`. `push`/`flush` call this instead of
    `sanitize(full_text)`; the rest of their bodies (release_cut, `emitted` slice, return) are
    UNCHANGED — so output is byte-identical to today.
  - A private `maybe_rebase(&mut self, full_text)`: when `full_text.len() - stable_raw >
    REBASE_THRESHOLD` (e.g. `4 * HOLDBACK`), pick a candidate raw boundary `p` (char-boundary,
    ≈ `full_text.len() - 2*HOLDBACK`), and commit the rebase only if `splits_cleanly(full_text, p)`;
    otherwise scan `p` backward to the nearest char boundary that splits cleanly (or skip rebasing
    this push). On commit: `stable_san.push_str(&sanitize(&full_text[stable_raw..p]).output);
    stable_raw = p;`. Called at the end of `push`.
  - A private `splits_cleanly(&self, full_text, p) -> bool`: with a bounded window `[a,b]` around
    `p` (`a = p.saturating_sub(HOLDBACK)`, `b = min(len, p + HOLDBACK)`, both nudged to char
    boundaries), return `sanitize(full[a..p]).output + sanitize(full[p..b]).output ==
    sanitize(full[a..b]).output` — true ⟺ no PII match straddles `p` within the window (matches
    beyond the window cannot reach `p`). Token-format-independent.
  - Update the module doc-comment: the tail is re-sanitized against a cached match-free stable
    prefix (not the whole buffer); output is byte-identical to the whole-buffer sanitize.

### Changes

Add the stable-prefix cache + rebase; `push`/`flush` route through `sanitized(...)` and are
otherwise unchanged. `release_cut` unchanged. Output byte-identical to the pre-B-36 code by
construction (match-free rebase ⇒ reconstruction == whole-buffer sanitize). New non-test code
≈ 60 lines; the differential tests live in the sibling file (Phase 2), keeping both files < 250.

### Unit Tests

All four existing in-file tests MUST pass **unchanged** — including
`multi_word_pii_split_across_pushes_is_redacted` at its original `holdback=8` — because the
cached-stable-prefix reconstruction is byte-identical to the whole-buffer sanitize. This
byte-identity is the crux: unlike the abandoned raw-cut design, nothing about observable behavior
changes, so `flush_redacts_pii_tail`, `multi_word_pii_split_across_pushes_is_redacted`,
`utf8_multibyte_is_not_corrupted`, and `clean_stream_passes_through` remain the regression floor
with no edits.

## Phase 2: Differential (whole-buffer + one-shot) + CC-leak safety test

Because the cached-stable-prefix reconstruction is byte-identical to the whole-buffer sanitize,
the strongest oracle IS available: **the release stream must equal both a whole-buffer streaming
reference AND the one-shot `sanitize_output(complete_text)`**, at every holdback. The CC-leak
case (which the abandoned raw-cut design failed) is the load-bearing safety test.

### Affected Files

- `docs/FEATURE_INDEX.md` — add row **F-59** registering the streaming egress PII sanitizer
  (`core-runtime/src/security/stream_sanitizer.rs`), which shipped in B-24b but was never
  individually indexed (the index jumps F-58 → this is the first row for it). Test path:
  `core-runtime/src/security/stream_sanitizer_diff_tests.rs` + the in-file `tests` module.
- `core-runtime/src/security/stream_sanitizer_diff_tests.rs` (NEW) — attached to the module via
  `#[cfg(test)] #[path = "stream_sanitizer_diff_tests.rs"] mod diff_tests;` in
  `stream_sanitizer.rs` (keeps the main file under Razor). All tests are transitively
  `#[cfg(feature = "gguf")]` (the module's gate).
  - `stream_cases()`: a deterministic generator of adversarial complete-texts (no `rand` dep) —
    clean filler interleaved with PII tokens (email, phone `555-123-4567`, SSN `123-45-6789`,
    credit-card `4111 1111 1111 1111`), including PII at start/end/middle and repeated PII, each
    driven token-by-token (growing prefixes) then flushed.
  - `WholeBufferRef`: a small self-contained copy of the pre-B-36 whole-buffer logic (sanitize
    the full buffer each push, `release_cut` on the sanitized string, `emitted` cursor) — the
    behavioral oracle. `PII_TOKENS`: the injected raw tokens for the safety assertion.

### Unit Tests

- `matches_whole_buffer_reference_byte_for_byte` — for each stream at both the default `HOLDBACK`
  and a small holdback (8), drive BOTH the new `StreamSanitizer` and `WholeBufferRef`; assert the
  concatenated releases are **byte-identical**. Binding behavior-preservation gate (the whole
  point of the cached-prefix design is zero observable change).
- `terminal_equals_one_shot_at_production_holdback` — at the default `HOLDBACK`, assert the
  stream+flush concatenation equals `pipeline.sanitize_output(complete_text).output`.
- `never_emits_raw_pii_including_space_separated` — for each stream (incl. the credit-card case
  that the abandoned raw-cut design leaked), at both holdbacks, assert no released chunk contains
  any injected raw PII token. The non-negotiable safety property; the CC case is the regression
  guard for ledger #162.

## Feature Inventory Touches

- `entry_id`: `F-59` (streaming egress PII sanitizer — shipped B-24b, never indexed; this cycle
  registers it) — `operation`: `NEW` (first FEATURE_INDEX row for `stream_sanitizer.rs`) —
  `test_path`: `core-runtime/src/security/stream_sanitizer_diff_tests.rs` + the in-file `tests`
  module. B-36 modifies the feature's internal algorithm (whole-buffer → bounded-tail) while
  preserving observable behavior; the index row + differential test make it verifiable.

## Definition of Done

### Deliverable: O(n) streaming egress sanitize, behavior-preserving

- **D1**: The streaming egress sanitizer no longer re-sanitizes the whole buffer per token; the
  per-stream cost is linear, and the observable release stream is unchanged (byte-identical to
  the pre-B-36 whole-buffer reference), never emitting raw PII.
- **D2**: `stream_sanitizer.rs` keeps `release_cut`-on-sanitized + the `emitted` cursor; adds
  `stable_raw`/`stable_san` cache, a `sanitized()` reconstruction, `maybe_rebase`, and
  `splits_cleanly`; `push`/`flush` signatures + `pub(crate)` visibility unchanged; new
  `stream_sanitizer_diff_tests.rs` attached via `#[path]`.
- **D3**: META_LEDGER entries (canonical markup) research #158, gate #159/#160/#161, finding
  #162 (done), re-audit, seal; BACKLOG B-36 → done; CHANGELOG `[Unreleased]` note; FEATURE_INDEX
  row F-59 added.
- **D4**: `cargo test -p gg-core --features gguf security::stream_sanitizer` — the 4 existing
  behavior tests + the 3 new tests all pass: `matches_whole_buffer_reference_byte_for_byte`
  (byte-identical behavior-preservation), `terminal_equals_one_shot_at_production_holdback`, and
  `never_emits_raw_pii_including_space_separated` (safety — the credit-card regression guard for
  ledger #162). Locally verifiable on the Windows dev host WITH `--features gguf` (the module is
  gguf-gated); CI-confirmed by the `features / gguf` leg.

## CI Commands

- `cargo test -p gg-core --features gguf security::stream_sanitizer` — behavior + differential + CC-safety tests pass
- `cargo fmt --check` — formatting
- `cargo clippy -p gg-core --features gguf -- -D warnings` — lint (gguf feature, where the module compiles)
