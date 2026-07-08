//! KV Cache read, attention, eviction, and query operations.

use super::kv_cache_config::{
    read_or_recover, write_or_recover, KvCacheError, KvCacheStats, SequenceId,
};
use super::kv_cache_core::KvCacheManager;
use super::paged::{PageId, PAGE_TOKENS};

impl KvCacheManager {
    /// Read KV pairs from a sequence at given position.
    pub fn read_kv(
        &self,
        seq_id: SequenceId,
        pos: usize,
        keys_out: &mut [f32],
        values_out: &mut [f32],
    ) -> Result<(), KvCacheError> {
        // Resolve page_id from per-sequence page_ids, then drop sequences lock.
        let (page_id, slot) = {
            let mut store = write_or_recover(&self.sequences);
            store.touch(seq_id);
            let entry = store.entries
                .get_mut(&seq_id)
                .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;

            if pos >= entry.seq_len {
                return Err(KvCacheError::PositionOutOfBounds {
                    pos,
                    seq_len: entry.seq_len,
                });
            }
            entry.last_access = std::time::Instant::now();
            entry.access_count += 1;

            #[cfg(feature = "advanced")]
            {
                if let Some(ref qs) = entry.quant_store {
                    if pos < qs.seq_len() {
                        qs.read_keys(pos, keys_out);
                        qs.read_values(pos, values_out);
                        return Ok(());
                    }
                }
            }

            let page_idx = pos / PAGE_TOKENS;
            let pid = *entry.page_ids.get(page_idx).ok_or(KvCacheError::PageNotFound)?;
            (pid, pos % PAGE_TOKENS)
        };
        self.read_from_page_table(page_id, slot, keys_out, values_out)
    }

    fn read_from_page_table(
        &self,
        page_id: PageId,
        slot: usize,
        keys_out: &mut [f32],
        values_out: &mut [f32],
    ) -> Result<(), KvCacheError> {
        let page_table = read_or_recover(&self.page_table);
        if let Some(page) = page_table.page(page_id) {
            keys_out.copy_from_slice(page.read_keys(slot));
            values_out.copy_from_slice(page.read_values(slot));
            Ok(())
        } else {
            Err(KvCacheError::PageNotFound)
        }
    }

    /// Compute attention scores for a query against cached keys.
    pub fn attention_scores(
        &self,
        seq_id: SequenceId,
        query: &[f32],
        scores_out: &mut [f32],
    ) -> Result<(), KvCacheError> {
        // Clone page_ids under sequences lock, then drop before accessing page_table.
        let (seq_len, page_ids) = {
            let store = read_or_recover(&self.sequences);
            let entry = store.entries
                .get(&seq_id)
                .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;

            #[cfg(feature = "advanced")]
            {
                if let Some(ref qs) = entry.quant_store {
                    if qs.seq_len() >= entry.seq_len {
                        qs.attention_scores(query, scores_out);
                        return Ok(());
                    }
                }
            }

            (entry.seq_len, entry.page_ids.clone())
        };
        self.attention_from_pages(seq_len, &page_ids, query, scores_out)
    }

    fn attention_from_pages(
        &self,
        seq_len: usize,
        page_ids: &[PageId],
        query: &[f32],
        scores_out: &mut [f32],
    ) -> Result<(), KvCacheError> {
        let page_table = read_or_recover(&self.page_table);
        for pos in 0..seq_len {
            let page_idx = pos / PAGE_TOKENS;
            if let Some(&pid) = page_ids.get(page_idx) {
                if let Some(page) = page_table.page(pid) {
                    let slot = pos % PAGE_TOKENS;
                    scores_out[pos] = Self::dot_product(query, page.read_keys(slot));
                }
            }
        }
        Ok(())
    }

    /// Evict KV cache entries beyond the sliding window boundary.
    pub fn evict_beyond_window(&self, seq_id: SequenceId, current_pos: usize) -> usize {
        let sw = match &self.config.sliding_window {
            Some(sw) => sw.clone(),
            None => return 0,
        };
        let keep = sw.window_size.saturating_add(sw.overlap_tokens);
        let cutoff = current_pos.saturating_sub(keep);
        if cutoff == 0 {
            return 0;
        }
        self.evict_pages_before(seq_id, cutoff)
    }

    fn evict_pages_before(&self, seq_id: SequenceId, cutoff_token: usize) -> usize {
        // Lock order: sequences → page_table (consistent with free_sequence).
        let mut store = write_or_recover(&self.sequences);
        let entry = match store.entries.get_mut(&seq_id) {
            Some(e) => e,
            None => return 0,
        };
        let cutoff_page = cutoff_token / PAGE_TOKENS;
        if cutoff_page == 0 {
            return 0;
        }
        let evict_count = cutoff_page.min(entry.page_ids.len());
        let evicted: Vec<_> = entry.page_ids.drain(..evict_count).collect();
        drop(store);
        let mut page_table = write_or_recover(&self.page_table);
        page_table.free(&evicted);
        evict_count
    }

    /// Get current statistics.
    pub fn stats(&self) -> KvCacheStats {
        (*self.stats).clone()
    }

    /// Get sequence length.
    pub fn seq_len(&self, seq_id: SequenceId) -> Result<usize, KvCacheError> {
        let store = read_or_recover(&self.sequences);
        let entry = store.entries
            .get(&seq_id)
            .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;
        Ok(entry.seq_len)
    }

    /// Check if sequence exists.
    pub fn has_sequence(&self, seq_id: SequenceId) -> bool {
        read_or_recover(&self.sequences).entries.contains_key(&seq_id)
    }

    /// Get number of active sequences.
    pub fn active_sequences(&self) -> usize {
        read_or_recover(&self.sequences).entries.len()
    }

    /// Get memory usage in bytes.
    pub fn memory_usage(&self) -> usize {
        let page_table = read_or_recover(&self.page_table);
        let count = page_table.page_count();
        count * PAGE_TOKENS * self.config.hidden_dim * 2 * std::mem::size_of::<f32>()
    }

    /// Get page count for a sequence (for testing).
    pub fn sequence_page_count(&self, seq_id: SequenceId) -> usize {
        let store = read_or_recover(&self.sequences);
        store.entries.get(&seq_id).map_or(0, |e| e.page_ids.len())
    }
}
