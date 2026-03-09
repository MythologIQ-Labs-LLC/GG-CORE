//! Decode executor optimized for single-token latency.
//!
//! Generates tokens sequentially with minimal latency per step.

use crate::engine::{FinishReason, InferenceError};
use crate::memory::paged::{PageTable, PAGE_TOKENS};

/// Result from a single decode step.
#[derive(Debug, Clone)]
pub struct DecodeStepResult {
    /// Generated token (None if no output yet).
    pub token: Option<u32>,
    /// Whether generation is finished.
    pub finished: bool,
    /// Reason for finishing (if applicable).
    pub finish_reason: Option<FinishReason>,
}

/// Configuration for decode execution.
#[derive(Debug, Clone)]
pub struct DecodeConfig {
    /// Hidden dimension for KV storage.
    pub hidden_dim: usize,
    /// End-of-sequence token ID.
    pub eos_token: u32,
}

impl Default for DecodeConfig {
    fn default() -> Self {
        Self {
            hidden_dim: 768,
            eos_token: 2, // Common EOS token ID
        }
    }
}

/// Decode executor optimized for single-token latency.
#[derive(Debug)]
pub struct DecodeExecutor {
    config: DecodeConfig,
    current_pos: usize,
    tokens_generated: usize,
}

impl DecodeExecutor {
    /// Create a new decode executor.
    pub fn new(config: DecodeConfig) -> Self {
        Self {
            config,
            current_pos: 0,
            tokens_generated: 0,
        }
    }

    /// Initialize decoder with prefill position.
    pub fn init(&mut self, prefill_len: usize) {
        self.current_pos = prefill_len;
        self.tokens_generated = 0;
    }

    /// Generate a single token with minimal latency.
    pub fn step(
        &mut self,
        page_table: &mut PageTable,
        max_tokens: usize,
    ) -> Result<DecodeStepResult, InferenceError> {
        if self.tokens_generated >= max_tokens {
            return Ok(DecodeStepResult {
                token: None,
                finished: true,
                finish_reason: Some(FinishReason::MaxTokens),
            });
        }

        // Allocate page for new position
        page_table
            .allocate(self.current_pos)
            .ok_or_else(|| InferenceError::MemoryExceeded {
                used: self.current_pos,
                limit: self.current_pos,
            })?;

        // Simulate token generation (actual model would sample here)
        let token = self.sample_token()?;

        // Write KV for generated position
        self.write_kv(page_table)?;

        self.current_pos += 1;
        self.tokens_generated += 1;

        // Check for EOS
        if token == self.config.eos_token {
            return Ok(DecodeStepResult {
                token: Some(token),
                finished: true,
                finish_reason: Some(FinishReason::Stop),
            });
        }

        Ok(DecodeStepResult {
            token: Some(token),
            finished: false,
            finish_reason: None,
        })
    }

    fn sample_token(&self) -> Result<u32, InferenceError> {
        // No model loaded - fail rather than return stub data
        // Real implementation requires model forward pass and sampling
        Err(InferenceError::ModelError(
            "decode executor requires integrated model - use InferenceEngine".into(),
        ))
    }

    fn write_kv(&self, page_table: &mut PageTable) -> Result<(), InferenceError> {
        let slot = PageTable::slot_in_page(self.current_pos);
        if let Some(page) = page_table.get_mut(self.current_pos) {
            let keys = vec![0.0f32; self.config.hidden_dim];
            let values = vec![0.0f32; self.config.hidden_dim];
            page.write(slot, &keys, &values);
        }
        Ok(())
    }

    /// Estimate pages needed for generation length.
    pub fn estimate_pages(current_pos: usize, max_tokens: usize) -> usize {
        let end_pos = current_pos + max_tokens;
        (end_pos + PAGE_TOKENS - 1) / PAGE_TOKENS
    }

    pub fn config(&self) -> &DecodeConfig {
        &self.config
    }
    pub fn tokens_generated(&self) -> usize {
        self.tokens_generated
    }
    pub fn current_pos(&self) -> usize {
        self.current_pos
    }
}
