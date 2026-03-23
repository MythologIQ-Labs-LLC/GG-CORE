//! Tests for LRU prompt cache.

use gg_core::memory::prompt_cache::PromptCache;

#[test]
fn cache_insert_and_get() {
    let mut cache = PromptCache::new(10);
    let tokens = [1u32, 2, 3, 4, 5];
    let kv_data = vec![0u8; 128];

    cache.insert(&tokens, kv_data.clone(), 5);
    assert_eq!(cache.len(), 1);

    let entry = cache.get(&tokens).expect("Should find entry");
    assert_eq!(entry.seq_len(), 5);
    assert_eq!(entry.kv_data().len(), 128);
}

#[test]
fn cache_miss_returns_none() {
    let mut cache = PromptCache::new(10);
    let tokens = [1u32, 2, 3];
    let other_tokens = [4u32, 5, 6];

    cache.insert(&tokens, vec![0; 64], 3);

    assert!(cache.get(&other_tokens).is_none());
}

#[test]
fn cache_hash_different_for_different_tokens() {
    let hash1 = PromptCache::hash_tokens(&[1, 2, 3]);
    let hash2 = PromptCache::hash_tokens(&[1, 2, 4]);
    let hash3 = PromptCache::hash_tokens(&[1, 2, 3, 4]);

    assert_ne!(hash1, hash2);
    assert_ne!(hash1, hash3);
    assert_ne!(hash2, hash3);
}

#[test]
fn cache_hash_same_for_same_tokens() {
    let hash1 = PromptCache::hash_tokens(&[1, 2, 3, 4, 5]);
    let hash2 = PromptCache::hash_tokens(&[1, 2, 3, 4, 5]);

    assert_eq!(hash1, hash2);
}

#[test]
fn cache_lru_eviction() {
    let mut cache = PromptCache::new(2);

    cache.insert(&[1], vec![1], 1);
    cache.insert(&[2], vec![2], 1);
    assert_eq!(cache.len(), 2);

    cache.get(&[1]); // Touch entry 1

    cache.insert(&[3], vec![3], 1);
    assert_eq!(cache.len(), 2);

    assert!(cache.get(&[1]).is_some(), "Entry 1 should survive (recently used)");
    assert!(cache.get(&[2]).is_none(), "Entry 2 should be evicted (LRU)");
    assert!(cache.get(&[3]).is_some(), "Entry 3 should exist");
}

#[test]
fn cache_find_prefix_exact() {
    let mut cache = PromptCache::new(10);
    cache.insert(&[1, 2, 3], vec![0; 64], 3);

    let result = cache.find_prefix(&[1, 2, 3]);
    assert!(result.is_some());
    let (len, _entry) = result.unwrap();
    assert_eq!(len, 3);
}

#[test]
fn cache_find_prefix_partial() {
    let mut cache = PromptCache::new(10);
    cache.insert(&[1, 2], vec![0; 64], 2);

    let result = cache.find_prefix(&[1, 2, 3, 4, 5]);
    assert!(result.is_some());
    let (len, entry) = result.unwrap();
    assert_eq!(len, 2);
    assert_eq!(entry.seq_len(), 2);
}

#[test]
fn cache_find_prefix_longest_match() {
    let mut cache = PromptCache::new(10);
    cache.insert(&[1], vec![0; 32], 1);
    cache.insert(&[1, 2], vec![0; 64], 2);
    cache.insert(&[1, 2, 3], vec![0; 96], 3);

    let result = cache.find_prefix(&[1, 2, 3, 4, 5]);
    assert!(result.is_some());
    let (len, entry) = result.unwrap();
    assert_eq!(len, 3);
    assert_eq!(entry.seq_len(), 3);
}

#[test]
fn cache_find_prefix_no_match() {
    let mut cache = PromptCache::new(10);
    cache.insert(&[1, 2, 3], vec![0; 64], 3);

    let result = cache.find_prefix(&[4, 5, 6]);
    assert!(result.is_none());
}

#[test]
fn cache_clear_removes_all() {
    let mut cache = PromptCache::new(10);
    cache.insert(&[1], vec![0; 32], 1);
    cache.insert(&[2], vec![0; 32], 1);
    cache.insert(&[3], vec![0; 32], 1);

    assert_eq!(cache.len(), 3);
    cache.clear();
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn cache_memory_bytes_tracking() {
    let mut cache = PromptCache::new(10);
    cache.insert(&[1], vec![0; 100], 1);
    cache.insert(&[2], vec![0; 200], 1);

    assert_eq!(cache.memory_bytes(), 300);
}
