//! Prefill executor optimized for parallel prompt processing.
//!
//! Processes prompt tokens in chunks with batch-parallel execution.

use crate::engine::InferenceError;
use crate::memory::paged::{PageId, PageTable, PAGE_TOKENS};

/// Result from prefill phase.
#[derive(Debug, Clone)]
pub struct PrefillResult {
    /// Length of KV-cache after prefill.
    pub kv_len: usize,
    /// Number of chunks processed.
    pub chunks_processed: usize,
}

/// Configuration for prefill execution.
#[derive(Debug, Clone)]
pub struct PrefillConfig {
    /// Chunk size for parallel processing.
    pub chunk_size: usize,
    /// Hidden dimension for KV storage.
    pub hidden_dim: usize,
}

impl Default for PrefillConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            hidden_dim: 768,
        }
    }
}

/// Prefill executor optimized for parallel prompt processing.
#[derive(Debug)]
pub struct PrefillExecutor {
    config: PrefillConfig,
}

impl PrefillExecutor {
    /// Create a new prefill executor.
    pub fn new(config: PrefillConfig) -> Self {
        Self { config }
    }

    /// Process prompt tokens and populate KV-cache.
    pub fn execute(
        &self,
        tokens: &[u32],
        page_table: &mut PageTable,
    ) -> Result<PrefillResult, InferenceError> {
        if tokens.is_empty() {
            return Err(InferenceError::InputValidation(
                "prefill requires non-empty prompt".into(),
            ));
        }

        let chunks: Vec<_> = tokens.chunks(self.config.chunk_size).collect();
        let mut pos = 0;

        for chunk in &chunks {
            self.process_chunk(chunk, pos, page_table)?;
            pos += chunk.len();
        }

        Ok(PrefillResult {
            kv_len: tokens.len(),
            chunks_processed: chunks.len(),
        })
    }

    /// Process a single chunk of tokens.
    fn process_chunk(
        &self,
        tokens: &[u32],
        start_pos: usize,
        page_table: &mut PageTable,
    ) -> Result<(), InferenceError> {
        let mut current_page_id: Option<PageId> = None;
        for (i, _token) in tokens.iter().enumerate() {
            let seq_pos = start_pos + i;
            // Allocate a new page at each page boundary.
            if seq_pos % PAGE_TOKENS == 0 {
                let page_id =
                    page_table
                        .allocate_page()
                        .ok_or_else(|| InferenceError::MemoryExceeded {
                            used: seq_pos,
                            limit: seq_pos,
                        })?;
                current_page_id = Some(page_id);
            }
            let slot = PageTable::slot_in_page(seq_pos);
            if let Some(pid) = current_page_id {
                if let Some(page) = page_table.page_mut(pid) {
                    let keys = vec![0.0f32; self.config.hidden_dim];
                    let values = vec![0.0f32; self.config.hidden_dim];
                    page.write(slot, &keys, &values);
                }
            }
        }
        Ok(())
    }

    /// Estimate pages needed for prompt length.
    pub fn estimate_pages(prompt_len: usize) -> usize {
        (prompt_len + PAGE_TOKENS - 1) / PAGE_TOKENS
    }

    pub fn config(&self) -> &PrefillConfig {
        &self.config
    }
}
