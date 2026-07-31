//! LRU prompt cache for repeated prefix reuse.
//!
//! Caches serialized KV data keyed by token sequence hash.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Cached KV entry with LRU tracking.
#[derive(Debug, Clone)]
pub struct CachedKv {
    _token_hash: [u8; 32],
    kv_data: Vec<u8>,
    seq_len: usize,
    last_used: u64,
}

impl CachedKv {
    pub fn kv_data(&self) -> &[u8] {
        &self.kv_data
    }
    pub fn seq_len(&self) -> usize {
        self.seq_len
    }
}

/// LRU prompt cache with hash-based lookup.
#[derive(Debug)]
pub struct PromptCache {
    entries: HashMap<[u8; 32], CachedKv>,
    max_entries: usize,
    access_counter: u64,
}

impl PromptCache {
    /// Create a new prompt cache with given capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries),
            max_entries,
            access_counter: 0,
        }
    }

    /// Hash a token sequence for cache lookup.
    pub fn hash_tokens(tokens: &[u32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for &t in tokens {
            hasher.update(t.to_le_bytes());
        }
        hasher.finalize().into()
    }

    /// Look up cached KV for exact token match.
    pub fn get(&mut self, tokens: &[u32]) -> Option<&CachedKv> {
        let hash = Self::hash_tokens(tokens);
        self.access_counter += 1;
        let counter = self.access_counter;
        if let Some(entry) = self.entries.get_mut(&hash) {
            entry.last_used = counter;
            return Some(entry);
        }
        None
    }

    /// Store computed KV for token sequence.
    pub fn insert(&mut self, tokens: &[u32], kv_data: Vec<u8>, seq_len: usize) {
        if self.entries.len() >= self.max_entries {
            self.evict_lru();
        }

        let hash = Self::hash_tokens(tokens);
        self.access_counter += 1;
        self.entries.insert(
            hash,
            CachedKv {
                _token_hash: hash,
                kv_data,
                seq_len,
                last_used: self.access_counter,
            },
        );
    }

    /// Find longest cached prefix of tokens. Returns (prefix_len, cloned entry).
    ///
    /// O(N): a single forward SHA256 pass produces every prefix hash (clone the
    /// running hasher and finalize the clone at each length — same bytes as
    /// `hash_tokens(&tokens[..len])`), replacing the old O(N²) per-length re-hash.
    /// Scanning short→long, the last prefix that hits the map is the longest match.
    pub fn find_prefix(&mut self, tokens: &[u32]) -> Option<(usize, CachedKv)> {
        let mut hasher = Sha256::new();
        let mut best: Option<(usize, [u8; 32])> = None;
        for (i, &t) in tokens.iter().enumerate() {
            hasher.update(t.to_le_bytes());
            let hash: [u8; 32] = hasher.clone().finalize().into();
            if self.entries.contains_key(&hash) {
                best = Some((i + 1, hash));
            }
        }
        let (len, hash) = best?;
        self.access_counter += 1;
        let counter = self.access_counter;
        let entry = self.entries.get_mut(&hash)?;
        entry.last_used = counter;
        Some((len, entry.clone()))
    }

    /// Evict least recently used entry.
    fn evict_lru(&mut self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| *k);

        if let Some(hash) = oldest {
            self.entries.remove(&hash);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Total memory used by cached entries.
    pub fn memory_bytes(&self) -> usize {
        self.entries.values().map(|e| e.kv_data.len()).sum()
    }
}
