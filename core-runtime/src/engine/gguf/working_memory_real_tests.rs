//! Measured acceptance gate (ship-blocking, run natively against real pinned
//! models): with flash attention forced OFF, the private-bytes peak of one
//! context creation + prefill must not exceed `working_memory_estimate`.
//!
//! `GG_CORE_WM_MODELS` = `;`-separated GGUF paths; `GG_CORE_WM_CTX` =
//! `;`-separated context lengths (default 4096). Run single-threaded:
//! `cargo test --features gguf -- --ignored --test-threads=1 working_memory`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use super::super::super::{GgufConfig, LlamaBackendInner};
use crate::engine::InferenceConfig;

#[repr(C)]
#[derive(Default)]
struct ProcessMemoryCountersEx {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(
        process: isize,
        counters: *mut ProcessMemoryCountersEx,
        cb: u32,
    ) -> i32;
}

fn private_bytes() -> usize {
    let mut c = ProcessMemoryCountersEx {
        cb: std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
        ..Default::default()
    };
    // SAFETY: valid out-pointer of the declared size; pseudo-handle needs no close.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    assert_ne!(ok, 0, "K32GetProcessMemoryInfo failed");
    c.private_usage
}

/// Peak private bytes above the pre-call baseline while `work` runs.
fn peak_delta(work: impl FnOnce()) -> usize {
    let baseline = private_bytes();
    let (peak, stop) = (
        Arc::new(AtomicUsize::new(baseline)),
        Arc::new(AtomicBool::new(false)),
    );
    let sampler = {
        let (peak, stop) = (Arc::clone(&peak), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                peak.fetch_max(private_bytes(), Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        })
    };
    work();
    peak.fetch_max(private_bytes(), Ordering::SeqCst);
    stop.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    peak.load(Ordering::SeqCst).saturating_sub(baseline)
}

fn measure(path: &str, n_ctx: u32, flash_attn_disabled: bool) -> (usize, usize) {
    let config = GgufConfig {
        n_ctx,
        n_threads: 4,
        n_gpu_layers: 0,
    };
    let mut backend =
        LlamaBackendInner::load(std::path::Path::new(path), &config).expect("model loads");
    backend.shape.flash_attn_disabled = flash_attn_disabled;
    // ~900 tokens: more than one 512-token micro-batch at every tested n_ctx.
    let prompt = "The brown fox walks beside a clear river under a quiet blue sky. ".repeat(60);
    let cfg = InferenceConfig {
        max_tokens: Some(1),
        ..Default::default()
    };
    let delta = peak_delta(|| {
        backend.generate(&prompt, &cfg).expect("prefill + 1 token");
    });
    (delta, backend.working_memory_estimate())
}

fn contexts() -> Vec<u32> {
    let raw = std::env::var("GG_CORE_WM_CTX").unwrap_or_else(|_| "4096".into());
    raw.split(';')
        .filter_map(|c| c.trim().parse().ok())
        .collect()
}

#[test]
#[ignore = "real models; run natively with GG_CORE_WM_MODELS"]
fn flash_attention_off_peak_stays_within_estimate() {
    let models = std::env::var("GG_CORE_WM_MODELS").expect("set GG_CORE_WM_MODELS");
    for path in models.split(';').filter(|p| !p.is_empty()) {
        let mut previous = 0;
        for n_ctx in contexts() {
            let (auto_delta, est) = measure(path, n_ctx, false);
            let (off_delta, _) = measure(path, n_ctx, true);
            eprintln!("WM {path} n_ctx={n_ctx}: estimate={est} fa_auto_peak={auto_delta} fa_off_peak={off_delta}");
            assert!(
                auto_delta <= est,
                "{path}@{n_ctx}: auto peak {auto_delta} > estimate {est}"
            );
            assert!(
                off_delta <= est,
                "{path}@{n_ctx}: flash-off peak {off_delta} > estimate {est}"
            );
            assert_ne!(est, usize::MAX, "{path}: pinned model must be boundable");
            assert!(est > previous, "{path}: estimate must grow with n_ctx");
            previous = est;
        }
    }
}
