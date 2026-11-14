//! Real-model proof that the prefill split, and with it greedy output, does
//! not depend on load, on a stop check, or on the entry point (N7 successor,
//! #184). The time-paced split this replaces changed output with host load
//! (review experiment `d184`), so each case asserts both the recorded split
//! of the production path and the generated text.

use std::sync::Arc;

use super::super::backend::LlamaBackendInner;
use super::super::test_fixture::{real_model, serial, under_load};
use super::super::GgufConfig;
use super::recorder::recording;
use super::{planned_split, StopCheck};
use crate::engine::{InferenceConfig, StreamBound, StreamItem, TokenStream};

const FOX: &str = "The brown fox walks beside a clear river under a quiet blue sky. ";

fn load(path: &std::path::Path, n_threads: u32) -> LlamaBackendInner {
    let cfg = GgufConfig {
        n_ctx: 8192,
        n_threads,
        n_gpu_layers: 0,
    };
    LlamaBackendInner::load(path, &cfg).expect("fixture must load")
}

/// The three d184 prompts: about 2516, 568 and 100 tokens. The 100-token one
/// produced different greedy output under different splits.
fn long_prompt() -> String {
    format!(
        "Reply with the word READY after this note: {}",
        FOX.repeat(179)
    )
}
fn medium_prompt() -> String {
    format!("Summarize in one sentence: {}", FOX.repeat(40))
}
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

/// One non-streaming generation's text and recorded prefill split.
fn run(backend: &LlamaBackendInner, prompt: &str, stop: StopCheck<'_>) -> (String, Vec<usize>) {
    let (r, chunks) = recording(|| backend.generate_cancellable(prompt, &greedy(64), stop));
    (r.expect("generation").text, chunks)
}

/// Assert split and output are the planned split and identical with no stop
/// check, with one, and with one under CPU load.
fn assert_load_independent(backend: &LlamaBackendInner, prompt: &str, threads: u32) {
    let n = backend.tokenize(prompt).expect("tokenize").len();
    let plan = planned_split(backend, n);
    let never = || false;
    let base = run(backend, prompt, None);
    let stoppable = run(backend, prompt, Some(&never));
    let loaded = under_load(|| run(backend, prompt, Some(&never)));
    eprintln!("N7D threads={threads} tokens={n} chunks={:?}", base.1);
    assert_eq!(base.1, plan, "split must be the planned schedule");
    assert_eq!(stoppable.1, plan, "a stop check must not change the split");
    assert_eq!(loaded.1, plan, "CPU load must not change the split");
    assert_eq!(stoppable.0, base.0, "greedy output with a stop check");
    assert_eq!(loaded.0, base.0, "greedy output under load");
}

/// Always-on: every thread bucket, each with a multi-chunk prompt (the
/// 100-token prompt is one chunk at 2 or more threads).
#[test]
fn real_model_prefill_split_and_output_are_load_independent() {
    let Some(path) = real_model() else {
        return;
    };
    let _serial = serial();
    let cases = [
        (4, long_prompt()),
        (1, short_prompt()),
        (2, medium_prompt()),
        (8, medium_prompt()),
    ];
    for (threads, prompt) in cases {
        assert_load_independent(&load(path, threads), &prompt, threads);
    }
}

/// Opt-in (`GG_N7_MEASURE=1`, set by the qualification job) acceptance
/// matrix: every thread bucket x the three d184 prompts.
#[test]
fn real_model_n7_determinism_matrix() {
    if std::env::var("GG_N7_MEASURE").as_deref() != Ok("1") {
        eprintln!("SKIP: set GG_N7_MEASURE=1 to run the N7 determinism matrix");
        return;
    }
    let Some(path) = real_model() else {
        return;
    };
    let _serial = serial();
    for threads in [1, 2, 4, 8] {
        let backend = load(path, threads);
        for prompt in [long_prompt(), medium_prompt(), short_prompt()] {
            assert_load_independent(&backend, &prompt, threads);
        }
    }
}

/// The streaming entry point decodes the same planned split (the recorder
/// is per thread, so it wraps the whole blocking generation).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::await_holding_lock)] // serial guard across this test's own runtime
async fn real_model_stream_prefill_follows_the_schedule() {
    let Some(path) = real_model() else {
        return;
    };
    let _serial = serial();
    let backend = Arc::new(load(path, 4));
    let prompt = long_prompt();
    let n = backend.tokenize(&prompt).expect("tokenize").len();
    let plan = planned_split(&backend, n);
    let (sender, mut stream) = TokenStream::new(256, StreamBound::default());
    let worker = {
        let backend = Arc::clone(&backend);
        tokio::task::spawn_blocking(move || {
            let never = || false;
            recording(|| backend.generate_stream(&prompt, &greedy(8), &sender, Some(&never), None))
        })
    };
    let (result, chunks) = worker.await.expect("stream worker");
    result.expect("stream generation");
    let mut tokens = 0;
    while let Ok(Some(item)) =
        tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await
    {
        tokens += usize::from(matches!(item, StreamItem::Token(_) | StreamItem::Text(_)));
    }
    eprintln!("N7D stream tokens={n} chunks={chunks:?} frames={tokens}");
    assert_eq!(chunks, plan, "streaming prefill must use the planned split");
    assert!(tokens >= 1, "the stream must produce output");
}
