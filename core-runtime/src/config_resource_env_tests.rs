//! Strict resource environment overrides (#194), separate from legacy config tests.

use super::*;
use crate::test_env::env_guard;

const KEYS: &[&str] = &[
    "GG_CORE_MAX_CONTEXT",
    "GG_CORE_MAX_QUEUE_DEPTH",
    "GG_CORE_MAX_CONTEXT_TOKENS",
    "GG_CORE_MAX_MEMORY_PER_CALL",
    "GG_CORE_MAX_TOTAL_MEMORY",
    "GG_CORE_MAX_CONCURRENT",
    "GG_CORE_BATCH_MAX_REQUESTS",
    "GG_CORE_BATCH_MAX_TOKENS",
    "GG_CORE_SHUTDOWN_TIMEOUT",
    "GG_CORE_SESSION_TIMEOUT",
    "GG_CORE_N_CTX",
    "GG_CORE_N_THREADS",
    "GG_CORE_N_GPU_LAYERS",
    "GG_CORE_IPC_FRAME_LIMIT",
    "GG_CORE_MAX_CONNECTIONS",
    "GG_CORE_STREAM_STALL_MS",
];

fn clear_env() {
    for key in KEYS {
        std::env::remove_var(key);
    }
}

#[test]
fn invalid_explicit_resource_values_fail_closed() {
    let _lock = env_guard();
    clear_env();
    for var in [
        "GG_CORE_MAX_CONTEXT",
        "GG_CORE_MAX_QUEUE_DEPTH",
        "GG_CORE_MAX_CONTEXT_TOKENS",
        "GG_CORE_MAX_CONCURRENT",
        "GG_CORE_BATCH_MAX_REQUESTS",
        "GG_CORE_BATCH_MAX_TOKENS",
        "GG_CORE_SHUTDOWN_TIMEOUT",
        "GG_CORE_SESSION_TIMEOUT",
        "GG_CORE_N_CTX",
        "GG_CORE_N_THREADS",
        "GG_CORE_N_GPU_LAYERS",
        "GG_CORE_IPC_FRAME_LIMIT",
        "GG_CORE_MAX_CONNECTIONS",
        "GG_CORE_STREAM_STALL_MS",
    ] {
        std::env::set_var(var, "not_a_number");
        let err = try_load().unwrap_err();
        assert!(matches!(err, ConfigError::Resource(_)), "{var}: {err}");
        std::env::remove_var(var);
    }
    clear_env();
}

#[test]
fn explicit_resource_boundaries_are_not_silently_clamped() {
    let _lock = env_guard();
    clear_env();
    for (var, value) in [
        ("GG_CORE_MAX_CONTEXT", "0"),
        ("GG_CORE_MAX_CONTEXT", "1000001"),
        ("GG_CORE_MAX_CONCURRENT", "0"),
        ("GG_CORE_MAX_QUEUE_DEPTH", "0"),
        ("GG_CORE_N_CTX", "127"),
        ("GG_CORE_N_GPU_LAYERS", "2147483648"),
        ("GG_CORE_IPC_FRAME_LIMIT", "4095"),
        ("GG_CORE_STREAM_STALL_MS", "99"),
        ("GG_CORE_SHUTDOWN_TIMEOUT", "0"),
        ("GG_CORE_SESSION_TIMEOUT", "0"),
    ] {
        std::env::set_var(var, value);
        let err = try_load().unwrap_err();
        assert!(
            matches!(err, ConfigError::Resource(_)),
            "{var}={value}: {err}"
        );
        std::env::remove_var(var);
    }
    clear_env();
}

