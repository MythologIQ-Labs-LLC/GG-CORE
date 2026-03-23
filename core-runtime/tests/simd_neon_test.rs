//! Tests for ARM NEON SIMD kernels.
//!
//! These tests verify NEON produces same results as scalar fallback.
//! Note: Full NEON path only exercised on aarch64 hardware.

use gg_core::engine::{dot_q4, dot_q8, init_simd};

#[test]
fn neon_init_simd_succeeds() {
    // Should not panic on any platform
    init_simd();
}

#[test]
fn dot_q8_matches_expected() {
    init_simd();

    // Simple test: [1, 2, 3, 4] as i8 * [1.0, 1.0, 1.0, 1.0] = 10.0
    let q_data: Vec<u8> = vec![1u8, 2, 3, 4, 0, 0, 0, 0];
    let input = vec![1.0f32; 8];
    let scale = 1.0;

    let result = dot_q8(&q_data, &input, scale);
    assert!((result - 10.0).abs() < 0.01, "Expected 10.0, got {result}");
}

#[test]
fn dot_q8_with_negative_values() {
    init_simd();

    // Test with signed values: [-1, -2, 1, 2] as i8
    let q_data: Vec<u8> = vec![255u8, 254, 1, 2, 0, 0, 0, 0]; // -1, -2, 1, 2 in i8
    let input = vec![1.0f32; 8];
    let scale = 1.0;

    let result = dot_q8(&q_data, &input, scale);
    // -1 + -2 + 1 + 2 = 0
    assert!((result - 0.0).abs() < 0.01, "Expected 0.0, got {result}");
}

#[test]
fn dot_q8_with_scale() {
    init_simd();

    let q_data: Vec<u8> = vec![10u8, 20, 30, 40, 0, 0, 0, 0];
    let input = vec![1.0f32; 8];
    let scale = 0.5;

    let result = dot_q8(&q_data, &input, scale);
    // (10 + 20 + 30 + 40) * 0.5 = 50.0
    assert!((result - 50.0).abs() < 0.01, "Expected 50.0, got {result}");
}

#[test]
fn dot_q4_matches_expected() {
    init_simd();

    // Q4 packs two 4-bit values per byte: low nibble, high nibble
    // Value 8 is zero point, so (8, 8) encodes (0, 0)
    // (9, 9) encodes (1, 1), (7, 7) encodes (-1, -1)
    let q_data: Vec<u8> = vec![
        0x99, // (9, 9) -> (1, 1)
        0x99, // (9, 9) -> (1, 1)
        0x88, // (8, 8) -> (0, 0)
        0x88, // (8, 8) -> (0, 0)
    ];
    let input = vec![1.0f32; 8];
    let scale = 1.0;

    let result = dot_q4(&q_data, &input, scale);
    // 1 + 1 + 1 + 1 + 0 + 0 + 0 + 0 = 4
    assert!((result - 4.0).abs() < 0.01, "Expected 4.0, got {result}");
}

#[test]
fn dot_q4_with_negative_values() {
    init_simd();

    // 0x77 = (7, 7) -> (-1, -1)
    // 0x99 = (9, 9) -> (1, 1)
    let q_data: Vec<u8> = vec![0x77, 0x99, 0x88, 0x88];
    let input = vec![1.0f32; 8];
    let scale = 1.0;

    let result = dot_q4(&q_data, &input, scale);
    // -1 + -1 + 1 + 1 + 0 + 0 + 0 + 0 = 0
    assert!((result - 0.0).abs() < 0.01, "Expected 0.0, got {result}");
}

#[test]
fn dot_q8_large_vector() {
    init_simd();

    // Test with 256 elements to exercise multiple SIMD iterations
    let q_data: Vec<u8> = (0..256).map(|i| (i % 128) as u8).collect();
    let input: Vec<f32> = (0..256).map(|i| (i as f32) * 0.01).collect();
    let scale = 1.0;

    let result = dot_q8(&q_data, &input, scale);

    // Compute expected value
    let expected: f32 = q_data.iter().zip(input.iter())
        .map(|(&q, &x)| (q as i8 as f32) * x)
        .sum::<f32>() * scale;

    assert!((result - expected).abs() < 0.1, "Expected {expected}, got {result}");
}

#[test]
fn dot_q4_large_vector() {
    init_simd();

    // Test with 128 bytes (256 values) to exercise multiple iterations
    // Pattern: (i%16, i%16) per byte, cycling 0-15
    // After Q4 decode: each nibble becomes (nibble - 8), range -8 to +7
    // Per 16-byte cycle: sum = 2 * (-8 + -7 + ... + 7) = 2 * -8 = -16
    // 128 bytes = 8 cycles, total = 8 * -16 = -128
    let q_data: Vec<u8> = (0..128).map(|i| ((i % 16) | ((i % 16) << 4)) as u8).collect();
    let input: Vec<f32> = (0..256).map(|_| 1.0).collect();
    let scale = 1.0;

    let result = dot_q4(&q_data, &input, scale);

    // Expected: -128 (verified mathematically)
    assert!((result - (-128.0)).abs() < 1.0, "Expected -128, got {result}");
}
