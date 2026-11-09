//! Fail-closed regressions: zero, missing, malformed or unsupported metadata
//! must never produce a smaller admissible estimate (review on #176).

use super::*;

const UNADMITTABLE: usize = usize::MAX;

fn est(d: &ModelDims) -> usize {
    working_memory_estimate(d, &ContextShape::new(4096))
}

#[test]
fn zero_core_dimensions_are_unadmittable() {
    let base = anchors()[1].dims;
    let cases = [
        ModelDims { n_layer: 0, ..base },
        ModelDims { n_embd: 0, ..base },
        ModelDims { n_head: 0, ..base },
        ModelDims { n_vocab: 0, ..base },
    ];
    for (i, d) in cases.iter().enumerate() {
        assert_eq!(est(d), UNADMITTABLE, "case {i}: {d:?}");
    }
}

#[test]
fn missing_or_zero_ffn_is_unadmittable_not_guessed() {
    let base = anchors()[0].dims;
    assert_eq!(est(&ModelDims { n_ff: None, ..base }), UNADMITTABLE);
    assert_eq!(
        est(&ModelDims {
            n_ff: Some(0),
            ..base
        }),
        UNADMITTABLE
    );
}

#[test]
fn zero_explicit_head_lengths_are_unadmittable() {
    let base = anchors()[2].dims;
    assert_eq!(
        est(&ModelDims {
            head_k: Some(0),
            ..base
        }),
        UNADMITTABLE
    );
    assert_eq!(
        est(&ModelDims {
            head_v: Some(0),
            ..base
        }),
        UNADMITTABLE
    );
}

#[test]
fn indivisible_embedding_without_explicit_heads_is_unadmittable() {
    let d = ModelDims {
        n_embd: 2049,
        ..anchors()[1].dims
    };
    assert_eq!(
        est(&d),
        UNADMITTABLE,
        "derived head length would be truncated"
    );
    let explicit = ModelDims {
        head_k: Some(64),
        head_v: Some(64),
        ..d
    };
    assert_ne!(
        est(&explicit),
        UNADMITTABLE,
        "explicit lengths make it boundable"
    );
}

#[test]
fn unsupported_shapes_are_unadmittable() {
    let d = ModelDims {
        unsupported: true,
        ..anchors()[0].dims
    };
    assert_eq!(est(&d), UNADMITTABLE);
}

#[test]
fn out_of_range_kv_heads_use_the_query_head_upper_bound() {
    let base = anchors()[0].dims; // GQA: 2 KV heads of 14
    let full = est(&ModelDims {
        n_head_kv: base.n_head,
        ..base
    });
    assert_eq!(
        est(&ModelDims {
            n_head_kv: 0,
            ..base
        }),
        full
    );
    assert_eq!(
        est(&ModelDims {
            n_head_kv: base.n_head + 1,
            ..base
        }),
        full
    );
    assert!(est(&base) < full, "valid GQA keeps its smaller cache");
}

#[test]
fn invalid_metadata_never_yields_a_smaller_admissible_estimate() {
    // Any single-field corruption of a valid shape is either unadmittable
    // or at least as large as the valid estimate.
    for a in anchors() {
        let valid = est(&a.dims);
        let d = a.dims;
        let corrupted = [
            ModelDims { n_layer: 0, ..d },
            ModelDims { n_embd: 0, ..d },
            ModelDims { n_head: 0, ..d },
            ModelDims { n_head_kv: 0, ..d },
            ModelDims {
                n_head_kv: d.n_head * 4,
                ..d
            },
            ModelDims { n_vocab: 0, ..d },
            ModelDims { n_ff: None, ..d },
            ModelDims { n_ff: Some(0), ..d },
            ModelDims {
                head_k: Some(0),
                ..d
            },
            ModelDims {
                head_v: Some(0),
                ..d
            },
            ModelDims {
                n_embd: d.n_embd + 1,
                ..d
            },
            ModelDims {
                unsupported: true,
                ..d
            },
        ];
        for (i, c) in corrupted.iter().enumerate() {
            let e = est(c);
            assert!(
                e == UNADMITTABLE || e >= valid,
                "{} case {i}: {e} < {valid}",
                a.name
            );
        }
    }
}

#[test]
fn estimate_covers_measured_need_at_2k_4k_8k_contexts() {
    // KV scales exactly with n_ctx; the score bound scales with n_ctx too.
    for a in anchors() {
        let at = |n| working_memory_estimate(&a.dims, &ContextShape::new(n));
        let (e2, e4, e8) = (at(2048), at(4096), at(8192));
        assert!(e2 < e4 && e4 < e8, "{}: {e2} {e4} {e8}", a.name);
        let measured4 = a.kv_bytes as f64 + a.compute_mib * MIB;
        assert!(e4 as f64 >= measured4, "{}", a.name);
        // KV at 8k is exactly twice KV at 4k: the estimate must at least add it.
        assert!((e8 - e4) as f64 >= a.kv_bytes as f64, "{}", a.name);
    }
}
