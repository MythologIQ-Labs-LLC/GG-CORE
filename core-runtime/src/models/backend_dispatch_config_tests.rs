//! The daemon's advertised context and thread env settings must actually
//! reach the native GGUF loader, not just its diagnostic output (and the
//! thread policy, HOL plan Addendum A).

use super::gguf_load_config;

#[test]
fn configured_context_and_threads_reach_the_model_loader() {
    let runtime = crate::config::GgufEnvConfig {
        n_ctx: 8192,
        n_threads: 6,
    };
    let load = gguf_load_config(&runtime, 4);
    assert_eq!(load.n_ctx, 8192);
    assert_eq!(load.n_threads, 6, "explicit override is not divided");
    assert_eq!(load.n_gpu_layers, 0);
}

#[test]
fn auto_threads_split_by_workers_and_embedded_is_unchanged() {
    use crate::engine::gguf::threads::per_context_for_host;
    let auto = crate::config::GgufEnvConfig::default();
    assert_eq!(
        gguf_load_config(&auto, 4).n_threads,
        per_context_for_host(0, 4)
    );
    let full = num_cpus::get().clamp(1, 16) as u32;
    assert_eq!(
        gguf_load_config(&auto, 1).n_threads,
        full,
        "embedded = today"
    );
}
