//! Decode executor optimized for single-token latency.
//!
//! Generates tokens sequentially with minimal latency per step.

use crate::engine::{FinishReason, InferenceError};
#[cfg(feature = "advanced")]
use crate::engine::SpeculativeConfig;
use crate::memory::paged::{PageId, PageTable, PAGE_TOKENS};

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
    /// Enable speculative decoding.
    #[cfg(feature = "advanced")]
    pub speculative: Option<SpeculativeConfig>,
}

impl Default for DecodeConfig {
    fn default() -> Self {
        Self {
            hidden_dim: 768,
            eos_token: 2, // Common EOS token ID
            #[cfg(feature = "advanced")]
            speculative: None,
        }
    }
}

/// Decode executor optimized for single-token latency.
#[derive(Debug)]
pub struct DecodeExecutor {
    config: DecodeConfig,
    current_pos: usize,
    tokens_generated: usize,
    /// PageId for the current page (updated at each page boundary).
    current_page_id: Option<PageId>,
}

impl DecodeExecutor {
    /// Create a new decode executor.
    pub fn new(config: DecodeConfig) -> Self {
        Self {
            config,
            current_pos: 0,
            tokens_generated: 0,
            current_page_id: None,
        }
    }

    /// Initialize decoder with prefill position.
    pub fn init(&mut self, prefill_len: usize) {
        self.current_pos = prefill_len;
        self.tokens_generated = 0;
        self.current_page_id = None;
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

        // Allocate a new page at each page boundary.
        if self.current_pos % PAGE_TOKENS == 0 {
            let page_id = page_table.allocate_page().ok_or_else(|| {
                InferenceError::MemoryExceeded {
                    used: self.current_pos,
                    limit: self.current_pos,
                }
            })?;
            self.current_page_id = Some(page_id);
        }

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
        if let Some(pid) = self.current_page_id {
            if let Some(page) = page_table.page_mut(pid) {
                let keys = vec![0.0f32; self.config.hidden_dim];
                let values = vec![0.0f32; self.config.hidden_dim];
                page.write(slot, &keys, &values);
            }
        }
        Ok(())
    }

    /// Estimate pages needed for generation length.
    pub fn estimate_pages(current_pos: usize, max_tokens: usize) -> usize {
        let end_pos = current_pos + max_tokens;
        (end_pos + PAGE_TOKENS - 1) / PAGE_TOKENS
    }

    pub fn config(&self) -> &DecodeConfig { &self.config }
    pub fn tokens_generated(&self) -> usize { self.tokens_generated }
    pub fn current_pos(&self) -> usize { self.current_pos }
}
