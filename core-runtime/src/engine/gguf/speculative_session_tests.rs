//! Tests for the persistent-KV speculative session (B-21f). Model-gated: they
//! early-return without `models/qwen2.5-0.5b-instruct-q4_k_m.gguf` (CI has no model).

use std::sync::Arc;

use super::GgufSpeculativeSession;
use crate::engine::gguf::backend::LlamaBackendInner;
use crate::engine::gguf::{GgufConfig, GgufGenerator};

/// Load the local qwen model + a tokenized repetitive prompt, or `None` (skip).
fn fixture() -> Option<(Arc<LlamaBackendInner>, Vec<u32>)> {
    let path = std::path::Path::new("../models/qwen2.5-0.5b-instruct-q4_k_m.gguf");
    if !path.exists() {
        eprintln!("skip: model not found at {path:?}");
        return None;
    }
    let cfg = GgufConfig {
        n_ctx: 512,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    let gen = GgufGenerator::load("qwen".into(), path, &cfg).ok()?;
    let prompt = gen
        .tokenize("The quick brown fox jumps over the lazy dog. The quick brown")
        .ok()?;
    let inner = gen.backend_arc()?;
    Some((inner, prompt))
}

/// Incremental KV reuse must produce the same greedy tokens as decoding the whole
/// context from a fresh context each step.
#[test]
fn session_output_equals_fresh_context() {
    let Some((inner, prompt)) = fixture() else {
        return;
    };
    const N: usize = 4;

    // Incremental: one persistent session, generate_one N times.
    let mut sess = GgufSpeculativeSession::new(inner.clone(), &prompt).unwrap();
    let mut ctx = prompt.clone();
    let mut incremental = Vec::new();
    for _ in 0..N {
        let t = sess.generate_one(&ctx).unwrap();
        incremental.push(t);
        ctx.push(t);
    }

    // Reference: a brand-new session (fresh KV, full re-decode) each step.
    let mut ctx2 = prompt.clone();
    let mut fresh = Vec::new();
    for _ in 0..N {
        let mut s = GgufSpeculativeSession::new(inner.clone(), &ctx2).unwrap();
        let t = s.generate_one(&ctx2).unwrap();
        fresh.push(t);
        ctx2.push(t);
    }

    assert_eq!(
        incremental, fresh,
        "KV-reuse greedy output must equal fresh-context greedy output"
    );
}

/// A verify with a deliberately-wrong draft must leave the committed prefix intact:
/// a subsequent generate_one returns exactly what it would have without the verify.
#[test]
fn verify_rollback_leaves_committed_prefix() {
    let Some((inner, prompt)) = fixture() else {
        return;
    };

    // Baseline: next greedy token with no intervening verify.
    let mut base_sess = GgufSpeculativeSession::new(inner.clone(), &prompt).unwrap();
    let expected = base_sess.generate_one(&prompt).unwrap();

    // Same session: run a verify with a bogus draft (forces a rollback), then
    // generate_one must still yield `expected` — proving the draft positions were
    // removed and the committed prefix restored.
    let mut sess = GgufSpeculativeSession::new(inner, &prompt).unwrap();
    let bogus_draft = vec![u32::from(u16::MAX), 1, 2]; // near-certainly diverges at 0
    let vr = sess.verify(&prompt, &bogus_draft).unwrap();
    assert_eq!(vr.accepted_count, 0, "bogus draft should be rejected at 0");
    let after = sess.generate_one(&prompt).unwrap();
    assert_eq!(
        after, expected,
        "generate_one after a rolled-back verify must match the no-verify baseline"
    );
}
