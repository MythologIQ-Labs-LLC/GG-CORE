//! Tests for SIMD matmul kernels.

use gg_core::engine::simd_matmul::{dot_q4, dot_q8, init_simd};

fn dot_q8_scalar(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    q_data.iter().zip(input.iter())
        .map(|(&q, &x)| (q as i8 as f32) * x)
        .sum::<f32>() * scale
}

fn dot_q4_scalar(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    let mut sum = 0.0f32;
    for (i, &byte) in q_data.iter().enumerate() {
        let idx0 = i * 2;
        let idx1 = idx0 + 1;
        if idx0 < input.len() {
            sum += ((byte & 0x0F) as i8 - 8) as f32 * input[idx0];
        }
        if idx1 < input.len() {
            sum += ((byte >> 4) as i8 - 8) as f32 * input[idx1];
        }
    }
    sum * scale
}

#[test]
fn simd_init_does_not_panic() {
    init_simd();
}

#[test]
fn dot_q8_matches_scalar_small() {
    let q_data: Vec<u8> = (0..8).map(|i| (i as i8 * 10) as u8).collect();
    let input: Vec<f32> = (0..8).map(|i| i as f32 * 0.5).collect();
    let scale = 0.1;

    let expected = dot_q8_scalar(&q_data, &input, scale);
    let actual = dot_q8(&q_data, &input, scale);

    assert!((expected - actual).abs() < 1e-5, "expected {expected}, got {actual}");
}

#[test]
fn dot_q8_matches_scalar_large() {
    let q_data: Vec<u8> = (0..64).map(|i| ((i % 256) as i8) as u8).collect();
    let input: Vec<f32> = (0..64).map(|i| (i as f32 - 32.0) * 0.01).collect();
    let scale = 0.05;

    let expected = dot_q8_scalar(&q_data, &input, scale);
    let actual = dot_q8(&q_data, &input, scale);

    assert!((expected - actual).abs() < 1e-4, "expected {expected}, got {actual}");
}

#[test]
fn dot_q8_unaligned_length() {
    let q_data: Vec<u8> = (0..13).map(|i| (i as i8 * 5) as u8).collect();
    let input: Vec<f32> = (0..13).map(|i| i as f32 * 0.1).collect();
    let scale = 1.0;

    let expected = dot_q8_scalar(&q_data, &input, scale);
    let actual = dot_q8(&q_data, &input, scale);

    assert!((expected - actual).abs() < 1e-5, "expected {expected}, got {actual}");
}

#[test]
fn dot_q8_empty_input() {
    let q_data: Vec<u8> = vec![];
    let input: Vec<f32> = vec![];

    let result = dot_q8(&q_data, &input, 1.0);
    assert_eq!(result, 0.0);
}

#[test]
fn dot_q4_matches_scalar_small() {
    let q_data: Vec<u8> = vec![0x12, 0x34, 0x56, 0x78];
    let input: Vec<f32> = (0..8).map(|i| i as f32 * 0.5).collect();
    let scale = 0.1;

    let expected = dot_q4_scalar(&q_data, &input, scale);
    let actual = dot_q4(&q_data, &input, scale);

    assert!((expected - actual).abs() < 1e-5, "expected {expected}, got {actual}");
}

#[test]
fn dot_q4_matches_scalar_large() {
    let q_data: Vec<u8> = (0..32).map(|i| i as u8).collect();
    let input: Vec<f32> = (0..64).map(|i| (i as f32 - 32.0) * 0.01).collect();
    let scale = 0.05;

    let expected = dot_q4_scalar(&q_data, &input, scale);
    let actual = dot_q4(&q_data, &input, scale);

    assert!((expected - actual).abs() < 1e-4, "expected {expected}, got {actual}");
}

#[test]
fn dot_q4_empty_input() {
    let q_data: Vec<u8> = vec![];
    let input: Vec<f32> = vec![];

    let result = dot_q4(&q_data, &input, 1.0);
    assert_eq!(result, 0.0);
}
