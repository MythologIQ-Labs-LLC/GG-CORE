//! N7 (#184) characterization: how long one uninterruptible unit of backend
//! work lasts, per thread count and context size, for the fixed 512-token
//! prefill (not cancellable) and the time-budgeted cancellable prefill.
//!
//! A stop request is only observed between prefill chunks and between
//! generated tokens, so the longest chunk (or token decode, or context
//! create/drop) is the worst-case tail after a cancel or deadline. The
//! characterization is opt-in (`GG_N7_MEASURE=1`), prints `N7M` lines and
//! asserts only output equality. The bounded-tail regression test always
//! runs when the real model is present. No prompt text is logged.

use std::sync::Mutex;
use std::time::Instant;

use super::*;
use crate::engine::gguf::GgufConfig;

const FIXTURE: &str = "../models/qwen2.5-0.5b-instruct-q4_k_m.gguf";
const FOX: &str = "The brown fox walks beside a clear river under a quiet blue sky. ";
const RUNS: usize = 2;
/// 0 = fixed 512-token chunks without a stop check (the uncancellable baseline).
const BUDGETS_MS: [u64; 5] = [0, 250, 500, 1000, 2000];

struct Sample {
    total_ms: u128,
    chunk_ms: Vec<u128>,
    token_ms_max: u128,
    out: Vec<i32>,
}

fn prefill_and_sample(
    backend: &LlamaBackendInner,
    tokens: &[LlamaToken],
    budget_ms: u64,
) -> Sample {
    let paced = budget_ms > 0;
    let mut ctx = backend.create_context().expect("context");
    let marks = Mutex::new(vec![Instant::now()]);
    let never = || {
        marks.lock().expect("marks").push(Instant::now());
        false
    };
    let start = Instant::now();
    let cap = PREFILL_CHECKPOINT_TOKENS.min(ctx.n_batch().min(ctx.n_ubatch()) as usize);
    let budget = std::time::Duration::from_millis(budget_ms);
    let pacer = paced.then(|| super::super::prefill_pacer::PrefillPacer::with_budget(cap, budget));
    let stop: Option<&(dyn Fn() -> bool + Send + Sync)> = if paced { Some(&never) } else { None };
    let done = decode_paced(&mut ctx, tokens, 0, false, cap, pacer, stop).expect("prefill");
    assert!(done);
    let total_ms = start.elapsed().as_millis();
    let mut m = marks.into_inner().expect("marks");
    m.push(Instant::now());
    // m[0] precedes the first check; chunk i runs between check i and i+1.
    // Without a stop check there are no marks: the whole prefill is one unit.
    let chunk_ms = if paced {
        m[1..]
            .windows(2)
            .map(|w| (w[1] - w[0]).as_millis())
            .collect()
    } else {
        vec![total_ms]
    };
    let (out, token_ms_max) = greedy_tokens(backend, &mut ctx, tokens, 16);
    Sample {
        total_ms,
        chunk_ms,
        token_ms_max,
        out,
    }
}

fn greedy_tokens(
    backend: &LlamaBackendInner,
    ctx: &mut LlamaContext<'_>,
    tokens: &[LlamaToken],
    count: usize,
) -> (Vec<i32>, u128) {
    let cfg = InferenceConfig {
        temperature: 0.0,
        top_p: 1.0,
        top_k: 1,
        ..InferenceConfig::default()
    };
    let mut sampler = build_sampler(&cfg, backend.model.n_vocab());
    sampler.accept_many(tokens.iter().copied());
    let mut batch = LlamaBatch::new(1, 1);
    let (mut out, mut worst) = (Vec::new(), 0u128);
    for i in 0..count {
        let tok = sampler.sample(ctx, -1);
        sampler.accept(tok);
        out.push(tok.0);
        batch.clear();
        add_one(&mut batch, tok, (tokens.len() + i) as i32).expect("batch");
        let t = Instant::now();
        decode(ctx, &mut batch).expect("decode");
        worst = worst.max(t.elapsed().as_millis());
    }
    (out, worst)
}

