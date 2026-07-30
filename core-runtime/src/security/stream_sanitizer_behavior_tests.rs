//! Behavior tests for `StreamSanitizer` (relocated from the inline module for Razor).
//! `gguf`-gated transitively via the parent module.

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
            stable_raw: 0,
            stable_san: String::new(),
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
    assert_eq!(out, text, "multibyte text must round-trip without corruption");
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
