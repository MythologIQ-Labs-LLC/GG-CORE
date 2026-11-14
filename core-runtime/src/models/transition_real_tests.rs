//! Real-model cold switch, in-process (not over IPC: the switch is not
//! exposed until #153 policy is approved). Opt-in: `GG_SWITCH_BASE` names a
//! base path whose `models/` holds the pinned fixtures; `GG_SWITCH_PAIRS` is
//! `;`-separated `relA|relB` pairs. Records greedy outputs, private bytes,
//! handle counts and timings as `SWITCH` lines. No prompt text is printed.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::engine::{InferenceEngine, InferenceParams};
use crate::memory::{ResourceLimits, ResourceLimitsConfig};
use crate::models::{ModelLoader, ModelRegistry};

const PROMPT: &str = "The three primary colors of light are";

#[cfg(windows)]
mod probe {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota: [usize; 4],
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(p: isize, c: *mut Counters, cb: u32) -> i32;
        fn GetProcessHandleCount(p: isize, count: *mut u32) -> i32;
    }
    /// (private bytes, working set, handle count) of this process.
    pub(super) fn sample() -> (usize, usize, u32) {
        let mut c = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Default::default()
        };
        let mut handles = 0u32;
        // SAFETY: valid out-pointers of the declared sizes; pseudo-handle.
        unsafe {
            K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
            GetProcessHandleCount(GetCurrentProcess(), &mut handles);
        }
        (c.private_usage, c.working_set_size, handles)
    }
}

#[cfg(not(windows))]
mod probe {
    pub(super) fn sample() -> (usize, usize, u32) {
        (0, 0, 0)
    }
}

fn note(pair: &str, step: &str, extra: &str) {
    let (private, ws, handles) = probe::sample();
    eprintln!(
        "SWITCH pair={pair} step={step} private_mb={} ws_mb={} handles={handles} {extra}",
        private >> 20,
        ws >> 20
    );
}

fn greedy(tokens: usize) -> InferenceParams {
    InferenceParams {
        max_tokens: tokens,
        temperature: 0.0,
        top_k: 1,
        top_p: 1.0,
        timeout_ms: Some(300_000),
        ..Default::default()
    }
}

async fn ask(engine: &InferenceEngine, id: &str, tokens: usize) -> Result<String, String> {
    let flag = Arc::new(AtomicBool::new(false));
    engine
        .run_cancellable(id, PROMPT, &greedy(tokens), flag)
        .await
        .map(|r| r.output)
        .map_err(|e| e.to_string())
}

fn digest(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(text.as_bytes())[..8])
}

struct Rig {
    engine: Arc<InferenceEngine>,
    lifecycle: Arc<ModelLifecycle>,
    loader: Arc<ModelLoader>,
    limits: ResourceLimits,
}

fn rig(base: &str) -> Rig {
    let limits = ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: 2 << 30,
        max_total_memory: 6 << 30,
        max_concurrent: 2,
    });
    let engine = Arc::new(InferenceEngine::new(32_768));
    let registry = Arc::new(ModelRegistry::new());
    let lifecycle =
        ModelLifecycle::new(registry, Arc::clone(&engine)).with_resource_limits(limits.clone());
    let loader = Arc::new(ModelLoader::new(base.into()));
    Rig {
        engine,
        lifecycle: Arc::new(lifecycle),
        loader,
        limits,
    }
}

async fn cold(rig: &Rig, from: &str, to_path: &str, to: &str) -> SwitchOutcome {
    let handle = rig.lifecycle.get_handle(from).await.expect("source loaded");
    let prep: Arc<dyn Preparer> = Arc::new(CanonicalPreparer {
        loader: Arc::clone(&rig.loader),
        workers: 1,
    });
    let req = ColdSwitchRequest {
        from_id: from.into(),
        expected_handle: handle,
        to_path: to_path.into(),
        to_id: to.into(),
        release_wait: Duration::from_secs(120),
    };
    join_switch(spawn_cold_switch(Arc::clone(&rig.lifecycle), prep, req).expect("authority")).await
}

/// A → B (with a request in flight on A) → A, on real weights.
async fn run_pair(base: &str, a_path: &str, b_path: &str) {
    let pair = format!("{}>{}", name(a_path), name(b_path));
    let rig = rig(base);
    note(&pair, "baseline", "");
    let loaded = crate::models::load_model_operated(
        &rig.lifecycle,
        &rig.loader,
        a_path,
        Some("a".into()),
        1,
        Duration::from_secs(30),
    )
    .await
    .expect("load A");
    let a1 = ask(&rig.engine, "a", 16).await.expect("A answers");
    note(
        &pair,
        "a_loaded",
        &format!(
            "handle={} budget={} out={}",
            loaded.handle_id,
            rig.limits.current_memory(),
            digest(&a1)
        ),
    );

    let engine = Arc::clone(&rig.engine);
    let in_flight = tokio::spawn(async move { ask(&engine, "a", 96).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let first = cold(&rig, "a", b_path, "b").await;
    let held = in_flight.await.expect("in-flight task");
    note(
        &pair,
        "switched_to_b",
        &format!(
            "outcome={first:?} budget={} inflight_ok={}",
            rig.limits.current_memory(),
            held.is_ok()
        ),
    );
    let SwitchOutcome::Switched { release_ms, .. } = first else {
        panic!("{first:?}")
    };
    // The switch waited for the in-flight request to release A: it really
    // was in flight when admissions closed (not finished before the unload).
    assert!(release_ms >= 100, "release waited only {release_ms} ms");
    assert!(
        held.is_ok(),
        "the in-flight request on A completed exactly once: {held:?}"
    );
    assert!(
        ask(&rig.engine, "a", 4).await.is_err(),
        "A is closed to new requests"
    );
    let b = ask(&rig.engine, "b", 16).await.expect("B answers");
    note(&pair, "b_serving", &format!("out={}", digest(&b)));

    let back = cold(&rig, "b", a_path, "a").await;
    assert!(matches!(back, SwitchOutcome::Switched { .. }), "{back:?}");
    let a2 = ask(&rig.engine, "a", 16).await.expect("A answers again");
    note(
        &pair,
        "back_to_a",
        &format!(
            "outcome={back:?} budget={} out={} same_as_first={}",
            rig.limits.current_memory(),
            digest(&a2),
            a1 == a2
        ),
    );
    assert_eq!(a1, a2, "greedy output of A is unchanged after A→B→A");
    rig.lifecycle.unload("a").await.expect("final unload");
    let released = rig
        .lifecycle
        .wait_released("a", Duration::from_secs(30))
        .await;
    note(
        &pair,
        "final_release",
        &format!(
            "released={released:?} budget={}",
            rig.limits.current_memory()
        ),
    );
    assert_eq!(released, (true, 0));
}

fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_model_cold_switch_a_b_a() {
    let (Ok(base), Ok(pairs)) = (
        std::env::var("GG_SWITCH_BASE"),
        std::env::var("GG_SWITCH_PAIRS"),
    ) else {
        eprintln!("SKIP: set GG_SWITCH_BASE and GG_SWITCH_PAIRS to run the real-model switch");
        return;
    };
    for pair in pairs.split(';').filter(|p| !p.is_empty()) {
        let (a, b) = pair.split_once('|').expect("relA|relB");
        run_pair(&base, a, b).await;
    }
}
