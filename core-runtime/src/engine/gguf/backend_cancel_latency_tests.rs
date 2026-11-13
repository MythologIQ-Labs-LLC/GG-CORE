//! N7 (#184) stop-tail regression and opt-in characterization for the
//! deterministic prefill schedule.
//!
//! A stop request is observed only between prefill chunks and between
//! generated tokens, so the longest chunk is the worst-case stop tail. The
//! split's load independence is proved in `prefill_split_tests.rs`. Gate:
//! `test_fixture::real_model`. No prompt text is logged.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::engine::gguf::prefill::planned_split;
use crate::engine::gguf::prefill::recorder::recording;
use crate::engine::gguf::test_fixture::{real_model, required, serial};
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

/// One thread made a fixed 512-token chunk take 5 to 18 s on the
/// qualification VM, so a stop set mid-prefill used to be honored that late.
/// The schedule's chunks at one thread are 64 tokens. The wall-clock limit
/// is a regression threshold with margin for a noisy host; the recorded
/// split (a prefix of the planned one) is the host-independent check.
#[test]
fn real_model_single_thread_prefill_stop_returns_within_bounded_tail() {
    let Some(path) = real_model() else {
        return;
    };
    let _serial = serial();
    let backend = load(path, 1, 8192);
    let prompt = long_prompt();
    let plan = planned_split(&backend, backend.tokenize(&prompt).expect("tokenize").len());
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
    assert!(chunks.len() < plan.len(), "the stop must end prefill early");
    assert_eq!(
        chunks,
        plan[..chunks.len()],
        "decoded chunks follow the plan"
    );
    let limit = Duration::from_secs(5);
    assert!(
        tail < limit,
        "stop honored {tail:?} after the request (limit {limit:?})"
    );
}

/// Opt-in characterization (`GG_N7_MEASURE=1`, set by the qualification
/// job): worst scheduled chunk time and prefill time against the fixed
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
    let _serial = serial();
    for n_ctx in [8192, 16384] {
        for n_threads in [1, 2, 4] {
            characterize(&load(path, n_threads, n_ctx), n_threads, n_ctx);
        }
    }
}

/// Wall time of one prefill and its longest chunk, from the stop-check
/// marks (`scheduled = false`: fixed 512-token `decode_range`, one unit).
fn timed_prefill(
    backend: &LlamaBackendInner,
    tokens: &[LlamaToken],
    scheduled: bool,
) -> (u128, u128) {
    let mut ctx = backend.create_context().expect("context");
    let marks = Mutex::new(vec![Instant::now()]);
    let mark = || {
        marks.lock().expect("marks").push(Instant::now());
        false
    };
    let t = Instant::now();
    if scheduled {
        assert!(backend
            .prefill(&mut ctx, tokens, Some(&mark))
            .expect("prefill"));
    } else {
        decode_range(&mut ctx, tokens, 0, false).expect("fixed 512 prefill");
    }
    let total = t.elapsed().as_millis();
    let mut m = marks.into_inner().expect("marks");
    m.push(Instant::now());
    let worst = m[1..].windows(2).map(|w| (w[1] - w[0]).as_millis()).max();
    (total, worst.unwrap_or(total))
}

fn characterize(backend: &LlamaBackendInner, n_threads: u32, n_ctx: u32) {
    let tokens = backend.tokenize(&long_prompt()).expect("tokenize");
    let (fixed_ms, _) = timed_prefill(backend, &tokens, false);
    let (scheduled_ms, worst) = timed_prefill(backend, &tokens, true);
    let cfg = InferenceConfig {
        max_tokens: Some(16),
        temperature: 0.0,
        top_k: 1,
        ..InferenceConfig::default()
    };
    let a = backend.generate(&long_prompt(), &cfg).expect("run a").text;
    let b = backend.generate(&long_prompt(), &cfg).expect("run b").text;
    eprintln!(
        "N7M prefill threads={n_threads} n_ctx={n_ctx} tokens={} fixed512_ms={fixed_ms} scheduled_ms={scheduled_ms} worst_chunk_ms={worst} same_output={}",
        tokens.len(),
        a == b
    );
    assert_eq!(a, b, "scheduled prefill must be reproducible");
}
