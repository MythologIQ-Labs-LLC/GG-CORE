//! NEON SIMD kernels for aarch64.
//!
//! Provides NEON-accelerated Q8/Q4 dot products for ARM platforms.

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

// ============================================================================
// NEON Kernels (aarch64)
// ============================================================================

/// # Safety
/// Must be called on aarch64 where NEON is always available.
/// Slices must have compatible lengths.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn dot_q8_neon(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    let len = q_data.len().min(input.len());
    let chunks = len / 4;
    let mut acc = vdupq_n_f32(0.0);

    for i in 0..chunks {
        let offset = i * 4;
        let q_i32 = load_q8_to_i32_neon(&q_data[offset..offset + 4]);
        let q_vec = vcvtq_f32_s32(q_i32);
        let in_vec = vld1q_f32(input[offset..].as_ptr());
        acc = vfmaq_f32(acc, q_vec, in_vec);
    }

    let sum = horizontal_sum_neon(acc);
    let remainder: f32 = (chunks * 4..len)
        .map(|i| (q_data[i] as i8 as f32) * input[i])
        .sum();
    (sum + remainder) * scale
}

/// # Safety
/// Must be called on aarch64 where NEON is always available.
/// Slices must have compatible lengths.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn dot_q4_neon(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    let num_pairs = q_data.len();
    let chunks = num_pairs / 2;
    let mut acc = vdupq_n_f32(0.0);

    for i in 0..chunks {
        let offset = i * 2;
        let unpacked = unpack_q4_to_i32_neon(&q_data[offset..offset + 2]);
        let q_vec = vcvtq_f32_s32(unpacked);
        let in_offset = i * 4;
        let in_vec = vld1q_f32(input[in_offset..].as_ptr());
        acc = vfmaq_f32(acc, q_vec, in_vec);
    }

    let sum = horizontal_sum_neon(acc);
    let remainder = super::simd_matmul::scalar_q4_remainder(q_data, input, chunks * 2, chunks * 4);
    (sum + remainder) * scale
}

/// # Safety
/// Must be called on aarch64. `data` must have at least 4 elements.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn load_q8_to_i32_neon(data: &[u8]) -> int32x4_t {
    let q_i32: [i32; 4] = std::array::from_fn(|j| data[j] as i8 as i32);
    vld1q_s32(q_i32.as_ptr())
}

/// # Safety
/// Must be called on aarch64. `data` must have at least 2 elements.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn unpack_q4_to_i32_neon(data: &[u8]) -> int32x4_t {
    let mut out = [0i32; 4];
    for (i, &byte) in data.iter().take(2).enumerate() {
        out[i * 2] = (byte & 0x0F) as i32 - 8;
        out[i * 2 + 1] = (byte >> 4) as i32 - 8;
    }
    vld1q_s32(out.as_ptr())
}

/// # Safety
/// Must be called on aarch64 where NEON is always available.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn horizontal_sum_neon(v: float32x4_t) -> f32 {
    let sum2 = vpadd_f32(vget_low_f32(v), vget_high_f32(v));
    let sum1 = vpadd_f32(sum2, sum2);
    vget_lane_f32(sum1, 0)
}
