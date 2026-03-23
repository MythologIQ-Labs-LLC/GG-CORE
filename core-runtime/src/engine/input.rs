//! Inference input types and validation for CORE Runtime.
//!
//! All inputs are validated before reaching the model. Invalid inputs are
//! rejected, not truncated — fail-closed security.

use super::error::InferenceError;

/// Maximum text input size in bytes (64KB).
pub const MAX_TEXT_BYTES: usize = 65_536;

/// Maximum batch size for batch operations.
pub const MAX_BATCH_SIZE: usize = 32;

/// Maximum token count per input.
pub const MAX_INPUT_TOKENS: usize = 4096;

/// Input variants for inference operations.
#[derive(Debug, Clone)]
pub enum InferenceInput {
    /// Single text input for classification/generation.
    Text(String),
    /// Batch of texts for embedding/classification.
    TextBatch(Vec<String>),
    /// Chat-style messages with typed roles.
    ChatMessages(Vec<ChatMessage>),
}

/// A single message in a chat conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// Typed chat roles — prevents invalid role strings at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl InferenceInput {
    /// Validate input against security constraints.
    pub fn validate(&self) -> Result<(), InferenceError> {
        match self {
            Self::Text(text) => validate_text(text),
            Self::TextBatch(batch) => validate_batch(batch),
            Self::ChatMessages(messages) => validate_messages(messages),
        }
    }

    /// Total byte size of the input.
    pub fn byte_size(&self) -> usize {
        match self {
            Self::Text(t) => t.len(),
            Self::TextBatch(b) => b.iter().map(|s| s.len()).sum(),
            Self::ChatMessages(m) => m.iter().map(|m| m.content.len()).sum(),
        }
    }
}

fn validate_text(text: &str) -> Result<(), InferenceError> {
    if text.is_empty() {
        return Err(InferenceError::InputValidation("text cannot be empty".into()));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(InferenceError::InputValidation(format!(
            "text exceeds maximum size: {} > {} bytes",
            text.len(),
            MAX_TEXT_BYTES
        )));
    }
    Ok(())
}

fn validate_batch(batch: &[String]) -> Result<(), InferenceError> {
    if batch.is_empty() {
        return Err(InferenceError::InputValidation("batch cannot be empty".into()));
    }
    if batch.len() > MAX_BATCH_SIZE {
        return Err(InferenceError::InputValidation(format!(
            "batch exceeds maximum size: {} > {}",
            batch.len(),
            MAX_BATCH_SIZE
        )));
    }
    for (i, text) in batch.iter().enumerate() {
        if let Err(e) = validate_text(text) {
            return Err(InferenceError::BatchItemValidation {
                index: i,
                error: Box::new(e),
            });
        }
    }
    Ok(())
}

fn validate_messages(messages: &[ChatMessage]) -> Result<(), InferenceError> {
    if messages.is_empty() {
        return Err(InferenceError::InputValidation("messages cannot be empty".into()));
    }
    let total_bytes: usize = messages.iter().map(|m| m.content.len()).sum();
    if total_bytes > MAX_TEXT_BYTES {
        return Err(InferenceError::InputValidation(format!(
            "total message content exceeds maximum: {} > {} bytes",
            total_bytes, MAX_TEXT_BYTES
        )));
    }
    for msg in messages {
        if msg.content.is_empty() {
            return Err(InferenceError::InputValidation(
                "message content cannot be empty".into(),
            ));
        }
    }
    Ok(())
}
