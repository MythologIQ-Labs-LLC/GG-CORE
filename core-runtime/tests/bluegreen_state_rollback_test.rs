//! Integration tests for blue-green deployment state synchronization.
//!
//! Tests cover cache warming, session state handling, and KV-cache pre-population.

use std::collections::HashMap;

// === State Synchronization Types ===

#[derive(Debug, Default)]
struct ModelCache {
    entries: HashMap<String, Vec<u8>>,
    warmed: bool,
}

impl ModelCache {
    fn warm(&mut self, entries: &[(&str, &[u8])]) {
        for (key, data) in entries {
            self.entries.insert(key.to_string(), data.to_vec());
        }
        self.warmed = true;
    }

    fn is_warmed(&self) -> bool {
        self.warmed && !self.entries.is_empty()
    }
}

#[derive(Debug, Default)]
struct SessionState {
    sessions: HashMap<String, String>,
}

impl SessionState {
    fn add(&mut self, session_id: &str, data: &str) {
        self.sessions.insert(session_id.to_string(), data.to_string());
    }

    fn transfer_to(&self, target: &mut SessionState) {
        for (k, v) in &self.sessions {
            target.sessions.insert(k.clone(), v.clone());
        }
    }

    fn len(&self) -> usize {
        self.sessions.len()
    }
}

#[derive(Debug, Default)]
struct KVCache {
    entries: HashMap<String, Vec<f32>>,
}

impl KVCache {
    fn prepopulate(&mut self, key: &str, data: Vec<f32>) {
        self.entries.insert(key.to_string(), data);
    }

    fn is_populated(&self) -> bool {
        !self.entries.is_empty()
    }
}

// === Model Cache Warming Tests ===

#[test]
fn bluegreen_model_cache_warming() {
    let mut cache = ModelCache::default();
    assert!(!cache.is_warmed());

    cache.warm(&[("model-v2-weights", b"weights"), ("model-v2-config", b"config")]);

    assert!(cache.is_warmed());
    assert!(cache.entries.contains_key("model-v2-weights"));
}

#[test]
fn bluegreen_cache_multiple_models() {
    let mut cache = ModelCache::default();

    cache.warm(&[
        ("model-a-weights", b"weights_a"),
        ("model-a-vocab", b"vocab_a"),
        ("model-b-weights", b"weights_b"),
    ]);

    assert!(cache.is_warmed());
    assert_eq!(cache.entries.len(), 3);
}

#[test]
fn bluegreen_cache_empty_not_warmed() {
    let mut cache = ModelCache::default();
    cache.warmed = true;
    // Empty cache should not be considered warmed
    assert!(!cache.is_warmed());
}

// === Session State Handling Tests ===

#[test]
fn bluegreen_session_state_handling() {
    let mut blue_state = SessionState::default();
    blue_state.add("session-1", "user-context-1");
    blue_state.add("session-2", "user-context-2");

    let mut green_state = SessionState::default();
    blue_state.transfer_to(&mut green_state);

    assert_eq!(green_state.len(), 2);
    assert_eq!(green_state.sessions.get("session-1").unwrap(), "user-context-1");
}

#[test]
fn bluegreen_session_transfer_preserves_data() {
    let mut source = SessionState::default();
    source.add("sess-abc", "context-data-large");
    source.add("sess-def", "context-data-small");

    let mut target = SessionState::default();
    source.transfer_to(&mut target);

    // Verify all data transferred correctly
    assert_eq!(target.sessions.get("sess-abc"), Some(&"context-data-large".to_string()));
    assert_eq!(target.sessions.get("sess-def"), Some(&"context-data-small".to_string()));

    // Source should still have data
    assert_eq!(source.len(), 2);
}

#[test]
fn bluegreen_session_transfer_to_non_empty() {
    let mut source = SessionState::default();
    source.add("session-1", "data-1");

    let mut target = SessionState::default();
    target.add("session-0", "existing-data");

    source.transfer_to(&mut target);

    // Target should have both sessions
    assert_eq!(target.len(), 2);
    assert!(target.sessions.contains_key("session-0"));
    assert!(target.sessions.contains_key("session-1"));
}

// === KV-Cache Pre-population Tests ===

#[test]
fn bluegreen_kv_cache_prepopulation() {
    let mut kv_cache = KVCache::default();
    assert!(!kv_cache.is_populated());

    kv_cache.prepopulate("prompt-hash-abc", vec![0.1, 0.2, 0.3, 0.4]);
    kv_cache.prepopulate("prompt-hash-def", vec![0.5, 0.6, 0.7, 0.8]);

    assert!(kv_cache.is_populated());
    assert_eq!(kv_cache.entries.len(), 2);
}

#[test]
fn bluegreen_kv_cache_data_integrity() {
    let mut kv_cache = KVCache::default();

    let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    kv_cache.prepopulate("test-key", test_data.clone());

    let retrieved = kv_cache.entries.get("test-key").unwrap();
    assert_eq!(retrieved, &test_data);
}

#[test]
fn bluegreen_kv_cache_overwrite() {
    let mut kv_cache = KVCache::default();

    kv_cache.prepopulate("key-1", vec![1.0, 2.0]);
    kv_cache.prepopulate("key-1", vec![3.0, 4.0, 5.0]);

    // Should have overwritten
    let data = kv_cache.entries.get("key-1").unwrap();
    assert_eq!(data, &vec![3.0, 4.0, 5.0]);
}

#[test]
fn bluegreen_kv_cache_large_vectors() {
    let mut kv_cache = KVCache::default();

    let large_data: Vec<f32> = (0..1000).map(|i| i as f32).collect();
    kv_cache.prepopulate("large-key", large_data.clone());

    let retrieved = kv_cache.entries.get("large-key").unwrap();
    assert_eq!(retrieved.len(), 1000);
    assert_eq!(retrieved[500], 500.0);
}
