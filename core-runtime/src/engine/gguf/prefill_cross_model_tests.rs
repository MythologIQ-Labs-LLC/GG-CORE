//! Opt-in cross-model acceptance for plan Revision 4 (#184): the sub-64
//! remainder is decoded in bounded pieces, and the split and greedy output
//! stay load-independent on larger models.
//!
//! Runs only with `GG_N7_MEASURE=1` and `GG_N7_CROSS_MODELS` set to a
//! `;`-separated list of provenance-verified GGUF paths (set by the
//! qualification job). No prompt text is logged.

use std::sync::Mutex;
use std::time::Instant;

use super::*;
use crate::engine::gguf::prefill::planned_split;
use crate::engine::gguf::prefill::recorder::recording;
use crate::engine::gguf::test_fixture::{serial, under_load};
use crate::engine::gguf::GgufConfig;

const FOX: &str = "The brown fox walks beside a clear river under a quiet blue sky. ";

fn prompt() -> String {
    format!(
        "Reply with the word READY after this note: {}",
        FOX.repeat(179)
    )
}

/// Recorded chunk lengths and each chunk's wall time for one prefill.
fn timed_split(backend: &LlamaBackendInner, tokens: &[LlamaToken]) -> Vec<(usize, u128)> {
    let mut ctx = backend.create_context().expect("context");
    let marks = Mutex::new(Vec::new());
    let mark = || {
        marks.lock().expect("marks").push(Instant::now());
        false
    };
    let (ok, split) = recording(|| backend.prefill(&mut ctx, tokens, Some(&mark)));
    assert!(ok.expect("prefill"));
    let mut m = marks.into_inner().expect("marks");
    m.push(Instant::now());
    assert_eq!(m.len(), split.len() + 1, "one stop check per chunk");
    let times = m.windows(2).map(|w| (w[1] - w[0]).as_millis());
    split.into_iter().zip(times).collect()
}

/// Greedy text and recorded split of one 16-token generation.
fn greedy(backend: &LlamaBackendInner, text: &str) -> (String, Vec<usize>) {
    let cfg = InferenceConfig {
        max_tokens: Some(16),
        temperature: 0.0,
        top_k: 1,
        ..InferenceConfig::default()
    };
    let (r, split) = recording(|| backend.generate(text, &cfg));
    (r.expect("generation").text, split)
}

/// Report every remainder piece against `(p + 6) / 24` of the run's last
/// 64-token chunk (audit R4-2 C2) and gate the worst chunk.
fn check(name: &str, threads: u32, chunks: &[(usize, u128)]) {
    let floor = chunks.iter().filter(|c| c.0 == 64).map(|c| c.1).max();
    let last64 = chunks.iter().rev().find(|c| c.0 == 64).map(|c| c.1);
    let worst = chunks.iter().map(|c| c.1).max().unwrap_or(0);
    for &(p, ms) in chunks.iter().filter(|c| c.0 < 64) {
        let bound = last64.map(|t| (p as u128 + 6) * t / 24);
        let over = bound.is_some_and(|b| ms > b);
        eprintln!(
            "N7X piece model={name} threads={threads} p={p} ms={ms} bound_ms={bound:?} over={over}"
        );
    }
    let limit = 1000.max(floor.unwrap_or(0)) * 12 / 10;
    eprintln!("N7X model={name} threads={threads} chunks={} worst_ms={worst} worst64_ms={floor:?} limit_ms={limit}", chunks.len());
    assert!(
        worst <= limit,
        "{name} t={threads}: worst chunk {worst} ms > {limit} ms"
    );
}

#[test]
fn real_model_n7_cross_model_remainder_acceptance() {
    let models = std::env::var("GG_N7_CROSS_MODELS").unwrap_or_default();
    if std::env::var("GG_N7_MEASURE").as_deref() != Ok("1") || models.is_empty() {
        eprintln!("SKIP: set GG_N7_MEASURE=1 and GG_N7_CROSS_MODELS to run");
        return;
    }
    let _serial = serial();
    for path in models.split(';').filter(|p| !p.is_empty()) {
        let name = Path::new(path)
            .file_stem()
            .map_or("model".into(), |s| s.to_string_lossy());
        for threads in [1, 4] {
            let cfg = GgufConfig {
                n_ctx: 8192,
                n_threads: threads,
                n_gpu_layers: 0,
            };
            let backend = LlamaBackendInner::load(Path::new(path), &cfg).expect("model");
            let tokens = backend.tokenize(&prompt()).expect("tokenize");
            let plan = planned_split(&backend, tokens.len());
            let chunks = timed_split(&backend, &tokens);
            assert_eq!(chunks.iter().map(|c| c.0).collect::<Vec<_>>(), plan);
            check(&name, threads, &chunks);
            let base = greedy(&backend, &prompt());
            let loaded = under_load(|| greedy(&backend, &prompt()));
            eprintln!(
                "N7X model={name} threads={threads} tokens={} split_tail={:?} same_under_load={}",
                tokens.len(),
                &plan[plan.len().saturating_sub(5)..],
                base == loaded
            );
            assert_eq!(base.1, plan, "split must be the planned schedule");
            assert_eq!(loaded, base, "split and output under load");
        }
    }
}
