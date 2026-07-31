//! E2E: B-21f speculative KV reuse + prompt-lookup draft against the real qwen
//! model. Model-gated (early-returns without models/qwen2.5-0.5b-instruct-q4_k_m.gguf,
//! so CI — which has no model — skips it). Requires `--features "gguf advanced"`.

#![cfg(all(feature = "gguf", feature = "advanced"))]

use std::path::Path;
use std::time::Instant;

use gg_core::engine::adaptive_speculative::executor::AdaptiveSpeculativeExecutor;
use gg_core::engine::adaptive_speculative::heuristic::{
    AdaptiveVerificationScheduler, HeuristicConfidenceEstimator,
};
use gg_core::engine::adaptive_speculative::prompt_lookup::PromptLookupDraft;
use gg_core::engine::adaptive_speculative::telemetry::SpeculativeTelemetry;
use gg_core::engine::gguf::{
    GgufConfig, GgufGenerator, GgufSpeculativeSession, GgufTargetVerifier,
};
use gg_core::models::speculative_config::{AdaptiveMode, AdaptiveSpeculativeConfig};

fn load() -> Option<GgufGenerator> {
    let p = Path::new("../models/qwen2.5-0.5b-instruct-q4_k_m.gguf");
    if !p.exists() {
        eprintln!("skip: model not found at {p:?}");
        return None;
    }
    let cfg = GgufConfig {
        n_ctx: 512,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    GgufGenerator::load("qwen".into(), p, &cfg).ok()
}

/// Exact greedy speculative decoding (greedy target) must produce byte-for-byte the
/// same tokens as single-model greedy generation — every committed token equals the
/// target's argmax, whether it came from an accepted draft or a correction.
#[test]
fn speculative_prompt_lookup_matches_single_model() {
    let Some(gen) = load() else {
        return;
    };
    let prompt = gen
        .tokenize("The quick brown fox. The quick brown fox. The quick brown")
        .unwrap();
    const N: usize = 24;
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Speculative path: prompt-lookup draft + session-backed (KV-reuse) verifier.
    let cfg = AdaptiveSpeculativeConfig {
        enabled: true,
        mode: AdaptiveMode::Balanced,
        ..Default::default()
    };
    let verifier = GgufTargetVerifier::new(&gen);
    let drafter = PromptLookupDraft::new(cfg.prompt_lookup_ngram, cfg.max_draft_tokens);
    let estimator = HeuristicConfidenceEstimator::new(0.0, 1.0);
    let scheduler = AdaptiveVerificationScheduler::new(cfg.clone());
    let telemetry = SpeculativeTelemetry::new();
    let executor = AdaptiveSpeculativeExecutor::new(
        &drafter,
        &verifier,
        &estimator,
        &scheduler,
        &telemetry,
        cfg.max_draft_tokens,
    );
    let spec_start = Instant::now();
    let spec = rt.block_on(executor.run(&prompt, N)).unwrap();
    let spec_ms = spec_start.elapsed().as_millis();

    // Reference: single-model greedy via the session's generate_one loop (Phase-1
    // proved this equals fresh-context generation).
    let inner = gen.backend_arc().unwrap();
    let mut sess = GgufSpeculativeSession::new(inner, &prompt).unwrap();
    let mut ctx = prompt.clone();
    let mut greedy = Vec::new();
    let ref_start = Instant::now();
    for _ in 0..N {
        let t = sess.generate_one(&ctx).unwrap();
        greedy.push(t);
        ctx.push(t);
        if Some(t) == gen.eos_token_id() {
            break;
        }
    }
    let ref_ms = ref_start.elapsed().as_millis();

    let n = spec.len().min(greedy.len());
    assert!(n > 0, "expected at least one generated token");
    assert_eq!(
        &spec[..n],
        &greedy[..n],
        "speculative greedy output must equal single-model greedy output"
    );
    eprintln!(
        "B-21f: speculative {spec_ms}ms vs single-model {ref_ms}ms over {n} tokens \
         (correctness asserted; wall-clock informational)"
    );
}
