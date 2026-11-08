//! Working-memory estimate against measured llama.cpp allocations
//! (offline Hyper-V guest, n_ctx 4096, n_ubatch 512, f16 KV, flash attention
//! auto -> enabled; `llama_kv_cache` / `sched_reserve` log lines, 2026-11-08).

use super::*;

const MIB: f64 = 1024.0 * 1024.0;

struct Anchor {
    name: &'static str,
    dims: ModelDims,
    kv_bytes: u64,
    compute_mib: f64,
}

fn dims(
    n_layer: u64,
    n_embd: u64,
    n_head: u64,
    n_head_kv: u64,
    n_ff: u64,
    n_vocab: u64,
) -> ModelDims {
    ModelDims {
        n_layer,
        n_embd,
        n_head,
        n_head_kv,
        n_vocab,
        n_ff: Some(n_ff),
        head_k: None,
        head_v: None,
    }
}

fn anchors() -> [Anchor; 3] {
    [
        Anchor {
            name: "qwen2.5-0.5b",
            dims: dims(24, 896, 14, 2, 4864, 151_936),
            kv_bytes: 50_331_648,
            compute_mib: 309.52,
        },
        Anchor {
            name: "smollm2-1.7b",
            dims: dims(24, 2048, 32, 32, 8192, 49_152),
            kv_bytes: 805_306_368,
            compute_mib: 120.02,
        },
        Anchor {
            name: "phi3.5-mini",
            dims: dims(32, 3072, 32, 32, 8192, 32_064),
            kv_bytes: 1_610_612_736,
            compute_mib: 96.64,
        },
    ]
}

#[test]
fn estimate_covers_measured_need_within_bound() {
    let shape = ContextShape::new(4096);
    for a in anchors() {
        let measured = a.kv_bytes as f64 + a.compute_mib * MIB;
        let est = working_memory_estimate(&a.dims, &shape) as f64;
        assert!(est >= measured, "{}: {est} < measured {measured}", a.name);
        assert!(
            est <= 2.25 * measured,
            "{}: {est} > 2.25x {measured}",
            a.name
        );
    }
}

#[test]
fn kv_term_is_exact_for_measured_models() {
    let shape = ContextShape::new(4096);
    for a in anchors() {
        let head = a.dims.n_embd / a.dims.n_head;
        let kv = a.dims.n_layer * 4096 * a.dims.n_head_kv * head * 2 * 2;
        assert_eq!(kv, a.kv_bytes, "{}", a.name);
        let without_kv = raw_bytes(&a.dims, &shape).unwrap() - kv;
        assert!(without_kv as f64 >= a.compute_mib * MIB * 0.9, "{}", a.name);
    }
}

#[test]
fn estimate_scales_with_context() {
    let a = &anchors()[1];
    let small = working_memory_estimate(&a.dims, &ContextShape::new(1024));
    let large = working_memory_estimate(&a.dims, &ContextShape::new(4096));
    assert!(large > small * 3, "{small} -> {large}");
}

#[test]
fn missing_ff_falls_back_to_conservative_bound() {
    let shape = ContextShape::new(4096);
    for a in anchors() {
        let known = working_memory_estimate(&a.dims, &shape);
        let missing = ModelDims {
            n_ff: None,
            ..a.dims
        };
        assert!(
            working_memory_estimate(&missing, &shape) >= known,
            "{}: fallback below the formula",
            a.name
        );
    }
}

#[test]
fn zero_heads_does_not_panic_and_is_conservative() {
    let d = ModelDims {
        n_head: 0,
        n_head_kv: 0,
        ..anchors()[0].dims
    };
    let est = working_memory_estimate(&d, &ContextShape::new(4096));
    assert!(est >= working_memory_estimate(&anchors()[0].dims, &ContextShape::new(4096)));
}

#[test]
fn overflow_is_never_admissible() {
    let d = ModelDims {
        n_layer: u64::MAX,
        ..anchors()[0].dims
    };
    assert_eq!(
        working_memory_estimate(&d, &ContextShape::new(4096)),
        usize::MAX
    );
}

#[test]
fn explicit_head_lengths_are_used() {
    let base = anchors()[1].dims;
    let wide = ModelDims {
        head_k: Some(256),
        head_v: Some(256),
        ..base
    };
    let shape = ContextShape::new(4096);
    assert!(working_memory_estimate(&wide, &shape) > working_memory_estimate(&base, &shape));
}

#[test]
fn shape_defaults_to_auto_flash_attention_and_fixed_ubatch() {
    let s = ContextShape::new(2048);
    assert_eq!(
        (s.n_ctx, s.n_ubatch, s.flash_attn_disabled),
        (2048, 512, false)
    );
}

#[cfg(feature = "gguf")]
#[test]
fn context_params_follow_the_shape() {
    let p = ContextShape::new(3072).params(4);
    assert_eq!(p.n_ctx().map(|n| n.get()), Some(3072));
    assert_eq!(p.n_ubatch(), 512);
    let off = ContextShape {
        flash_attn_disabled: true,
        ..ContextShape::new(3072)
    };
    assert_eq!(off.params(4).flash_attention_policy(), 0);
}

#[cfg(all(feature = "gguf", windows))]
#[path = "working_memory_real_tests.rs"]
mod real;
