//! Real-model regression: cancellation and deadlines during prompt PREFILL.
//!
//! A ~2,500-token prompt spends seconds in prefill, so a per-token check alone
//! let a stopped request run the whole prefill (and IPC then returned a
//! success). These tests prove that prefill stops at a decode-chunk
//! checkpoint, that the failure is explicit, that a cancelled request does
//! not disturb a concurrent one, and that the model serves the next request.
//! Gated by `test_fixture::real_model` (`GG_CORE_REQUIRE_REAL_MODEL=1` fails).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, MutexGuard};
use std::time::{Duration, Instant};

use super::LlamaBackendInner;
use crate::engine::gguf::{test_fixture, GgufConfig, GgufGenerator};
use crate::engine::inference::{InferenceEngine, InferenceError as RunError, InferenceParams};
use crate::engine::{FinishReason, InferenceConfig, Model};

const FOX: &str = "The brown fox walks beside a clear river under a quiet blue sky. ";

/// Fixture, config and serial guard (held for the test: timing baselines).
fn fixture() -> Option<(PathBuf, GgufConfig, MutexGuard<'static, ()>)> {
    let path = test_fixture::real_model()?.to_path_buf();
    let serial = test_fixture::serial();
    let cfg = GgufConfig {
        n_ctx: 4096,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    Some((path, cfg, serial))
}

fn long_prompt() -> String {
    format!(
        "Reply with the word READY after reading this note: {}",
        FOX.repeat(179)
    )
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

/// Uncancelled prefill + one token of the long prompt: the work a stopped
/// request must avoid.
fn baseline(backend: &LlamaBackendInner, prompt: &str) -> Duration {
    let start = Instant::now();
    let r = backend
        .generate(prompt, &greedy(1))
        .expect("uncancelled long prompt must complete");
    assert!(r.tokens_generated <= 1);
    start.elapsed()
}

#[test]
fn real_model_prefill_cancel_stops_at_chunk_checkpoint() {
    let Some((path, cfg, _serial)) = fixture() else {
        return;
    };
    let backend = LlamaBackendInner::load(&path, &cfg).expect("real Qwen fixture must load");
    let prompt = long_prompt();
    let tokens = backend.tokenize(&prompt).expect("tokenize").len();
    assert!(
        tokens > 2048,
        "fixture must be a long prefill ({tokens} tokens)"
    );
    let full = baseline(&backend, &prompt);

    // Cancel after the first prefill chunk: checks 1 (pre-context) and 2
    // (first chunk) pass, the third check stops the request.
    let checks = AtomicUsize::new(0);
    let stop = || checks.fetch_add(1, Ordering::SeqCst) >= 2;
    let start = Instant::now();
    let r = backend
        .generate_cancellable(&prompt, &greedy(64), Some(&stop))
        .expect("cancellation is not a backend error");
    let stopped = start.elapsed();
    eprintln!(
        "prefill_cancel: tokens={tokens} full_prefill_ms={} cancelled_ms={} checks={}",
        full.as_millis(),
        stopped.as_millis(),
        checks.load(Ordering::SeqCst)
    );
    assert_eq!(r.finish_reason, FinishReason::Cancelled);
    assert_eq!(r.tokens_generated, 0, "no token may be produced");
    assert!(r.text.is_empty());
    assert!(
        stopped < full / 2,
        "prefill must stop at a chunk checkpoint ({stopped:?} vs full {full:?})"
    );

    // Recovery: the same model serves a normal request right after.
    let ok = backend
        .generate("Answer in one word: hello.", &greedy(4))
        .expect("model must serve the next request after a cancelled prefill");
    assert!(ok.tokens_generated >= 1);
}

#[test]
fn real_model_cancelled_prefill_does_not_disturb_concurrent_request() {
    let Some((path, cfg, _serial)) = fixture() else {
        return;
    };
    let backend =
        Arc::new(LlamaBackendInner::load(&path, &cfg).expect("real Qwen fixture must load"));
    let other_prompt = format!("Repeat the first sentence of this note: {}", FOX.repeat(40));
    // Reference without a stop check: prefill follows the same deterministic
    // schedule with or without one (#184), so the concurrent, cancellable
    // run below must decode the same split and produce the same text.
    let reference = backend
        .generate_cancellable(&other_prompt, &greedy(8), None)
        .expect("reference run")
        .text;

    let cancel = Arc::new(AtomicBool::new(false));
    let victim = {
        let (backend, cancel) = (Arc::clone(&backend), Arc::clone(&cancel));
        std::thread::spawn(move || {
            let stop = move || cancel.load(Ordering::Acquire);
            backend.generate_cancellable(&long_prompt(), &greedy(64), Some(&stop))
        })
    };
    let survivor = {
        let backend = Arc::clone(&backend);
        let prompt = other_prompt.clone();
        std::thread::spawn(move || {
            let never = || false;
            backend.generate_cancellable(&prompt, &greedy(8), Some(&never))
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    cancel.store(true, Ordering::Release);

    let v = victim
        .join()
        .expect("victim thread")
        .expect("victim result");
    let s = survivor
        .join()
        .expect("survivor thread")
        .expect("survivor result");
    assert_eq!(v.finish_reason, FinishReason::Cancelled);
    assert_ne!(s.finish_reason, FinishReason::Cancelled);
    assert_eq!(
        s.text, reference,
        "concurrent request output must be unaffected"
    );
}

async fn engine_with_fixture(path: &std::path::Path, cfg: &GgufConfig) -> InferenceEngine {
    let generator = GgufGenerator::load("qwen".into(), path, cfg).expect("load generator");
    let engine = InferenceEngine::new(4096);
    let model: Arc<dyn Model> = Arc::new(generator);
    engine
        .register_model("qwen".into(), crate::models::ModelHandle::new(1), model)
        .await;
    engine
}

fn params(max_tokens: usize, timeout_ms: Option<u64>) -> InferenceParams {
    InferenceParams {
        max_tokens,
        temperature: 0.0,
        top_p: 1.0,
        top_k: 1,
        stream: false,
        timeout_ms,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::await_holding_lock)] // serial guard across this test's own runtime
async fn real_model_nonstream_deadline_and_cancel_fail_explicitly() {
    let Some((path, cfg, _serial)) = fixture() else {
        return;
    };
    let engine = engine_with_fixture(&path, &cfg).await;
    let prompt = long_prompt();
    let start = Instant::now();
    engine
        .run("qwen", &prompt, &params(1, None))
        .await
        .expect("uncancelled long prompt");
    let full = start.elapsed();

    // Deadline: a tenth of the measured prefill must expire inside prefill.
    let timeout = (full.as_millis() as u64 / 10).max(50);
    let start = Instant::now();
    let r = engine
        .run("qwen", &prompt, &params(256, Some(timeout)))
        .await;
    let expired = start.elapsed();
    eprintln!(
        "nonstream_deadline: full_ms={} timeout_ms={timeout} returned_ms={}",
        full.as_millis(),
        expired.as_millis()
    );
    assert!(
        matches!(r, Err(RunError::DeadlineExceeded { timeout_ms }) if timeout_ms == timeout),
        "expected DeadlineExceeded, got {r:?}"
    );
    assert!(
        expired < full / 2,
        "deadline must interrupt prefill ({expired:?} vs {full:?})"
    );

    // Caller cancellation from another thread during prefill.
    let flag = Arc::new(AtomicBool::new(false));
    let setter = {
        let flag = Arc::clone(&flag);
        let delay = Duration::from_millis(timeout);
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            flag.store(true, Ordering::Release);
        })
    };
    let start = Instant::now();
    let r = engine
        .run_cancellable("qwen", &prompt, &params(256, None), flag)
        .await;
    let cancelled = start.elapsed();
    setter.join().expect("setter");
    eprintln!("nonstream_cancel: returned_ms={}", cancelled.as_millis());
    assert!(
        matches!(r, Err(RunError::Cancelled)),
        "expected Cancelled, got {r:?}"
    );
    assert!(
        cancelled < full / 2,
        "cancel must interrupt prefill ({cancelled:?} vs {full:?})"
    );

    // Recovery: successful inference after the stopped requests.
    let ok = engine
        .run(
            "qwen",
            "Answer in one word: hello.",
            &params(4, Some(60_000)),
        )
        .await
        .expect("inference must succeed after a cancelled request");
    assert!(ok.tokens_generated >= 1);
}
