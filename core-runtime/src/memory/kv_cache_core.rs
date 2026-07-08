//! KV Cache Manager struct definition and write operations.
//!
//! # Panic Safety
//! This module uses poison-recovering lock guards to maintain cache availability
//! even if a thread panics while holding a lock.
//!
//! # Lock Order
//! When both locks are held simultaneously, always acquire in the order:
//! sequences → page_table. Never hold page_table while acquiring sequences.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use super::kv_cache_config::{
    read_or_recover, write_or_recover, KvCacheConfig, KvCacheError, KvCacheStats, SequenceId,
};
#[cfg(feature = "advanced")]
use super::kv_quant::Q8KvStore;
use super::paged::{PageId, PageTable, PAGE_TOKENS};

/// Entry tracking for a cached sequence.
#[derive(Debug)]
pub(super) struct SequenceEntry {
    #[allow(dead_code)]
    pub(super) id: SequenceId,
    pub(super) page_ids: Vec<PageId>,
    pub(super) seq_len: usize,
    pub(super) last_access: Instant,
    pub(super) access_count: u64,
    #[cfg(feature = "advanced")]
    pub(super) quant_store: Option<Q8KvStore>,
}

/// Sequences with integrated LRU access order (single lock).
pub(super) struct SequenceStore {
    pub(super) entries: HashMap<SequenceId, SequenceEntry>,
    pub(super) access_order: VecDeque<SequenceId>,
}

impl SequenceStore {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            access_order: VecDeque::new(),
        }
    }

    /// Move a sequence to the back of the LRU order (most recent).
    pub(super) fn touch(&mut self, seq_id: SequenceId) {
        self.access_order.retain(|&id| id != seq_id);
        self.access_order.push_back(seq_id);
    }
}

/// Integrated KV Cache Manager.
///
/// Combines paged attention with optional Q8 quantization for
/// efficient memory management during inference.
pub struct KvCacheManager {
    pub(super) config: KvCacheConfig,
    pub(super) page_table: RwLock<PageTable>,
    pub(super) sequences: RwLock<SequenceStore>,
    pub(super) stats: Arc<KvCacheStats>,
    pub(super) next_seq_id: AtomicU64,
}

impl KvCacheManager {
    /// Create a new KV Cache Manager.
    pub fn new(config: KvCacheConfig) -> Self {
        let page_table = RwLock::new(PageTable::new(config.hidden_dim, config.max_pages));
        Self {
            config,
            page_table,
            sequences: RwLock::new(SequenceStore::new()),
            stats: Arc::new(KvCacheStats::default()),
            next_seq_id: AtomicU64::new(1),
        }
    }

    /// Allocate a new sequence in the cache.
    pub fn allocate_sequence(&self) -> SequenceId {
        let id = SequenceId(self.next_seq_id.fetch_add(1, Ordering::SeqCst));
        #[cfg(feature = "advanced")]
        let quant_store = if self.config.enable_quantization {
            Some(Q8KvStore::new(
                self.config.hidden_dim,
                self.config.max_seq_len,
            ))
        } else {
            None
        };
        let entry = SequenceEntry {
            id,
            page_ids: Vec::new(),
            seq_len: 0,
            last_access: Instant::now(),
            access_count: 0,
            #[cfg(feature = "advanced")]
            quant_store,
        };
        let mut store = write_or_recover(&self.sequences);
        store.entries.insert(id, entry);
        store.access_order.push_back(id);
        id
    }

    /// Append KV pairs to a sequence.
    pub fn append_kv(
        &self,
        seq_id: SequenceId,
        keys: &[f32],
        values: &[f32],
    ) -> Result<(), KvCacheError> {
        // Phase 1: read seq_pos, update LRU, check if page needed.
        let (seq_pos, needs_page) = {
            let mut store = write_or_recover(&self.sequences);
            store.touch(seq_id);
            let entry = store
                .entries
                .get_mut(&seq_id)
                .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;
            entry.last_access = Instant::now();
            entry.access_count += 1;
            let pos = entry.seq_len;
            let need = pos % PAGE_TOKENS == 0 || entry.page_ids.is_empty();
            (pos, need)
        };
        // Phase 2: allocate page (lock-order: page_table acquired then dropped, sequences after).
        if needs_page {
            self.allocate_page_for_seq(seq_id)?;
        }
        // Phase 3: resolve page_id from per-sequence page_ids, then write.
        // Use last() — evict_beyond_window drains from the front, so the
        // absolute index (seq_pos / PAGE_TOKENS) is no longer valid after eviction.
        let page_id = {
            let store = read_or_recover(&self.sequences);
            let entry = store
                .entries
                .get(&seq_id)
                .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;
            *entry.page_ids.last().ok_or(KvCacheError::PageNotFound)?
        };
        self.write_to_page(page_id, seq_pos % PAGE_TOKENS, keys, values);
        let mut store = write_or_recover(&self.sequences);
        let entry = store
            .entries
            .get_mut(&seq_id)
            .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;
        #[cfg(feature = "advanced")]
        Self::write_to_quant_store(entry, keys, values);
        entry.seq_len += 1;
        Ok(())
    }

    fn allocate_page_for_seq(&self, seq_id: SequenceId) -> Result<(), KvCacheError> {
        // Acquire page_table, allocate, then DROP before acquiring sequences.
        // This enforces sequences→page_table lock order (never hold both simultaneously).
        let page_id = {
            let mut page_table = write_or_recover(&self.page_table);
            match page_table.allocate_page() {
                Some(id) => id,
                None => {
                    drop(page_table);
                    self.evict_lru()?;
                    write_or_recover(&self.page_table)
                        .allocate_page()
                        .ok_or(KvCacheError::MemoryExhausted)?
                }
            }
        };
        let mut store = write_or_recover(&self.sequences);
        if let Some(entry) = store.entries.get_mut(&seq_id) {
            entry.page_ids.push(page_id);
        }
        Ok(())
    }

    fn write_to_page(&self, page_id: PageId, slot: usize, keys: &[f32], values: &[f32]) {
        let mut page_table = write_or_recover(&self.page_table);
        if let Some(page) = page_table.page_mut(page_id) {
            page.write(slot, keys, values);
        }
    }

    #[cfg(feature = "advanced")]
    fn write_to_quant_store(entry: &mut SequenceEntry, keys: &[f32], values: &[f32]) {
        if let Some(ref mut qs) = entry.quant_store {
            if !qs.append(keys, values) {
                qs.reset();
                qs.append(keys, values);
            }
        }
    }

    /// Free a sequence and its pages.
    pub fn free_sequence(&self, seq_id: SequenceId) -> Result<(), KvCacheError> {
        let mut store = write_or_recover(&self.sequences);
        let entry = store
            .entries
            .remove(&seq_id)
            .ok_or(KvCacheError::SequenceNotFound(seq_id.0))?;
        store.access_order.retain(|&id| id != seq_id);
        drop(store);
        let mut page_table = write_or_recover(&self.page_table);
        page_table.free(&entry.page_ids);
        Ok(())
    }

    pub(super) fn evict_lru(&self) -> Result<(), KvCacheError> {
        let victim_id = write_or_recover(&self.sequences).access_order.pop_front();
        if let Some(id) = victim_id {
            self.free_sequence(id)?;
        }
        Ok(())
    }

    pub(super) fn dot_product(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Reset all cache state.
    pub fn reset(&self) {
        let mut store = write_or_recover(&self.sequences);
        store.entries.clear();
        store.access_order.clear();
    }
}
