//! SIMD-accelerated dot product kernels for quantized matmul.
//!
//! Provides AVX2 (x86_64) and NEON (aarch64) implementations with runtime dispatch.
//! Falls back to scalar on unsupported platforms.
//! Note: AVX-512 deferred to future nightly feature gate stabilization.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_arch = "aarch64")]
use super::simd_neon;

static AVX2_AVAILABLE: AtomicBool = AtomicBool::new(false);
static SIMD_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize SIMD detection (call once at startup).
#[cfg(target_arch = "x86_64")]
pub fn init_simd() {
    let has_avx2 = is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma");
    AVX2_AVAILABLE.store(has_avx2, Ordering::Relaxed);
    SIMD_INITIALIZED.store(true, Ordering::Relaxed);
}

#[cfg(target_arch = "aarch64")]
pub fn init_simd() {
    // NEON is baseline on aarch64 - always available
    SIMD_INITIALIZED.store(true, Ordering::Relaxed);
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn init_simd() {
    SIMD_INITIALIZED.store(true, Ordering::Relaxed);
}

fn ensure_initialized() {
    if !SIMD_INITIALIZED.load(Ordering::Relaxed) {
        init_simd();
    }
}

/// Dispatch Q8 dot product to best available kernel.
pub fn dot_q8(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    ensure_initialized();

    #[cfg(target_arch = "x86_64")]
    if AVX2_AVAILABLE.load(Ordering::Relaxed) {
        return unsafe { dot_q8_avx2(q_data, input, scale) };
    }

    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { simd_neon::dot_q8_neon(q_data, input, scale) };
    }

    #[allow(unreachable_code)]
    dot_q8_scalar(q_data, input, scale)
}

/// Dispatch Q4 dot product to best available kernel.
pub fn dot_q4(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    ensure_initialized();

    #[cfg(target_arch = "x86_64")]
    if AVX2_AVAILABLE.load(Ordering::Relaxed) {
        return unsafe { dot_q4_avx2(q_data, input, scale) };
    }

    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { simd_neon::dot_q4_neon(q_data, input, scale) };
    }

    #[allow(unreachable_code)]
    dot_q4_scalar(q_data, input, scale)
}

// ============================================================================
// AVX2 Kernels (x86_64)
// ============================================================================

/// # Safety
/// Caller must verify AVX2+FMA support. Slices must have compatible lengths.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_q8_avx2(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    let len = q_data.len().min(input.len());
    let chunks = len / 8;
    let mut acc = _mm256_setzero_ps();

    for i in 0..chunks {
        let offset = i * 8;
        let q_i32 = load_q8_to_i32_avx2(&q_data[offset..offset + 8]);
        let q_vec = _mm256_cvtepi32_ps(q_i32);
        let in_vec = _mm256_loadu_ps(input[offset..].as_ptr());
        acc = _mm256_fmadd_ps(q_vec, in_vec, acc);
    }

    let sum = horizontal_sum_avx2(acc);
    let remainder: f32 = (chunks * 8..len)
        .map(|i| (q_data[i] as i8 as f32) * input[i])
        .sum();
    (sum + remainder) * scale
}

/// # Safety
/// Caller must verify AVX2+FMA support. Slices must have compatible lengths.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_q4_avx2(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    let num_pairs = q_data.len();
    let chunks = num_pairs / 4;
    let mut acc = _mm256_setzero_ps();

    for i in 0..chunks {
        let offset = i * 4;
        let unpacked = unpack_q4_to_i32(&q_data[offset..offset + 4]);
        let q_vec = _mm256_cvtepi32_ps(_mm256_loadu_si256(unpacked.as_ptr() as *const __m256i));
        let in_offset = i * 8;
        let in_vec = _mm256_loadu_ps(input[in_offset..].as_ptr());
        acc = _mm256_fmadd_ps(q_vec, in_vec, acc);
    }

    let sum = horizontal_sum_avx2(acc);
    let remainder = scalar_q4_remainder(q_data, input, chunks * 4, chunks * 8);
    (sum + remainder) * scale
}

/// # Safety
/// Caller must verify AVX2 support. `data` must have at least 8 elements.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn load_q8_to_i32_avx2(data: &[u8]) -> __m256i {
    let q_i32: [i32; 8] = std::array::from_fn(|j| data[j] as i8 as i32);
    _mm256_loadu_si256(q_i32.as_ptr() as *const __m256i)
}

/// # Safety
/// Caller must verify AVX2 support.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn horizontal_sum_avx2(v: __m256) -> f32 {
    let hi = _mm256_extractf128_ps(v, 1);
    let lo = _mm256_castps256_ps128(v);
    let sum128 = _mm_add_ps(lo, hi);
    let hi64 = _mm_movehl_ps(sum128, sum128);
    let sum64 = _mm_add_ps(sum128, hi64);
    let hi32 = _mm_shuffle_ps(sum64, sum64, 1);
    _mm_cvtss_f32(_mm_add_ss(sum64, hi32))
}

// ============================================================================
// Scalar Fallbacks
// ============================================================================

fn dot_q8_scalar(q_data: &[u8], input: &[f32], scale: f32) -> f32 {
    q_data
        .iter()
        .zip(input.iter())
        .map(|(&q, &x)| (q as i8 as f32) * x)
        .sum::<f32>()
        * scale
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

// ============================================================================
// Helper Functions
// ============================================================================

fn unpack_q4_to_i32(packed: &[u8]) -> [i32; 8] {
    let mut out = [0i32; 8];
    for (i, &byte) in packed.iter().take(4).enumerate() {
        out[i * 2] = (byte & 0x0F) as i32 - 8;
        out[i * 2 + 1] = (byte >> 4) as i32 - 8;
    }
    out
}

pub fn scalar_q4_remainder(
    q_data: &[u8],
    input: &[f32],
    byte_start: usize,
    val_start: usize,
) -> f32 {
    let mut sum = 0.0f32;
    for (i, &byte) in q_data.iter().skip(byte_start).enumerate() {
        let idx0 = val_start + i * 2;
        let idx1 = idx0 + 1;
        if idx0 < input.len() {
            sum += ((byte & 0x0F) as i8 - 8) as f32 * input[idx0];
        }
        if idx1 < input.len() {
            sum += ((byte >> 4) as i8 - 8) as f32 * input[idx1];
        }
    }
    sum
}