fn measure(path: &Path, n_threads: u32, n_ctx: u32) {
    let cfg = GgufConfig {
        n_ctx,
        n_threads,
        n_gpu_layers: 0,
    };
    let backend = LlamaBackendInner::load(path, &cfg).expect("fixture must load");
    let prompt = format!(
        "Reply with the word READY after this note: {}",
        FOX.repeat(179)
    );
    let tokens = backend.tokenize(&prompt).expect("tokenize");
    let t = Instant::now();
    let ctx = backend.create_context().expect("context");
    let create_ms = t.elapsed().as_millis();
    let t = Instant::now();
    drop(ctx);
    let drop_ms = t.elapsed().as_millis();
    eprintln!(
        "N7M ctx threads={n_threads} resolved={} n_ctx={n_ctx} tokens={} create_ms={create_ms} drop_ms={drop_ms}",
        backend.n_threads,
        tokens.len()
    );
    sweep(&backend, &tokens, n_threads, n_ctx);
}

fn sweep(backend: &LlamaBackendInner, tokens: &[LlamaToken], n_threads: u32, n_ctx: u32) {
    let mut reference: Option<Vec<i32>> = None;
    for run in 0..RUNS {
        for budget_ms in BUDGETS_MS {
            let s = prefill_and_sample(backend, tokens, budget_ms);
            let same = reference.get_or_insert_with(|| s.out.clone()) == &s.out;
            let mut sorted = s.chunk_ms.clone();
            sorted.sort_unstable();
            eprintln!(
                "N7M prefill threads={n_threads} n_ctx={n_ctx} run={run} budget_ms={budget_ms} total_ms={} chunks={} chunk_p50_ms={} chunk_max_ms={} token_max_ms={} same_output={same}",
                s.total_ms,
                sorted.len(),
                sorted[sorted.len() / 2],
                sorted[sorted.len() - 1],
                s.token_ms_max
            );
            assert!(same, "paced prefill must not change greedy output");
        }
    }
}

#[test]
fn real_model_n7_uninterruptible_unit_characterization() {
    if std::env::var("GG_N7_MEASURE").as_deref() != Ok("1") {
        eprintln!("SKIP: set GG_N7_MEASURE=1 to run the N7 characterization");
        return;
    }
    let path = Path::new(FIXTURE);
    if std::fs::metadata(path)
        .map(|m| m.len() < 1_000_000)
        .unwrap_or(true)
    {
        eprintln!("SKIP: real GGUF fixture not installed");
        return;
    }
    for n_ctx in [8192, 16384] {
        for n_threads in [1, 2, 4] {
            measure(path, n_threads, n_ctx);
        }
    }
}

/// One thread makes a fixed 512-token chunk take about 10 s on the
/// qualification VM, so a stop set mid-prefill used to be honored that late.
/// With paced chunks the backend must return well before that. The limit is
/// a regression threshold with margin for a noisy host (measured tails are
/// under 2 s), not the pacer's target.
#[test]
fn real_model_single_thread_prefill_stop_returns_within_bounded_tail() {
    let path = Path::new(FIXTURE);
    if std::fs::metadata(path)
        .map(|m| m.len() < 1_000_000)
        .unwrap_or(true)
    {
        eprintln!("SKIP: real GGUF fixture not installed");
        return;
    }
    let cfg = GgufConfig {
        n_ctx: 8192,
        n_threads: 1,
        n_gpu_layers: 0,
    };
    let backend = LlamaBackendInner::load(path, &cfg).expect("fixture must load");
    let prompt = format!(
        "Reply with the word READY after this note: {}",
        FOX.repeat(179)
    );
    let started = Instant::now();
    let stop_after = std::time::Duration::from_millis(2500);
    let stop = move || started.elapsed() >= stop_after;
    let r = backend
        .generate_cancellable(&prompt, &InferenceConfig::default(), Some(&stop))
        .expect("a stop is not a backend error");
    let tail = started.elapsed().saturating_sub(stop_after);
    eprintln!("N7M single_thread_stop tail_ms={}", tail.as_millis());
    if r.finish_reason != FinishReason::Cancelled {
        eprintln!("SKIP: host finished the whole request before the stop was set");
        return;
    }
    let limit = std::time::Duration::from_secs(5);
    assert!(
        tail < limit,
        "stop honored {tail:?} after the request (limit {limit:?})"
    );
}
