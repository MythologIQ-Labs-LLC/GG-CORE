//! Streaming-safe egress PII sanitizer.
//!
//! Wraps [`SecurityPipeline::sanitize_output`] for token-by-token generation.
//! Each `push` feeds the full detokenized text so far. The release decision is made
//! on the SANITIZED buffer (PII already collapsed to `[REDACTED:…]` tokens): the cut
//! sits at least `HOLDBACK` characters behind the growing end — far enough that no
//! in-flight PII match can still extend into it — and only the newly-settled prefix
//! is released. `flush` releases everything remaining on the terminal frame.
//!
//! Cost (B-36): the full buffer is NOT re-sanitized every token. Instead a cached
//! sanitized STABLE PREFIX (`stable_san` = the sanitize of `full_text[0..stable_raw]`
//! at a match-free boundary) is reused, and each push sanitizes only the bounded tail
//! `full_text[stable_raw..]`; `stable_raw` is rebased forward (once the tail exceeds a
//! bound) at boundaries proven to split no PII match. Because the split point is
//! match-free, `stable_san + sanitize(tail)` is byte-identical to sanitizing the whole
//! buffer, so behavior is unchanged while an N-byte stream is O(N) not O(N²) (per-call
//! sanitize is linear, B-35 ledger #157). The release decision must stay on sanitized
//! text: deciding on raw text would split an internal-separator match (e.g. a credit
//! card) and leak it (ledger #162, `SG-StreamSanitizeRawCut`).
//!
//! Correctness: a PII match of length ≤ `HOLDBACK` ending after the cut started within
//! `HOLDBACK` of the end, so it lies entirely in the withheld tail and is never emitted
//! raw. Documented residual: a PII string longer than `HOLDBACK` split exactly at the
//! window boundary (see B-24b, ledger Entry #121).

use super::SecurityPipeline;

/// Trailing sanitized characters withheld until settled.
const HOLDBACK: usize = 128;
/// Rebase the cached prefix once the re-sanitized tail grows past this, keeping
/// per-push work bounded.
const REBASE_THRESHOLD: usize = 4 * HOLDBACK;

pub struct StreamSanitizer<'a> {
    pipeline: &'a SecurityPipeline,
    holdback: usize,
    /// Bytes of the (byte-stable) settled prefix already released.
    emitted: usize,
    /// Raw bytes whose sanitize is cached in `stable_san`; always a match-free
    /// boundary, so `stable_san + sanitize(rest)` == `sanitize(whole)`.
    stable_raw: usize,
    /// Cached `sanitize_output(full_text[0..stable_raw]).output`.
    stable_san: String,
}

impl<'a> StreamSanitizer<'a> {
    pub(crate) fn new(pipeline: &'a SecurityPipeline) -> Self {
        Self {
            pipeline,
            holdback: HOLDBACK,
            emitted: 0,
            stable_raw: 0,
            stable_san: String::new(),
        }
    }

    /// Sanitize `full_text` as `cached stable prefix + sanitize(bounded tail)`. Equal
    /// to `sanitize_output(full_text)` because `stable_raw` is a match-free boundary.
    fn sanitized(&self, full_text: &str) -> String {
        let mut san = self.stable_san.clone();
        san.push_str(
            &self
                .pipeline
                .sanitize_output(&full_text[self.stable_raw..])
                .output,
        );
        san
    }

    /// Feed the full detokenized text so far; return any newly-settled sanitized
    /// prefix (≥ `holdback` chars behind the end, not splitting an alphanumeric run).
    /// `None` when nothing new has settled.
    pub(crate) fn push(&mut self, full_text: &str) -> Option<String> {
        let sanitized = self.sanitized(full_text);
        let cut = release_cut(&sanitized, self.holdback);
        let out = if cut > self.emitted {
            let o = sanitized[self.emitted..cut].to_string();
            self.emitted = cut;
            Some(o)
        } else {
            None
        };
        self.maybe_rebase(full_text);
        out
    }

    /// Terminal: sanitize the full buffer and return everything not yet released.
    pub(crate) fn flush(&mut self, full_text: &str) -> Option<String> {
        let sanitized = self.sanitized(full_text);
        if self.emitted >= sanitized.len() {
            self.emitted = sanitized.len();
            return None;
        }
        let out = sanitized[self.emitted..].to_string();
        self.emitted = sanitized.len();
        Some(out)
    }

    /// Advance the cached stable prefix so the re-sanitized tail stays bounded.
    /// Commits a new boundary only when it provably splits no PII match.
    fn maybe_rebase(&mut self, full_text: &str) {
        if full_text.len() - self.stable_raw <= REBASE_THRESHOLD {
            return;
        }
        let target = full_text.len() - 2 * self.holdback;
        let Some(p) = self.clean_split_at_or_before(full_text, target) else {
            return;
        };
        let add = self
            .pipeline
            .sanitize_output(&full_text[self.stable_raw..p])
            .output;
        self.stable_san.push_str(&add);
        self.stable_raw = p;
    }

    /// Largest char boundary in `(stable_raw, target]` at which no PII match straddles,
    /// or `None` if none is found within a bounded backward scan.
    fn clean_split_at_or_before(&self, full_text: &str, target: usize) -> Option<usize> {
        let mut p = floor_char_boundary(full_text, target);
        let limit = self.stable_raw.max(p.saturating_sub(2 * self.holdback));
        while p > limit {
            if full_text.is_char_boundary(p) && self.splits_cleanly(full_text, p) {
                return Some(p);
            }
            p -= 1;
        }
        None
    }

    /// True iff sanitizing the current tail across `p` equals sanitizing its two halves
    /// separately — i.e. no PII match straddles `p`. Uses the whole tail as context, so
    /// a straddling match of any length is detected (no false-clean).
    fn splits_cleanly(&self, full_text: &str, p: usize) -> bool {
        let tail = self.stable_raw;
        let joint = self.pipeline.sanitize_output(&full_text[tail..]).output;
        let mut split = self.pipeline.sanitize_output(&full_text[tail..p]).output;
        split.push_str(&self.pipeline.sanitize_output(&full_text[p..]).output);
        joint == split
    }
}

/// Largest char boundary ≤ `i` (clamped to the string length).
fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Byte offset up to which `sanitized` is settled: `holdback` characters behind the
/// end, backed up so the cut never splits an alphanumeric run.
fn release_cut(sanitized: &str, holdback: usize) -> usize {
    let chars: Vec<(usize, char)> = sanitized.char_indices().collect();
    if chars.len() <= holdback {
        return 0;
    }
    let mut idx = chars.len() - holdback;
    while idx > 0 && chars[idx - 1].1.is_alphanumeric() && chars[idx].1.is_alphanumeric() {
        idx -= 1;
    }
    if idx == 0 {
        0
    } else {
        chars[idx].0
    }
}

#[cfg(test)]
#[path = "stream_sanitizer_behavior_tests.rs"]
mod behavior_tests;

#[cfg(test)]
#[path = "stream_sanitizer_diff_tests.rs"]
mod diff_tests;
