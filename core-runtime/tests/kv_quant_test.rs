//! Tests for Q8 KV-cache storage.

use gg_core::memory::kv_quant::{compute_scale, dequantize, quantize_to, Q8KvStore};

#[test]
fn q8_roundtrip_within_tolerance() {
    let original: Vec<f32> = (0..64).map(|i| (i as f32 - 32.0) * 0.1).collect();
    let scale = compute_scale(&original);

    let mut quantized = vec![0u8; 64];
    quantize_to(&mut quantized, &original, scale);

    let mut recovered = vec![0.0f32; 64];
    dequantize(&quantized, &mut recovered, scale);

    for (i, (&orig, &rec)) in original.iter().zip(recovered.iter()).enumerate() {
        let error = (orig - rec).abs() / orig.abs().max(0.001);
        assert!(error < 0.02, "Position {i}: orig={orig}, rec={rec}, error={error}");
    }
}

#[test]
fn q8_store_append_and_read() {
    let mut store = Q8KvStore::new(4, 16);

    let keys = [1.0, 2.0, 3.0, 4.0];
    let values = [5.0, 6.0, 7.0, 8.0];

    assert!(store.append(&keys, &values));
    assert_eq!(store.seq_len(), 1);

    let mut read_keys = [0.0f32; 4];
    store.read_keys(0, &mut read_keys);

    for (i, (&orig, &read)) in keys.iter().zip(read_keys.iter()).enumerate() {
        let error = (orig - read).abs() / orig.abs().max(0.001);
        assert!(error < 0.02, "Key {i}: orig={orig}, read={read}");
    }
}

#[test]
fn q8_store_attention_scores() {
    let mut store = Q8KvStore::new(4, 16);

    store.append(&[1.0, 0.0, 0.0, 0.0], &[1.0; 4]);
    store.append(&[0.0, 1.0, 0.0, 0.0], &[2.0; 4]);
    store.append(&[0.0, 0.0, 1.0, 0.0], &[3.0; 4]);

    let query = [1.0, 0.0, 0.0, 0.0];
    let mut scores = [0.0f32; 3];
    store.attention_scores(&query, &mut scores);

    assert!(scores[0] > scores[1], "Score 0 should be highest");
    assert!(scores[0] > scores[2], "Score 0 should be highest");
}

#[test]
fn q8_store_weighted_values() {
    let mut store = Q8KvStore::new(4, 16);

    store.append(&[1.0; 4], &[1.0, 2.0, 3.0, 4.0]);
    store.append(&[1.0; 4], &[4.0, 3.0, 2.0, 1.0]);

    let weights = [0.5, 0.5];
    let mut output = [0.0f32; 4];
    store.weighted_values(&weights, &mut output);

    for &v in &output {
        assert!((v - 2.5).abs() < 0.1, "Expected ~2.5, got {v}");
    }
}

#[test]
fn q8_store_memory_is_4x_smaller() {
    let store = Q8KvStore::new(64, 128);
    let q8_bytes = store.memory_bytes();
    let f32_bytes = 64 * 128 * 4 * 2;

    assert_eq!(q8_bytes * 4, f32_bytes, "Q8 should be 4x smaller than f32");
}

#[test]
fn q8_store_reset_clears_seq_len() {
    let mut store = Q8KvStore::new(4, 16);
    store.append(&[1.0; 4], &[1.0; 4]);
    store.append(&[2.0; 4], &[2.0; 4]);
    assert_eq!(store.seq_len(), 2);

    store.reset();
    assert_eq!(store.seq_len(), 0);
}

#[test]
fn q8_store_rejects_overflow() {
    let mut store = Q8KvStore::new(4, 2);
    assert!(store.append(&[1.0; 4], &[1.0; 4]));
    assert!(store.append(&[2.0; 4], &[2.0; 4]));
    assert!(!store.append(&[3.0; 4], &[3.0; 4]));
}
