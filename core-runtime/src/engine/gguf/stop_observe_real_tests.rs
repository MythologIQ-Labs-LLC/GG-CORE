//! Real-model S3 (#184): a stop during a 1-thread multi-chunk prefill is
//! observed once, at the next chunk boundary, and reports the chunk that
//! was in flight; a stop during generation reports `phase = decode` and a
//! one-token decode. Gated by `test_fixture::real_model`.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::backend::LlamaBackendInner;
use super::super::prefill::planned_split;
use super::super::{test_fixture, GgufConfig};
use crate::engine::{FinishReason, InferenceConfig};
use crate::telemetry::stop_trace::{channel, subscriber};

const FOX: &str = "The brown fox walks beside a clear river under a quiet blue sky. ";

fn backend() -> Option<(LlamaBackendInner, std::sync::MutexGuard<'static, ()>)> {
    let path = test_fixture::real_model()?;
    let serial = test_fixture::serial();
    let cfg = GgufConfig {
        n_ctx: 4096,
        n_threads: 1,
        n_gpu_layers: 0,
    };
    Some((
        LlamaBackendInner::load(path, &cfg).expect("fixture loads"),
        serial,
    ))
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

/// Run `f` with the stop-trace subscriber on this thread; S3 events only.
fn observed<T>(f: impl FnOnce() -> T) -> (T, Vec<serde_json::Value>) {
    let (writer, rx) = channel(256);
    let out = tracing::subscriber::with_default(subscriber(writer), f);
    let events = rx
        .try_iter()
        .map(|l| serde_json::from_slice::<serde_json::Value>(&l).unwrap()["fields"].clone())
        .filter(|f| f["message"] == "backend stop observed")
        .collect();
    (out, events)
}

#[test]
fn real_model_prefill_stop_reports_the_in_flight_chunk() {
    let Some((backend, _serial)) = backend() else {
        return;
    };
    let prompt = format!("Reply with READY after reading: {}", FOX.repeat(60));
    let n = backend.tokenize(&prompt).expect("tokenize").len();
    let split = planned_split(&backend, n);
    assert!(
        split.len() >= 3,
        "multi-chunk prefill at 1 thread: {split:?}"
    );
    // Check 1 is request entry, check 2 precedes chunk 0, check 3 stops.
    let checks = AtomicUsize::new(0);
    let stop = || checks.fetch_add(1, Ordering::SeqCst) >= 2;
    let (r, s3) = observed(|| backend.generate_cancellable(&prompt, &greedy(16), Some(&stop)));
    assert_eq!(
        r.expect("not an error").finish_reason,
        FinishReason::Cancelled
    );
    assert_eq!(s3.len(), 1, "{s3:?}");
    let f = &s3[0];
    assert_eq!(f["phase"], "prefill");
    assert_eq!(f["done"].as_u64(), Some(split[0] as u64));
    assert_eq!(f["remaining"].as_u64(), Some((n - split[0]) as u64));
    assert_eq!(f["last_decode_tokens"].as_u64(), Some(split[0] as u64));
    assert!(f["last_decode_ms"].as_f64().unwrap() > 0.0, "{f}");
}

#[test]
fn real_model_generation_stop_reports_a_one_token_decode() {
    let Some((backend, _serial)) = backend() else {
        return;
    };
    let prompt = "Write a long story about a river.";
    let n = backend.tokenize(prompt).expect("tokenize").len();
    let chunks = planned_split(&backend, n).len();
    // Entry + one check per prefill chunk, then stop before generated token 2.
    let checks = AtomicUsize::new(0);
    let stop = || checks.fetch_add(1, Ordering::SeqCst) >= 1 + chunks + 2;
    let (r, s3) = observed(|| backend.generate_cancellable(prompt, &greedy(64), Some(&stop)));
    assert_eq!(
        r.expect("not an error").finish_reason,
        FinishReason::Cancelled
    );
    assert_eq!(s3.len(), 1, "{s3:?}");
    let f = &s3[0];
    assert_eq!(f["phase"], "decode");
    assert_eq!(f["done"].as_u64(), Some(2));
    assert_eq!(f["last_decode_tokens"].as_u64(), Some(1));
}
