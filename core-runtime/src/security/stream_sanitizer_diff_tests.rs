//! Differential + safety tests for the cached-stable-prefix `StreamSanitizer` (B-36).
//!
//! The cached-prefix reconstruction must be byte-identical to the pre-B-36 whole-buffer
//! sanitize. These tests drive adversarial streams through both and assert equality, and
//! guard the ledger #162 regression (raw-cut leaked space-separated PII such as a credit
//! card). `gguf`-gated transitively via the parent module.

use super::{release_cut, StreamSanitizer};
use crate::security::{SecurityConfig, SecurityPipeline};

fn redacting() -> SecurityPipeline {
    SecurityPipeline::from_config(&SecurityConfig {
        enable_prompt_injection_detection: false,
        block_prompt_injection: false,
        enable_pii_detection: true,
        redact_pii: true,
        enable_model_encryption: false,
        encryption_key: None,
    })
}

/// Raw PII tokens injected into streams; none may ever be released verbatim.
const PII_TOKENS: &[&str] = &[
    "john.doe@example.com",
    "555-123-4567",
    "123-45-6789",
    "4111 1111 1111 1111",
];

/// Deterministic adversarial complete-texts (no `rand`): clean filler interleaved with
/// PII at start / middle / end and repeated, plus a long fully-clean case that forces a
/// rebase, and a long case with an embedded credit card past the rebase window.
fn stream_cases() -> Vec<String> {
    // PII tokens are kept at word boundaries (surrounded by spaces / string ends) so
    // the `\b`-anchored regexes actually match them — gluing a token to a letter would
    // make it a non-match that even one-shot sanitize leaves raw.
    let f = "the quick brown fox jumps over the lazy dog ";
    let long = f.repeat(8); // 352 chars — pushes the case past REBASE_THRESHOLD (512)
    let mut cases = Vec::new();
    for tok in PII_TOKENS {
        cases.push(format!("{f}contact {tok} today {f}"));
        cases.push(format!("{tok} {f}"));
        cases.push(format!("{f}{tok}"));
        cases.push(format!("a {tok} b {tok} c"));
        // > REBASE_THRESHOLD with PII on both sides of the rebase window.
        cases.push(format!("{long}card {tok} mid {long}"));
    }
    cases.push(f.repeat(12)); // long, fully clean → exercises rebase on clean text
    cases
}

/// Growing prefixes at every char boundary (the caller feeds the full text so far).
fn drive<F>(text: &str, mut step: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    let mut out = String::new();
    for (i, _) in text.char_indices() {
        if i == 0 {
            continue;
        }
        if let Some(o) = step(&text[..i]) {
            out.push_str(&o);
        }
    }
    out
}

fn run_stream(p: &SecurityPipeline, holdback: usize, text: &str) -> String {
    let mut s = StreamSanitizer {
        pipeline: p,
        holdback,
        emitted: 0,
        stable_raw: 0,
        stable_san: String::new(),
    };
    let mut out = drive(text, |pre| s.push(pre));
    if let Some(o) = s.push(text) {
        out.push_str(&o);
    }
    if let Some(o) = s.flush(text) {
        out.push_str(&o);
    }
    out
}

/// The pre-B-36 whole-buffer reference: sanitize the full buffer each push,
/// `release_cut` on the sanitized string, sanitized-offset cursor.
struct WholeBufferRef<'a> {
    pipeline: &'a SecurityPipeline,
    holdback: usize,
    emitted: usize,
}

impl<'a> WholeBufferRef<'a> {
    fn push(&mut self, full: &str) -> Option<String> {
        let s = self.pipeline.sanitize_output(full).output;
        let cut = release_cut(&s, self.holdback);
        if cut <= self.emitted {
            return None;
        }
        let out = s[self.emitted..cut].to_string();
        self.emitted = cut;
        Some(out)
    }
    fn flush(&mut self, full: &str) -> Option<String> {
        let s = self.pipeline.sanitize_output(full).output;
        if self.emitted >= s.len() {
            return None;
        }
        let out = s[self.emitted..].to_string();
        self.emitted = s.len();
        Some(out)
    }
}

fn run_reference(p: &SecurityPipeline, holdback: usize, text: &str) -> String {
    let mut s = WholeBufferRef {
        pipeline: p,
        holdback,
        emitted: 0,
    };
    let mut out = drive(text, |pre| s.push(pre));
    if let Some(o) = s.push(text) {
        out.push_str(&o);
    }
    if let Some(o) = s.flush(text) {
        out.push_str(&o);
    }
    out
}

#[test]
fn matches_whole_buffer_reference_byte_for_byte() {
    let p = redacting();
    for holdback in [128usize, 8] {
        for text in stream_cases() {
            let got = run_stream(&p, holdback, &text);
            let want = run_reference(&p, holdback, &text);
            assert_eq!(
                got, want,
                "cached-prefix must match whole-buffer reference; holdback={holdback} text={text:?}"
            );
        }
    }
}

#[test]
fn terminal_equals_one_shot_at_production_holdback() {
    let p = redacting();
    for text in stream_cases() {
        let streamed = run_stream(&p, 128, &text);
        let one_shot = p.sanitize_output(&text).output;
        assert_eq!(
            streamed, one_shot,
            "stream+flush must equal one-shot sanitize at production holdback; text={text:?}"
        );
    }
}

#[test]
fn never_emits_raw_pii_including_space_separated() {
    // Production holdback (128) exceeds every structured token here (≤ 19 chars), so no
    // leak is the real guarantee. The credit card is the ledger-#162 regression guard:
    // the abandoned raw-cut design released it here; the sanitized-side design must not.
    // (Byte-identity to the shipped code at holdback=8 — incl. its documented > -holdback
    // residual — is covered by `matches_whole_buffer_reference_byte_for_byte`.)
    let p = redacting();
    for text in stream_cases() {
        let released = run_stream(&p, 128, &text);
        for tok in PII_TOKENS {
            if text.contains(tok) {
                assert!(
                    !released.contains(tok),
                    "raw PII {tok:?} leaked at production holdback; text={text:?}"
                );
            }
        }
    }
}
