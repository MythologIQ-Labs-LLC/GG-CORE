//! N7 (#184) real-model tests for the deterministic prefill schedule.
//!
//! A stop request is observed only between prefill chunks and between
//! generated tokens, so the longest chunk is the worst-case stop tail.
//! These tests cover four properties:
//! - the split is identical with or without a stop check and under CPU load
//!   (the review found that the former time-paced split changed greedy
//!   output, `d184`);
//! - a single-thread stop lands in prefill and returns within a bounded tail;
//! - the decoded split is the planned one;
//! - optionally (`GG_N7_MEASURE=1`), prints chunk timings. This mode asserts
//!   only that the output is equal. Gate: `test_fixture::real_model`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::engine::gguf::prefill::recorder::recording;
use crate::engine::gguf::prefill_schedule::{chunk, MAX_PREFILL_CHUNK};
use crate::engine::gguf::test_fixture::{real_model, required};
use crate::engine::gguf::GgufConfig;

const FOX: &str = "The brown fox walks beside a clear river under a quiet blue sky. ";

fn load(path: &Path, n_threads: u32, n_ctx: u32) -> LlamaBackendInner {
    let cfg = GgufConfig {
        n_ctx,
        n_threads,
        n_gpu_layers: 0,
    };
    LlamaBackendInner::load(path, &cfg).expect("fixture must load")
}

fn long_prompt() -> String {
    format!(
        "Reply with the word READY after this note: {}",
        FOX.repeat(179)
    )
}

/// The d184 prompt whose greedy output changed with the split (100 tokens).
fn short_prompt() -> String {
    "Write a short poem about rivers, autumn light and patience.".repeat(9)
}

fn greedy(max_tokens: u32) -> InferenceConfig {
    InferenceConfig {
        max_tokens: Some(max_tokens),
        temperature: 0.0,
        top_p: 1.0,
        top_k: 1,
        ..InferenceConfig::default()
    }
}

/// The split the schedule must produce for `n` prompt tokens on `backend`.
fn planned(backend: &LlamaBackendInner, n: usize) -> Vec<usize> {
    let ctx = backend.create_context().expect("context");
    let cap = MAX_PREFILL_CHUNK.min(ctx.n_batch().min(ctx.n_ubatch()) as usize);
    let threads = u32::try_from(backend.n_threads).unwrap_or(1);
    let (mut pos, mut out) = (0, Vec::new());
    while pos < n {
        let len = chunk(pos, threads, backend.model.size(), cap).min(n - pos);
        out.push(len);
        pos += len;
    }
    out
}

/// Saturate the CPU from other threads while `f` runs on this one.
fn under_load<T>(f: impl FnOnce() -> T) -> T {
    let stop = Arc::new(AtomicBool::new(false));
    let spinners: Vec<_> = (0..8)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut x = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                }
            })
        })
        .collect();
    let out = f();
    stop.store(true, Ordering::Relaxed);
    spinners
        .into_iter()
        .for_each(|s| s.join().expect("spinner"));
    out
}

/// One generation's text and recorded prefill split.
fn run(backend: &LlamaBackendInner, prompt: &str, stop: StopCheck<'_>) -> (String, Vec<usize>) {
    let (r, chunks) = recording(|| backend.generate_cancellable(prompt, &greedy(64), stop));
    (r.expect("generation").text, chunks)
}

#[test]
fn real_model_prefill_split_and_output_are_load_independent() {
    let Some(path) = real_model() else {
        return;
    };
    for (threads, prompt) in [(4, long_prompt()), (4, short_prompt()), (1, short_prompt())] {
        let backend = load(path, threads, 8192);
        let n = backend.tokenize(&prompt).expect("tokenize").len();
        let plan = planned(&backend, n);
        let never = || false;
        let base = run(&backend, &prompt, None);
        let stoppable = run(&backend, &prompt, Some(&never));
        let loaded = under_load(|| run(&backend, &prompt, Some(&never)));
        eprintln!("N7D threads={threads} tokens={n} chunks={:?}", base.1);
        assert_eq!(base.1, plan, "split must be the planned schedule");
        assert_eq!(stoppable.1, plan, "a stop check must not change the split");
        assert_eq!(loaded.1, plan, "CPU load must not change the split");
        assert_eq!(stoppable.0, base.0, "greedy output with a stop check");
        assert_eq!(loaded.0, base.0, "greedy output under load");
    }
}

