//! Unit tests for the deterministic prefill schedule. Expected values come
//! from the same integer formula, as printed by
//! `scripts/qualification/n7_prefill_schedule_model.py --integer`.

use super::*;

const REF: u64 = 485_452_288;

/// Chunk lengths the schedule produces for an `n`-token prompt.
fn sequence(n: usize, threads: u32, model_bytes: u64, cap: usize) -> Vec<usize> {
    let (mut pos, mut out) = (0, Vec::new());
    while pos < n {
        let len = chunk(pos, threads, model_bytes, cap).min(n - pos);
        out.push(len);
        pos += len;
    }
    out
}

#[test]
fn table_slopes_are_non_negative() {
    for row in TABLE {
        for [t0, t16] in row {
            assert!(t16 >= t0, "T16 {t16} < T0 {t0}");
        }
    }
}

#[test]
fn threads_round_down_to_calibrated_buckets() {
    let rows: Vec<usize> = [0, 1, 2, 3, 4, 7, 8, 16, u32::MAX]
        .iter()
        .map(|&t| bucket(t))
        .collect();
    assert_eq!(rows, [0, 0, 1, 1, 2, 2, 3, 3, 3]);
}

#[test]
fn reference_model_transitions_match_the_integer_model() {
    // (threads, [(first position of a new chunk size, size)])
    let cases: [(u32, &[(usize, usize)]); 4] = [
        (1, &[(0, 64)]),
        (2, &[(0, 128), (3111, 64)]),
        (4, &[(0, 256), (2123, 128), (9497, 64)]),
        (8, &[(0, 256), (5646, 128), (16297, 64)]),
    ];
    for (threads, steps) in cases {
        for &(pos, size) in steps {
            assert_eq!(chunk(pos, threads, REF, 512), size, "t={threads} p={pos}");
            if pos > 0 {
                assert_ne!(
                    chunk(pos - 1, threads, REF, 512),
                    size,
                    "t={threads} p={pos}"
                );
            }
        }
    }
}

#[test]
fn larger_models_never_get_larger_chunks() {
    for threads in [1, 2, 4, 8] {
        for pos in [0, 2048, 4096, 13_926] {
            let sizes: Vec<usize> = [0.5, 1.0, 1.9, 2.0, 3.0, 9.0, 100.0]
                .iter()
                .map(|f| chunk(pos, threads, (REF as f64 * f) as u64, 512))
                .collect();
            assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
            assert!(sizes.iter().all(|&s| s >= MIN_CHUNK && s != 32 && s != 16));
        }
    }
    assert_eq!(chunk(0, 4, REF / 2, 512), 512);
    assert_eq!(chunk(0, 4, REF * 3, 512), 64);
    assert_eq!(chunk(0, 8, REF * 3, 512), 128);
}

#[test]
fn small_caps_bound_the_chunk() {
    for (cap, want) in [(16, 16), (48, 48), (64, 64), (100, 64), (512, 256)] {
        assert_eq!(chunk(0, 8, REF, cap), want, "cap {cap}");
        assert!(chunk(13_926, 8, REF, cap) <= cap);
    }
    assert_eq!(chunk(0, 8, REF, 0), 1, "a zero cap is clamped to one token");
    assert_eq!(
        chunk(0, 8, REF / 4, 4096),
        512,
        "never above MAX_PREFILL_CHUNK"
    );
}

#[test]
fn extreme_inputs_saturate_without_panicking() {
    assert_eq!(chunk(usize::MAX, 4, REF, 512), MIN_CHUNK);
    assert_eq!(chunk(0, 4, u64::MAX, 512), MIN_CHUNK);
    assert_eq!(chunk(usize::MAX, u32::MAX, u64::MAX, usize::MAX), MIN_CHUNK);
    assert_eq!(chunk(0, 4, 0, 512), 512, "zero bytes predicts zero time");
}

#[test]
fn sequences_are_fixed_per_configuration() {
    assert_eq!(sequence(1, 4, REF, 512), [1]);
    assert_eq!(sequence(63, 1, REF, 512), [63]);
    assert_eq!(sequence(100, 1, REF, 512), [64, 36]);
    assert_eq!(sequence(100, 4, REF, 512), [100]);
    let mut b4 = vec![256; 9];
    b4.extend([128, 84]);
    assert_eq!(sequence(2516, 4, REF, 512), b4);
    let mut b8 = vec![256; 9];
    b8.push(212);
    assert_eq!(sequence(2516, 8, REF, 512), b8);
    let b1 = sequence(2516, 1, REF, 512);
    assert_eq!((b1.len(), b1[39]), (40, 20));
    assert!(b1[..39].iter().all(|&s| s == 64));
}

#[test]
fn plan_positions_match_per_bucket() {
    let positions = [0, 2047, 2048, 4095, 4096, 9500, 13_926, 16_383];
    let expected: [(u32, [usize; 8]); 4] = [
        (1, [64, 64, 64, 64, 64, 64, 64, 64]),
        (2, [128, 128, 128, 64, 64, 64, 64, 64]),
        (4, [256, 256, 256, 128, 128, 64, 64, 64]),
        (8, [256, 256, 256, 256, 256, 128, 128, 64]),
    ];
    for (threads, want) in expected {
        let got = positions.map(|p| chunk(p, threads, REF, 512));
        assert_eq!(got, want, "threads {threads}");
    }
}
