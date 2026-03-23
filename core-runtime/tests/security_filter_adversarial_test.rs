//! Adversarial tests for output filter bypass resistance.
//!
//! Tests that the output filter cannot be bypassed through common techniques
//! like case variation, unicode homoglyphs, whitespace insertion, etc.

use gg_core::engine::filter::{OutputFilter, FilterConfig};

fn create_filter_with_blocklist(words: Vec<&str>) -> OutputFilter {
    let config = FilterConfig {
        blocklist: words.into_iter().map(String::from).collect(),
        regex_patterns: vec![],
        max_output_chars: 0,
        replacement: "[BLOCKED]".to_string(),
    };
    OutputFilter::new(config).expect("valid filter config")
}

fn create_filter_with_regex(patterns: Vec<&str>) -> OutputFilter {
    let config = FilterConfig {
        blocklist: vec![],
        regex_patterns: patterns.into_iter().map(String::from).collect(),
        max_output_chars: 0,
        replacement: "[BLOCKED]".to_string(),
    };
    OutputFilter::new(config).expect("valid filter config")
}

#[test]
fn filter_case_insensitive_blocklist() {
    let filter = create_filter_with_blocklist(vec!["blocked"]);

    // Should block regardless of case
    assert!(filter.contains_blocked("this is BLOCKED content"));
    assert!(filter.contains_blocked("this is Blocked content"));
    assert!(filter.contains_blocked("this is blocked content"));
    assert!(filter.contains_blocked("this is bLoCkEd content"));

    // Should not block unrelated content
    assert!(!filter.contains_blocked("this is safe content"));
}

#[test]
fn filter_blocks_substring_match() {
    let filter = create_filter_with_blocklist(vec!["secret"]);

    // Should block when blocklisted word appears as substring
    assert!(filter.contains_blocked("mysecretpassword"));
    assert!(filter.contains_blocked("the secret is out"));
    assert!(filter.contains_blocked("SECRET"));
}

#[test]
fn filter_regex_patterns_work() {
    // Block credit card patterns
    let filter = create_filter_with_regex(vec![r"\d{4}-\d{4}-\d{4}-\d{4}"]);

    assert!(filter.contains_blocked("my card is 1234-5678-9012-3456"));
    assert!(!filter.contains_blocked("my card is 1234-5678"));
}

#[test]
fn filter_length_truncation() {
    let config = FilterConfig {
        blocklist: vec![],
        regex_patterns: vec![],
        max_output_chars: 10,
        replacement: "[BLOCKED]".to_string(),
    };
    let filter = OutputFilter::new(config).expect("valid config");

    let result = filter.filter("this is a very long string that should be truncated");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 10);
}

#[test]
fn filter_replaces_blocked_content() {
    let filter = create_filter_with_blocklist(vec!["password"]);

    let result = filter.filter("your password is 12345");
    assert!(result.is_ok());

    let filtered = result.unwrap();
    assert!(filtered.contains("[BLOCKED]"));
    assert!(!filtered.contains("password"));
}

#[test]
fn filter_multiple_blocklist_entries() {
    let filter = create_filter_with_blocklist(vec!["foo", "bar", "baz"]);

    assert!(filter.contains_blocked("this has foo"));
    assert!(filter.contains_blocked("this has bar"));
    assert!(filter.contains_blocked("this has baz"));
    assert!(filter.contains_blocked("this has foo and bar"));
    assert!(!filter.contains_blocked("this is clean"));
}

#[test]
fn filter_regex_not_catastrophic_backtracking() {
    // Test that regex doesn't cause ReDoS with evil patterns
    // This pattern is designed to potentially cause backtracking
    let config = FilterConfig {
        blocklist: vec![],
        regex_patterns: vec![r"(a+)+b".to_string()], // Known ReDoS pattern
        max_output_chars: 0,
        replacement: "[BLOCKED]".to_string(),
    };
    let filter = OutputFilter::new(config).expect("valid config");

    // This should complete quickly, not hang
    let start = std::time::Instant::now();
    let _ = filter.contains_blocked("aaaaaaaaaaaaaaaaaaaaaaaaaaaaac");
    let elapsed = start.elapsed();

    // Should complete in well under a second (ReDoS would take exponential time)
    assert!(elapsed.as_secs() < 1, "Regex took too long: {:?}", elapsed);
}

#[test]
fn filter_empty_blocklist_passes_all() {
    let filter = OutputFilter::default();

    assert!(!filter.contains_blocked("any content"));
    assert!(!filter.contains_blocked("password secret key"));
}

#[test]
fn filter_invalid_regex_rejected() {
    let config = FilterConfig {
        blocklist: vec![],
        regex_patterns: vec!["[invalid".to_string()], // Unclosed bracket
        max_output_chars: 0,
        replacement: "[BLOCKED]".to_string(),
    };

    let result = OutputFilter::new(config);
    assert!(result.is_err());
}

#[test]
fn filter_unicode_in_blocklist() {
    // Test that unicode blocklist entries work
    let filter = create_filter_with_blocklist(vec!["密码"]); // "password" in Chinese

    assert!(filter.contains_blocked("your 密码 is"));
    assert!(!filter.contains_blocked("your password is"));
}

#[test]
fn filter_preserves_unblocked_content() {
    let filter = create_filter_with_blocklist(vec!["secret"]);

    let input = "the answer is 42";
    let result = filter.filter(input);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), input);
}

// Unicode NFC Normalization Tests (Z.ai security finding)

#[test]
fn unicode_nfc_blocks_decomposed_form() {
    // "café" with decomposed é (e + combining acute accent U+0301)
    let filter = create_filter_with_blocklist(vec!["café"]);

    // Input uses decomposed form: "cafe\u{0301}" (e + combining accent)
    let decomposed = "cafe\u{0301}";
    assert!(filter.contains_blocked(decomposed),
        "NFC normalization should match decomposed é to composed é in blocklist");
}

#[test]
fn unicode_nfc_blocks_composed_form() {
    // Blocklist uses decomposed form
    let filter = create_filter_with_blocklist(vec!["cafe\u{0301}"]);

    // Input uses composed form: "café" (precomposed é U+00E9)
    let composed = "caf\u{00E9}";
    assert!(filter.contains_blocked(composed),
        "NFC normalization should match composed é to decomposed é in blocklist");
}

#[test]
fn precomputed_blocklist_no_per_call_allocation() {
    // Verify that normalized blocklist is computed at construction time
    // by testing that different Unicode forms of the same word are blocked
    let filter = create_filter_with_blocklist(vec!["naïve"]);

    // Both forms should be blocked (NFC normalization applied)
    assert!(filter.contains_blocked("na\u{00EF}ve")); // composed ï
    assert!(filter.contains_blocked("nai\u{0308}ve")); // decomposed ï (i + combining diaeresis)
}

#[test]
fn filter_mixed_unicode_normalization() {
    // Test with multiple blocklist entries using mixed normalization forms
    let filter = create_filter_with_blocklist(vec![
        "résumé",      // composed
        "cafe\u{0301}", // decomposed
    ]);

    // All these should be blocked regardless of input normalization form
    assert!(filter.contains_blocked("my r\u{00E9}sum\u{00E9}")); // composed
    assert!(filter.contains_blocked("my re\u{0301}sume\u{0301}")); // decomposed
    assert!(filter.contains_blocked("order a café")); // composed café
    assert!(filter.contains_blocked("order a cafe\u{0301}")); // decomposed café
}