/// Opt-in (`GG_N7_MEASURE=1`) acceptance matrix: every thread bucket x the
/// three d184 prompts, each with and without a stop check and under load.
#[test]
fn real_model_n7_determinism_matrix() {
    if std::env::var("GG_N7_MEASURE").as_deref() != Ok("1") {
        eprintln!("SKIP: set GG_N7_MEASURE=1 to run the N7 determinism matrix");
        return;
    }
    let Some(path) = real_model() else {
        return;
    };
    let medium = format!("Summarize in one sentence: {}", FOX.repeat(40));
    for threads in [1, 2, 4, 8] {
        let backend = load(path, threads, 8192);
        for prompt in [long_prompt(), medium.clone(), short_prompt()] {
            let never = || false;
            let base = run(&backend, &prompt, None);
            let loaded = under_load(|| run(&backend, &prompt, Some(&never)));
            let same = base == loaded;
            eprintln!(
                "N7D matrix threads={threads} chunks={} same={same}",
                base.1.len()
            );
            assert!(same, "split and output must not depend on stop or load");
        }
    }
}

/// One thread made a fixed 512-token chunk take 5 to 18 s on the
/// qualification VM, so a stop set mid-prefill used to be honored that late.
/// The schedule's chunks at one thread are 64 tokens. The wall-clock limit
/// is a regression threshold with margin for a noisy host; the recorded
/// split is the host-independent check.
#[test]
fn real_model_single_thread_prefill_stop_returns_within_bounded_tail() {
    let Some(path) = real_model() else {
        return;
    };
    let backend = load(path, 1, 8192);
    let prompt = long_prompt();
    let started = Instant::now();
    let stop_after = Duration::from_millis(1000);
    let stop = move || started.elapsed() >= stop_after;
    let (r, chunks) = recording(|| {
        backend.generate_cancellable(&prompt, &InferenceConfig::default(), Some(&stop))
    });
    let r = r.expect("a stop is not a backend error");
    let tail = started.elapsed().saturating_sub(stop_after);
    eprintln!(
        "N7M single_thread_stop tail_ms={} chunks={}",
        tail.as_millis(),
        chunks.len()
    );
    if r.finish_reason != FinishReason::Cancelled {
        assert!(!required(), "host finished the request before the stop");
        eprintln!("SKIP: host finished the whole request before the stop was set");
        return;
    }
    assert_eq!(r.tokens_generated, 0, "the stop must land inside prefill");
    assert!(
        chunks.iter().all(|&c| c <= 128),
        "one-thread chunks: {chunks:?}"
    );
    let limit = Duration::from_secs(5);
    assert!(
        tail < limit,
        "stop honored {tail:?} after the request (limit {limit:?})"
    );
}

/// Opt-in characterization: per-chunk time of the schedule against the fixed
/// 512-token split, per thread count and context. Asserts only that two
/// scheduled runs produce identical greedy output.
#[test]
fn real_model_n7_prefill_characterization() {
    if std::env::var("GG_N7_MEASURE").as_deref() != Ok("1") {
        eprintln!("SKIP: set GG_N7_MEASURE=1 to run the N7 characterization");
        return;
    }
    let Some(path) = real_model() else {
        return;
    };
    for n_ctx in [8192, 16384] {
        for n_threads in [1, 2, 4] {
            characterize(&load(path, n_threads, n_ctx), n_threads, n_ctx);
        }
    }
}

fn characterize(backend: &LlamaBackendInner, n_threads: u32, n_ctx: u32) {
    let tokens = backend.tokenize(&long_prompt()).expect("tokenize");
    let mut ctx = backend.create_context().expect("context");
    let t = Instant::now();
    decode_range(&mut ctx, &tokens, 0, false).expect("fixed 512 prefill");
    let fixed_ms = t.elapsed().as_millis();
    drop(ctx);
    let marks = Mutex::new(vec![Instant::now()]);
    let mark = || {
        marks.lock().expect("marks").push(Instant::now());
        false
    };
    let mut ctx = backend.create_context().expect("context");
    let t = Instant::now();
    assert!(backend
        .prefill(&mut ctx, &tokens, Some(&mark))
        .expect("prefill"));
    let scheduled_ms = t.elapsed().as_millis();
    let mut m = marks.into_inner().expect("marks");
    m.push(Instant::now());
    let worst = m[1..]
        .windows(2)
        .map(|w| (w[1] - w[0]).as_millis())
        .max()
        .unwrap_or(0);
    let a = backend
        .generate(&long_prompt(), &greedy(16))
        .expect("run a")
        .text;
    let b = backend
        .generate(&long_prompt(), &greedy(16))
        .expect("run b")
        .text;
    eprintln!(
        "N7M prefill threads={n_threads} n_ctx={n_ctx} tokens={} fixed512_ms={fixed_ms} scheduled_ms={scheduled_ms} worst_chunk_ms={worst} same_output={}",
        tokens.len(),
        a == b
    );
    assert_eq!(a, b, "scheduled prefill must be reproducible");
}
