//! Unit tests for the deterministic prefill schedule. Expected values come
//! from the same integer formula, as printed by
//! `scripts/qualification/n7_prefill_schedule_model.py --integer`.

use super::*;

const REF: u64 = 485_452_288;

const PHI: u64 = 2_392_493_952;
const SMOLLM2: u64 = 1_053_827_072;

/// The bulk rule alone: a chunk with unlimited tokens remaining.
fn bulk(pos: usize, threads: u32, model_bytes: u64, cap: usize) -> usize {
    chunk(pos, usize::MAX, threads, model_bytes, cap)
}

/// Chunk lengths the schedule produces for an `n`-token prompt.
fn sequence(n: usize, threads: u32, model_bytes: u64, cap: usize) -> Vec<usize> {
    let (mut pos, mut out) = (0, Vec::new());
    while pos < n {
        let len = chunk(pos, n - pos, threads, model_bytes, cap);
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
            assert_eq!(bulk(pos, threads, REF, 512), size, "t={threads} p={pos}");
            if pos > 0 {
                assert_ne!(
                    bulk(pos - 1, threads, REF, 512),
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
                .map(|f| bulk(pos, threads, (REF as f64 * f) as u64, 512))
                .collect();
            assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
            assert!(sizes.iter().all(|&s| s >= MIN_CHUNK && s != 32 && s != 16));
        }
    }
    assert_eq!(bulk(0, 4, REF / 2, 512), 512);
    assert_eq!(bulk(0, 4, REF * 3, 512), 64);
    assert_eq!(bulk(0, 8, REF * 3, 512), 128);
}

#[test]
fn small_caps_bound_the_chunk() {
    for (cap, want) in [(16, 16), (48, 48), (64, 64), (100, 64), (512, 256)] {
        assert_eq!(bulk(0, 8, REF, cap), want, "cap {cap}");
        assert!(bulk(13_926, 8, REF, cap) <= cap);
    }
    assert_eq!(bulk(0, 8, REF, 0), 1, "a zero cap is clamped to one token");
    assert_eq!(
        bulk(0, 8, REF / 4, 4096),
        512,
        "never above MAX_PREFILL_CHUNK"
    );
}

#[test]
fn extreme_inputs_saturate_without_panicking() {
    assert_eq!(bulk(usize::MAX, 4, REF, 512), MIN_CHUNK);
    assert_eq!(bulk(0, 4, u64::MAX, 512), MIN_CHUNK);
    assert_eq!(bulk(usize::MAX, u32::MAX, u64::MAX, usize::MAX), MIN_CHUNK);
    assert_eq!(bulk(0, 4, 0, 512), 512, "zero bytes predicts zero time");
}

#[test]
fn sequences_are_fixed_per_configuration() {
    assert_eq!(sequence(1, 4, REF, 512), [1]);
    assert_eq!(sequence(63, 1, REF, 512), [21, 21, 21]);
    assert_eq!(sequence(100, 1, REF, 512), [64, 18, 18]);
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
        let got = positions.map(|p| bulk(p, threads, REF, 512));
        assert_eq!(got, want, "threads {threads}");
    }
}

/// The tail of a sequence after its first `skip` chunks.
fn tail(n: usize, threads: u32, bytes: u64, skip: usize) -> Vec<usize> {
    sequence(n, threads, bytes, 512)[skip..].to_vec()
}

#[test]
fn sub_64_remainders_split_into_equal_pieces() {
    assert_eq!(tail(568, 1, REF, 8), [19, 19, 18]);
    assert_eq!(tail(568, 2, REF, 4), [28, 28]);
    assert_eq!(sequence(63, 2, REF, 512), [32, 31]);
    for threads in [4, 8] {
        assert_eq!(tail(568, threads, REF, 2), [56], "t={threads}");
        assert_eq!(sequence(63, threads, REF, 512), [63], "t={threads}");
    }
    assert_eq!(tail(2877, 4, PHI, 44), [16, 15, 15, 15]);
    assert_eq!(tail(2877, 1, PHI, 44), [16, 15, 15, 15]);
    assert_eq!(tail(2877, 4, SMOLLM2, 33), [21, 20, 20]);
}

#[test]
fn packaged_gate_prompt_tail_is_at_the_piece_boundary() {
    // Audit R4-2 condition C1: at 1 thread the 2516-token prompt's final 20
    // tokens sit exactly at `piece_limit` (quotient 26.86, floored to 26).
    // Any change to TABLE, the budget or the piece bound splits this tail.
    assert_eq!(piece_limit(bucket(1), 2496, REF), 20);
    assert_eq!(sequence(2516, 1, REF, 512).last(), Some(&20));
}

#[test]
fn piece_limit_saturates_and_respects_caps() {
    assert_eq!(piece_limit(bucket(4), 0, u64::MAX), 18);
    assert_eq!(piece_limit(bucket(4), usize::MAX, REF), 18);
    assert_eq!(
        piece_limit(bucket(4), 0, 0),
        usize::MAX,
        "zero bytes: unbounded"
    );
    assert_eq!(chunk(0, 61, 4, u64::MAX, 512), 16);
    assert_eq!(chunk(0, 40, 8, REF, 16), 16, "cap below 64 bounds the bulk");
    assert_eq!(chunk(0, 10, 8, REF, 16), 10);
    assert_eq!(
        chunk(0, 5, 8, REF, 0),
        1,
        "zero cap is clamped to one token"
    );
}

#[test]
fn every_length_is_within_remaining_cap_and_piece_limit() {
    for bytes in [0, REF, PHI, u64::MAX] {
        for threads in [1, 2, 4, 8] {
            for pos in [0, 2496, 6963, 16_383] {
                for cap in [1, 7, 16, 63, 64, 100, 512] {
                    for remaining in 1..=600 {
                        let len = chunk(pos, remaining, threads, bytes, cap);
                        assert!((1..=remaining.min(cap)).contains(&len));
                        if remaining < MIN_CHUNK.min(cap) {
                            let limit = piece_limit(bucket(threads), pos, bytes);
                            assert!(len <= limit.max(1), "p={pos} r={remaining}");
                        } else {
                            assert_eq!(len, bulk(pos, threads, bytes, cap).min(remaining));
                        }
                    }
                }
            }
        }
    }
}