#[test]
fn valid_lower_bound_values_are_preserved_exactly() {
    let _lock = env_guard();
    clear_env();
    for (var, value) in [
        ("GG_CORE_MAX_CONTEXT", "1"),
        ("GG_CORE_MAX_QUEUE_DEPTH", "1"),
        ("GG_CORE_MAX_CONTEXT_TOKENS", "1"),
        ("GG_CORE_MAX_CONCURRENT", "1"),
        ("GG_CORE_BATCH_MAX_REQUESTS", "1"),
        ("GG_CORE_BATCH_MAX_TOKENS", "1"),
        ("GG_CORE_SHUTDOWN_TIMEOUT", "1"),
        ("GG_CORE_SESSION_TIMEOUT", "1"),
        ("GG_CORE_N_CTX", "128"),
        ("GG_CORE_N_THREADS", "0"),
        ("GG_CORE_N_GPU_LAYERS", "0"),
        ("GG_CORE_IPC_FRAME_LIMIT", "4096"),
        ("GG_CORE_MAX_CONNECTIONS", "1"),
        ("GG_CORE_STREAM_STALL_MS", "100"),
    ] {
        std::env::set_var(var, value);
    }
    let cfg = try_load().expect("documented explicit minima must remain valid");
    assert_eq!(cfg.max_context_length, 1);
    assert_eq!(cfg.request_queue.max_pending, 1);
    assert_eq!(cfg.request_queue.max_context_tokens, 1);
    assert_eq!(cfg.resource_limits.max_concurrent, 1);
    assert_eq!(cfg.batch.max_batch_size, 1);
    assert_eq!(cfg.batch.max_total_tokens, 1);
    assert_eq!(cfg.shutdown_timeout.as_secs(), 1);
    assert_eq!(cfg.session_timeout.as_secs(), 1);
    assert_eq!(cfg.gguf.n_ctx, 128);
    assert_eq!(cfg.gguf.n_threads, 0);
    assert_eq!(cfg.gguf.n_gpu_layers, 0);
    assert_eq!(cfg.ipc_server.max_frame_size, 4096);
    assert_eq!(cfg.connections.max_connections, 1);
    assert_eq!(cfg.ipc_server.stream_stall.as_millis(), 100);
    clear_env();
}

#[test]
fn explicit_threads_must_fit_the_native_gguf_thread_integer() {
    let _lock = env_guard();
    clear_env();
    std::env::set_var("GG_CORE_N_THREADS", "2147483648");
    assert!(matches!(try_load(), Err(ConfigError::Resource(_))));
    assert!(matches!(load_gguf_config(), Err(ConfigError::Resource(_))));
    std::env::set_var("GG_CORE_N_THREADS", "2147483647");
    assert_eq!(load_gguf_config().unwrap().n_threads, i32::MAX as u32);
    clear_env();
}

#[test]
fn ipc_frame_limit_must_fit_the_wire_format_length_prefix() {
    let _lock = env_guard();
    clear_env();
    std::env::set_var("GG_CORE_IPC_FRAME_LIMIT", "4294967296");
    assert!(matches!(try_load(), Err(ConfigError::Resource(_))));
    std::env::set_var("GG_CORE_IPC_FRAME_LIMIT", "4294967295");
    assert_eq!(
        try_load().unwrap().ipc_server.max_frame_size,
        u32::MAX as usize
    );
    clear_env();
}

#[test]
fn gguf_config_refuses_invalid_explicit_settings() {
    let _lock = env_guard();
    clear_env();
    std::env::set_var("GG_CORE_N_THREADS", "invalid");
    assert!(matches!(load_gguf_config(), Err(ConfigError::Resource(_))));
    std::env::set_var("GG_CORE_N_THREADS", "0");
    assert_eq!(load_gguf_config().unwrap().n_threads, 0);
    std::env::set_var("GG_CORE_N_THREADS", "5");
    assert_eq!(load_gguf_config().unwrap().n_threads, 5);
    clear_env();
}

#[test]
fn load_panics_on_invalid_resource_setting() {
    let _lock = env_guard();
    clear_env();
    std::env::set_var("GG_CORE_MAX_CONCURRENT", "0");
    let result = std::panic::catch_unwind(load);
    clear_env();
    assert!(
        result.is_err(),
        "load() must not silently clamp an invalid setting"
    );
}

#[test]
fn explicit_gpu_offload_refuses_cpu_only_build() {
    let _lock = env_guard();
    clear_env();
    std::env::set_var("GG_CORE_N_GPU_LAYERS", "1");
    let result = load_gguf_config();
    if cfg!(any(
        feature = "gguf-cuda",
        all(feature = "gguf-metal", target_os = "macos")
    )) {
        assert_eq!(result.unwrap().n_gpu_layers, 1);
    } else {
        assert!(
            matches!(result, Err(ConfigError::Resource(_))),
            "a CPU-only build must refuse requested native GPU offload"
        );
    }
    clear_env();
}

#[test]
fn gpu_layer_count_beyond_native_signed_range_fails_closed() {
    let _lock = env_guard();
    clear_env();
    std::env::set_var("GG_CORE_N_GPU_LAYERS", "2147483648");
    assert!(matches!(load_gguf_config(), Err(ConfigError::Resource(_))));
    clear_env();
}
