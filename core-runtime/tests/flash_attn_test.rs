//! Tests for CPU Flash Attention implementation.
//!
//! Verifies tiled attention produces correct results with reduced memory.

use gg_core::engine::{FlashAttn, FlashAttnConfig};

#[test]
fn flash_attn_single_position() {
    let config = FlashAttnConfig {
        block_size: 64,
        head_dim: 4,
    };
    let attn = FlashAttn::new(config);

    let query = vec![1.0f32, 0.0, 0.0, 0.0];
    let keys = vec![1.0f32, 0.0, 0.0, 0.0]; // Single key matching query
    let values = vec![1.0f32, 2.0, 3.0, 4.0];
    let mut output = vec![0.0f32; 4];

    attn.forward(&query, &keys, &values, 1, &mut output);

    // With single position, output should equal values (softmax = 1.0)
    for (i, &v) in values.iter().enumerate() {
        assert!((output[i] - v).abs() < 0.01, "Position {i}: expected {v}, got {}", output[i]);
    }
}

#[test]
fn flash_attn_two_positions_equal_scores() {
    let config = FlashAttnConfig {
        block_size: 64,
        head_dim: 4,
    };
    let attn = FlashAttn::new(config);

    let query = vec![1.0f32, 0.0, 0.0, 0.0];
    // Two identical keys -> equal attention weights
    let keys = vec![
        1.0, 0.0, 0.0, 0.0,
        1.0, 0.0, 0.0, 0.0,
    ];
    let values = vec![
        1.0, 2.0, 3.0, 4.0,
        5.0, 6.0, 7.0, 8.0,
    ];
    let mut output = vec![0.0f32; 4];

    attn.forward(&query, &keys, &values, 2, &mut output);

    // Equal attention -> output should be average: [3, 4, 5, 6]
    let expected = [3.0f32, 4.0, 5.0, 6.0];
    for (i, &e) in expected.iter().enumerate() {
        assert!((output[i] - e).abs() < 0.01, "Position {i}: expected {e}, got {}", output[i]);
    }
}

#[test]
fn flash_attn_weighted_attention() {
    let config = FlashAttnConfig {
        block_size: 64,
        head_dim: 4,
    };
    let attn = FlashAttn::new(config);

    // Query strongly matches first key
    let query = vec![1.0f32, 0.0, 0.0, 0.0];
    let keys = vec![
        1.0, 0.0, 0.0, 0.0,  // High score
        0.0, 1.0, 0.0, 0.0,  // Low score
    ];
    let values = vec![
        10.0, 10.0, 10.0, 10.0,
        0.0, 0.0, 0.0, 0.0,
    ];
    let mut output = vec![0.0f32; 4];

    attn.forward(&query, &keys, &values, 2, &mut output);

    // First key has higher attention, output should be closer to [10, 10, 10, 10]
    for &v in &output {
        assert!(v > 5.0, "Expected output > 5.0, got {v}");
    }
}

#[test]
fn flash_attn_tiled_matches_single_block() {
    // Test that tiled computation matches when seq_len > block_size
    let config = FlashAttnConfig {
        block_size: 4,  // Small block to force multiple tiles
        head_dim: 4,
    };
    let attn = FlashAttn::new(config);

    let query = vec![1.0f32, 0.0, 0.0, 0.0];
    // 8 positions (2 tiles with block_size=4)
    let keys: Vec<f32> = (0..8).flat_map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect();
    let values: Vec<f32> = (0..8).flat_map(|i| vec![i as f32; 4]).collect();
    let mut output = vec![0.0f32; 4];

    attn.forward(&query, &keys, &values, 8, &mut output);

    // All equal attention -> average of values [0, 1, 2, 3, 4, 5, 6, 7] = 3.5
    for &v in &output {
        assert!((v - 3.5).abs() < 0.5, "Expected ~3.5, got {v}");
    }
}

#[test]
fn flash_attn_numerical_stability() {
    // Test with large magnitude differences
    let config = FlashAttnConfig {
        block_size: 64,
        head_dim: 4,
    };
    let attn = FlashAttn::new(config);

    let query = vec![10.0f32, 0.0, 0.0, 0.0]; // Large magnitude
    let keys = vec![
        10.0, 0.0, 0.0, 0.0,  // Very high score (100)
        0.0, 0.0, 0.0, 0.0,   // Low score (0)
    ];
    let values = vec![
        1.0, 1.0, 1.0, 1.0,
        0.0, 0.0, 0.0, 0.0,
    ];
    let mut output = vec![0.0f32; 4];

    attn.forward(&query, &keys, &values, 2, &mut output);

    // Should not overflow/underflow - output should be finite
    for &v in &output {
        assert!(v.is_finite(), "Output should be finite, got {v}");
    }
}

#[test]
fn flash_attn_empty_handling() {
    let config = FlashAttnConfig {
        block_size: 64,
        head_dim: 4,
    };
    let attn = FlashAttn::new(config);

    let query = vec![1.0f32; 4];
    let keys: Vec<f32> = vec![];
    let values: Vec<f32> = vec![];
    let mut output = vec![0.0f32; 4];

    // Should not panic with empty input
    attn.forward(&query, &keys, &values, 0, &mut output);

    // Output should remain zeroed
    for &v in &output {
        assert_eq!(v, 0.0, "Output should be zero for empty input");
    }
}

#[test]
fn flash_attn_config_defaults() {
    let config = FlashAttnConfig::default();
    assert_eq!(config.block_size, 64);
    assert_eq!(config.head_dim, 64);
}
