//! The daemon's advertised context and thread env settings must actually
//! reach the native GGUF loader, not just its diagnostic output (and the
//! thread policy, HOL plan Addendums A and B).

use super::{gguf_load_config, load_model_dispatch, load_model_dispatch_for_workers};
use crate::config::GgufEnvConfig;
use crate::memory::{ResourceLimits, ResourceLimitsConfig};

const WORKERS: [usize; 4] = [1, 2, 4, 8];

#[test]
fn configured_context_and_threads_reach_the_model_loader() {
    let runtime = GgufEnvConfig {
        n_ctx: 8192,
        n_threads: 6,
    };
    let load = gguf_load_config(&runtime, 4);
    assert_eq!(load.n_ctx, 8192);
    assert_eq!(load.n_threads, 6, "explicit override is not divided");
    assert_eq!(load.n_gpu_layers, 0);
}

#[test]
fn daemon_auto_threads_are_cores_divided_by_workers() {
    // Written out independently of `threads.rs` so the policy is pinned.
    let cores = num_cpus::get().clamp(1, 16);
    let auto = GgufEnvConfig::default();
    for w in WORKERS {
        let want = u32::try_from((cores / w).max(1)).unwrap();
        assert_eq!(gguf_load_config(&auto, w).n_threads, want, "workers {w}");
    }
    let embedded = u32::try_from(cores).unwrap();
    assert_eq!(
        gguf_load_config(&auto, 1).n_threads,
        embedded,
        "embedded = pre-#179"
    );
}

#[test]
fn explicit_thread_override_is_honoured_for_every_worker_count() {
    let explicit = GgufEnvConfig {
        n_ctx: 2048,
        n_threads: 5,
    };
    for w in WORKERS {
        assert_eq!(gguf_load_config(&explicit, w).n_threads, 5, "workers {w}");
    }
}

#[test]
fn worker_loops_follow_max_concurrent() {
    assert_eq!(ResourceLimits::worker_loops(None), 1);
    for n in WORKERS {
        let limits = ResourceLimits::new(ResourceLimitsConfig {
            max_concurrent: n,
            ..ResourceLimitsConfig::default()
        });
        assert_eq!(ResourceLimits::worker_loops(Some(&limits)), n);
    }
}

#[test]
fn original_dispatch_is_the_single_context_variant() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent.gguf");
    let a = load_model_dispatch(&missing, "m")
        .err()
        .map(|e| e.to_string());
    let b = load_model_dispatch_for_workers(&missing, "m", 1)
        .err()
        .map(|e| e.to_string());
    assert!(a.is_some(), "missing model must fail");
    assert_eq!(a, b);
}
