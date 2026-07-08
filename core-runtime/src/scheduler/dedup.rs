//! Request deduplication via output caching.
//!
//! Caches outputs for identical prompts within a TTL window
//! to avoid redundant inference computation.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::engine::InferenceParams;

/// Cached output for a completed request.
#[derive(Debug, Clone)]
pub struct CachedOutput {
    pub output_tokens: Vec<u32>,
    pub cached_at: Instant,
}

/// Configuration for output cache.
#[derive(Debug, Clone)]
pub struct OutputCacheConfig {
    pub ttl: Duration,
    pub max_entries: usize,
}

impl Default for OutputCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(30),
            max_entries: 1000,
        }
    }
}

/// Output cache for request deduplication.
pub struct OutputCache {
    entries: HashMap<[u8; 32], CachedOutput>,
    ttl: Duration,
    max_entries: usize,
}

impl OutputCache {
    pub fn new(config: OutputCacheConfig) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: config.ttl,
            max_entries: config.max_entries,
        }
    }

    /// Compute cache key from prompt tokens and params.
    pub fn cache_key(tokens: &[u32], params: &InferenceParams) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for &t in tokens {
            hasher.update(t.to_le_bytes());
        }
        hasher.update(params.max_tokens.to_le_bytes());
        hasher.update(params.temperature.to_le_bytes());
        hasher.update(params.top_p.to_le_bytes());
        hasher.update(params.top_k.to_le_bytes());
        hasher.finalize().into()
    }

    /// Get cached output if within TTL.
    pub fn get(&self, key: &[u8; 32]) -> Option<&CachedOutput> {
        let entry = self.entries.get(key)?;
        if entry.cached_at.elapsed() <= self.ttl {
            Some(entry)
        } else {
            None // Expired
        }
    }

    /// Store output for future dedup.
    pub fn insert(&mut self, key: [u8; 32], output_tokens: Vec<u32>) {
        // Evict oldest if at capacity
        if self.entries.len() >= self.max_entries {
            self.evict_oldest();
        }
        self.entries.insert(
            key,
            CachedOutput {
                output_tokens,
                cached_at: Instant::now(),
            },
        );
    }

    /// Remove expired entries.
    pub fn cleanup(&mut self) {
        self.entries
            .retain(|_, entry| entry.cached_at.elapsed() <= self.ttl);
    }

    /// Number of entries in cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self.find_oldest_key() {
            self.entries.remove(&oldest_key);
        }
    }

    fn find_oldest_key(&self) -> Option<[u8; 32]> {
        self.entries
            .iter()
            .min_by_key(|(_, v)| v.cached_at)
            .map(|(k, _)| *k)
    }
}

/// Result of deduplication check.
#[derive(Debug)]
pub enum DedupResult {
    /// Found cached output for this request.
    Cached(Vec<u32>),
    /// Request was queued (no cache hit).
    Queued { id: u64, position: usize },
}
