//! Security tests for input validation.
//!
//! Tests boundary conditions, malformed inputs, and edge cases that could
//! bypass input validation or cause security issues.

use gg_core::engine::{
    InferenceInput, ChatMessage, ChatRole, MAX_TEXT_BYTES, MAX_BATCH_SIZE,
};

#[test]
fn reject_text_exceeding_64kb() {
    // Create text that exceeds MAX_TEXT_BYTES (64KB)
    let oversized_text = "x".repeat(MAX_TEXT_BYTES + 1);
    let input = InferenceInput::Text(oversized_text);

    let result = input.validate();
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("exceeds maximum size"));
}

#[test]
fn accept_text_at_exactly_64kb() {
    // Text at exactly MAX_TEXT_BYTES should be accepted
    let max_text = "x".repeat(MAX_TEXT_BYTES);
    let input = InferenceInput::Text(max_text);

    let result = input.validate();
    assert!(result.is_ok());
}

#[test]
fn reject_batch_exceeding_32_items() {
    // Create batch that exceeds MAX_BATCH_SIZE (32)
    let oversized_batch: Vec<String> = (0..MAX_BATCH_SIZE + 1)
        .map(|i| format!("item {}", i))
        .collect();
    let input = InferenceInput::TextBatch(oversized_batch);

    let result = input.validate();
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(err.to_string().contains("batch exceeds maximum size"));
}

#[test]
fn accept_batch_at_exactly_32_items() {
    // Batch at exactly MAX_BATCH_SIZE should be accepted
    let max_batch: Vec<String> = (0..MAX_BATCH_SIZE)
        .map(|i| format!("item {}", i))
        .collect();
    let input = InferenceInput::TextBatch(max_batch);

    let result = input.validate();
    assert!(result.is_ok());
}

#[test]
fn reject_empty_text_input() {
    let input = InferenceInput::Text(String::new());

    let result = input.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn reject_empty_batch() {
    let input = InferenceInput::TextBatch(vec![]);

    let result = input.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn reject_empty_messages() {
    let input = InferenceInput::ChatMessages(vec![]);

    let result = input.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn reject_empty_message_content() {
    let messages = vec![ChatMessage {
        role: ChatRole::User,
        content: String::new(),
    }];
    let input = InferenceInput::ChatMessages(messages);

    let result = input.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("content cannot be empty"));
}

#[test]
fn reject_messages_total_exceeding_64kb() {
    // Multiple messages whose total exceeds MAX_TEXT_BYTES
    let large_content = "x".repeat(MAX_TEXT_BYTES / 2 + 100);
    let messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: large_content.clone(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: large_content,
        },
    ];
    let input = InferenceInput::ChatMessages(messages);

    let result = input.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
}

#[test]
fn accept_valid_utf8_text() {
    // Valid UTF-8 with various unicode characters
    let unicode_text = "Hello 世界 🌍 مرحبا";
    let input = InferenceInput::Text(unicode_text.to_string());

    let result = input.validate();
    assert!(result.is_ok());
}

#[test]
fn byte_size_calculation_correct() {
    let text = "Hello";
    let input = InferenceInput::Text(text.to_string());
    assert_eq!(input.byte_size(), 5);

    let batch = InferenceInput::TextBatch(vec!["abc".into(), "defgh".into()]);
    assert_eq!(batch.byte_size(), 8);

    let messages = InferenceInput::ChatMessages(vec![
        ChatMessage { role: ChatRole::User, content: "test".into() },
    ]);
    assert_eq!(messages.byte_size(), 4);
}
