//! Streaming-safe egress PII sanitizer.
//!
//! Wraps [`SecurityPipeline::sanitize_output`] for token-by-token generation.
//! Each `push` feeds the full detokenized text so far; the sanitizer re-sanitizes
//! the whole buffer and releases only the sanitized prefix that is at least
//! `HOLDBACK` characters behind the growing end — far enough that no in-flight PII
//! match can still extend into it. `flush` sanitizes the remaining tail on the
//! terminal frame.
//!
//! Correctness: a PII match of length ≤ `HOLDBACK` ending after the cut started
//! within `HOLDBACK` of the end, so it lies entirely in the withheld tail and is
//! never emitted raw; once `HOLDBACK` further characters arrive it is complete and
//! redacted before release. The settled prefix (before the cut) contains only
//! completed matches, so it is byte-stable across pushes. Documented residual: a
//! PII string longer than `HOLDBACK` split exactly at the window boundary (see
//! B-24b, ledger Entry #121).

use super::SecurityPipeline;

/// Trailing sanitized characters withheld until settled.
const HOLDBACK: usize = 128;

pub struct StreamSanitizer<'a> {
    pipeline: &'a SecurityPipeline,
    holdback: usize,
    /// Bytes of the (byte-stable) settled prefix already released.
    emitted: usize,
}

impl<'a> StreamSanitizer<'a> {
    pub(crate) fn new(pipeline: &'a SecurityPipeline) -> Self {
        Self {
            pipeline,
            holdback: HOLDBACK,
            emitted: 0,
        }
    }

    /// Feed the full detokenized text so far; return any newly-settled sanitized
    /// prefix (≥ `holdback` chars behind the end, not splitting an alphanumeric
    /// run). `None` when nothing new has settled.
    pub(crate) fn push(&mut self, full_text: &str) -> Option<String> {
        let sanitized = self.pipeline.sanitize_output(full_text).output;
        let cut = release_cut(&sanitized, self.holdback);
        if cut <= self.emitted {
            return None;
        }
        let out = sanitized[self.emitted..cut].to_string();
        self.emitted = cut;
        Some(out)
    }

    /// Terminal: sanitize the full buffer and return everything not yet released.
    pub(crate) fn flush(&mut self, full_text: &str) -> Option<String> {
        let sanitized = self.pipeline.sanitize_output(full_text).output;
        if self.emitted >= sanitized.len() {
            self.emitted = sanitized.len();
            return None;
        }
        let out = sanitized[self.emitted..].to_string();
        self.emitted = sanitized.len();
        Some(out)
    }
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
mod tests {
    use super::*;
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

    impl<'a> StreamSanitizer<'a> {
        fn with_holdback(pipeline: &'a SecurityPipeline, holdback: usize) -> Self {
            Self {
                pipeline,
                holdback,
                emitted: 0,
            }
        }
    }

    const EMAIL: &str = "john.doe@example.com";

    #[test]
    fn flush_redacts_pii_tail() {
        let p = redacting();
        let mut s = StreamSanitizer::new(&p);
        // Short buffer: nothing settles under the default holdback.
        assert!(s.push(&format!("contact {EMAIL}")).is_none());
        let out = s.flush(&format!("contact {EMAIL}")).expect("tail flushed");
        assert!(!out.contains(EMAIL), "email must be redacted on flush");
    }

    #[test]
    fn multi_word_pii_split_across_pushes_is_redacted() {
        let p = redacting();
        let mut s = StreamSanitizer::with_holdback(&p, 8);
        let mut released = String::new();
        // The email is completed only in the second push, straddling the window.
        let filler = "some clean words here and there ";
        if let Some(o) = s.push(&format!("{filler}my email is john.doe@exa")) {
            released.push_str(&o);
        }
        if let Some(o) = s.push(&format!("{filler}my email is {EMAIL} thanks")) {
            released.push_str(&o);
        }
        if let Some(o) = s.flush(&format!("{filler}my email is {EMAIL} thanks")) {
            released.push_str(&o);
        }
        assert!(
            !released.contains(EMAIL),
            "email split across pushes must never be emitted raw; got: {released}"
        );
        assert!(released.contains(filler.trim()), "clean text still flows");
    }

    #[test]
    fn utf8_multibyte_is_not_corrupted() {
        let p = redacting();
        let mut s = StreamSanitizer::with_holdback(&p, 4);
        let text = "héllo wörld 日本語 café ☕ more clean text here";
        let mut out = String::new();
        if let Some(o) = s.push(text) {
            out.push_str(&o);
        }
        if let Some(o) = s.flush(text) {
            out.push_str(&o);
        }
        // Concatenated release reconstructs the original clean text intact.
        assert_eq!(
            out, text,
            "multibyte text must round-trip without corruption"
        );
    }

    #[test]
    fn clean_stream_passes_through() {
        let p = redacting();
        let mut s = StreamSanitizer::with_holdback(&p, 4);
        let text = "the quick brown fox jumps over the lazy dog repeatedly";
        let mut out = String::new();
        if let Some(o) = s.push(text) {
            out.push_str(&o);
        }
        if let Some(o) = s.flush(text) {
            out.push_str(&o);
        }
        assert_eq!(out, text);
    }
}
