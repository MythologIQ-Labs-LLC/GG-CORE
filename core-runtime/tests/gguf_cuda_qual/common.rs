//! Shared helpers for the native CUDA GGUF qualification tests.
//!
//! Every measurement is printed as one `GPUQUAL <key>=<value>` line so the
//! job log is the evidence. VRAM comes from `nvidia-smi` (path in
//! `GG_NVSMI`), never from GG-CORE's own claims.

#![allow(dead_code)]

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use gg_core::engine::gguf::{GgufConfig, GgufGenerator};
use gg_core::engine::{InferenceConfig, StreamItem, TokenStream};

pub const MODEL: &str = "../models/qwen2.5-0.5b-instruct-q4_k_m.gguf";

const APPROVED_MODEL_BYTES: u64 = 491_400_032;
const APPROVED_MODEL_SHA256: &str =
    "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db";

/// Enforce the approved real-model fixture, not merely the filename or an LFS
/// pointer. A wrong fixture must never produce GPU qualification evidence.
fn verify_fixture(path: &Path, expected_bytes: u64, expected_sha256: &str) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("cannot inspect pinned GGUF fixture: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != expected_bytes {
        return Err(format!(
            "pinned GGUF fixture is not a regular file of {expected_bytes} bytes"
        ));
    }
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open pinned GGUF fixture: {e}"))?;
    let mut digest = Sha256::new();
    let mut buf = [0_u8; 65_536];
    loop {
        let count = file
            .read(&mut buf)
            .map_err(|e| format!("cannot hash pinned GGUF fixture: {e}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buf[..count]);
    }
    let actual = hex::encode(digest.finalize());
    if actual != expected_sha256 {
        return Err(format!(
            "GGUF fixture SHA-256 mismatch: expected {expected_sha256}, got {actual}"
        ));
    }
    Ok(())
}

/// More than Qwen2.5-0.5B's 24 transformer blocks: offload everything.
pub const ALL_LAYERS: u32 = 99;

pub fn report(key: &str, value: impl std::fmt::Display) {
    println!("GPUQUAL {key}={value}");
}

pub fn load(n_gpu_layers: u32, n_ctx: u32) -> Result<GgufGenerator, String> {
    verify_fixture(
        Path::new(MODEL),
        APPROVED_MODEL_BYTES,
        APPROVED_MODEL_SHA256,
    )?;
    let config = GgufConfig {
        n_ctx,
        n_threads: 4,
        n_gpu_layers,
    };
    GgufGenerator::load("qwen".into(), Path::new(MODEL), &config).map_err(|e| e.to_string())
}

pub fn greedy(max_tokens: u32) -> InferenceConfig {
    InferenceConfig {
        max_tokens: Some(max_tokens),
        temperature: 0.0,
        top_k: 1,
        ..Default::default()
    }
}

/// Aggregate VRAM on the explicitly selected GPU UUID, not assumed GPU 0.
/// This is diagnostic host telemetry, NOT per-process allocation proof.
pub fn vram_mib() -> Option<u64> {
    let smi = std::env::var("GG_NVSMI").ok()?;
    let gpu_uuid = std::env::var("GG_GPU_DEVICE_UUID").ok()?;
    if !gpu_uuid.starts_with("GPU-") {
        return None;
    }
    let out = std::process::Command::new(smi)
        .args([
            "--query-gpu=memory.used",
            "--format=csv,noheader,nounits",
            "-i",
            gpu_uuid.as_str(),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// One streamed generation: (text, tokens, first-token latency, total time).
pub struct StreamRun {
    pub ids: Vec<u32>,
    pub tokens: usize,
    pub ttft: Duration,
    pub total: Duration,
    pub result: Result<(), String>,
    /// True only if the native backend actually observed the stop predicate.
    pub stop_observed: bool,
}

impl StreamRun {
    /// Decode throughput after the first token, in tokens per second.
    pub fn decode_tps(&self) -> f64 {
        let decode = self.total.saturating_sub(self.ttft).as_secs_f64();
        (self.tokens.saturating_sub(1)) as f64 / decode.max(1e-9)
    }
}

/// Stream `prompt`; `stop_after` cancels cooperatively after that many tokens.
pub fn stream(
    model: std::sync::Arc<GgufGenerator>,
    prompt: &str,
    config: InferenceConfig,
    stop_after: Option<usize>,
) -> StreamRun {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(stream_in_runtime(
        model,
        prompt.to_string(),
        config,
        stop_after,
    ))
}

async fn stream_in_runtime(
    model: std::sync::Arc<GgufGenerator>,
    prompt: String,
    config: InferenceConfig,
    stop_after: Option<usize>,
) -> StreamRun {
    let (tx, mut rx) = TokenStream::new(64, Default::default());
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&seen);
    let stop_observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_by_backend = std::sync::Arc::clone(&stop_observed);
    let start = Instant::now();
    let task = tokio::task::spawn_blocking(move || {
        let stop = move || {
            let requested = stop_after.is_some_and(|n| counter.load(ORD) >= n);
            if requested {
                observed_by_backend.store(true, ORD);
            }
            requested
        };
        model.generate_stream(&prompt, &config, &tx, Some(&stop), None)
    });
    let (mut ids, mut ttft) = (Vec::new(), None);
    while let Some(item) = rx.next().await {
        match item {
            StreamItem::Token(id) => {
                ids.push(id);
                seen.store(ids.len(), ORD);
                ttft.get_or_insert_with(|| start.elapsed());
            }
            StreamItem::Text(_) => {}
            StreamItem::End(_) => break,
        }
    }
    let result = task.await.expect("joined").map_err(|e| e.to_string());
    let total = start.elapsed();
    let tokens = ids.len();
    let ttft = ttft.unwrap_or(total);
    StreamRun {
        ids,
        tokens,
        ttft,
        total,
        result,
        stop_observed: stop_observed.load(ORD),
    }
}

const ORD: std::sync::atomic::Ordering = std::sync::atomic::Ordering::SeqCst;

#[cfg(test)]
mod fixture_integrity_tests {
    use super::*;

    #[test]
    fn pinned_fixture_check_rejects_wrong_bytes_and_wrong_hash() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("model.gguf");
        std::fs::write(&path, b"fixture").expect("test fixture");
        let approved = hex::encode(Sha256::digest(b"fixture"));
        assert!(verify_fixture(&path, 7, &approved).is_ok());
        assert!(verify_fixture(&path, 8, &approved).is_err());
        assert!(verify_fixture(&path, 7, &"0".repeat(64)).is_err());
    }
}
