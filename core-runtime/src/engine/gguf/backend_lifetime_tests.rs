//! Regression: model lifetimes must not depend on each other's in-flight work.
//!
//! Before the process-wide backend, `LlamaBackendInner` owned the single
//! llama-cpp-2 `LlamaBackend`, so loading a model while any other instance was
//! still alive (a generation thread winding down after a cancelled stream, an
//! unload racing a reload) failed with `BackendAlreadyInitialized`. Model-gated:
//! early-returns without `models/qwen2.5-0.5b-instruct-q4_k_m.gguf`.

use super::LlamaBackendInner;
use crate::engine::gguf::GgufConfig;

fn fixture() -> Option<(std::path::PathBuf, GgufConfig)> {
    let path = std::path::PathBuf::from("../models/qwen2.5-0.5b-instruct-q4_k_m.gguf");
    // CI checks the repository out without git-lfs, so the path exists but
    // holds a pointer file: require the GGUF magic, not mere existence.
    let mut magic = [0u8; 4];
    let is_gguf = std::fs::File::open(&path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .is_ok()
        && &magic == b"GGUF";
    if !is_gguf {
        eprintln!("skip: real model fixture not present at {path:?}");
        return None;
    }
    let cfg = GgufConfig {
        n_ctx: 256,
        n_threads: 2,
        n_gpu_layers: 0,
    };
    Some((path, cfg))
}

#[test]
fn second_model_loads_while_first_is_still_alive() {
    let Some((path, cfg)) = fixture() else {
        return;
    };
    let first = LlamaBackendInner::load(&path, &cfg).expect("first load");
    // A live instance (e.g. a cancelled stream still holding its Arc) must not
    // block a new load: this is the unload → reload path CoreForge uses to
    // switch models.
    let second = LlamaBackendInner::load(&path, &cfg).expect("load while first alive");
    drop(first);
    let third = LlamaBackendInner::load(&path, &cfg).expect("load after first dropped");
    drop(second);
    assert!(third.model_size() > 0);
}

#[test]
fn loads_from_several_threads_share_one_backend() {
    let Some((path, cfg)) = fixture() else {
        return;
    };
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let (p, c) = (path.clone(), cfg.clone());
            std::thread::spawn(move || LlamaBackendInner::load(&p, &c).map(|m| m.model_size()))
        })
        .collect();
    for h in handles {
        assert!(
            h.join().expect("thread").is_ok(),
            "concurrent load must succeed"
        );
    }
}
