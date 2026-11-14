//! Environment configuration tests (moved out of config.rs for the Section 4 Razor).

use super::*;
use crate::test_env::env_guard;

const ENV_KEYS: &[&str] = &[
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
    "GG_CORE_IPC_FRAME_LIMIT",
    "GG_CORE_MAX_CONNECTIONS",
    "CORE_AUTH_TOKEN",
];

fn clear_env_vars() {
    for k in ENV_KEYS {
        std::env::remove_var(k);
    }
}

#[test]
fn test_defaults_are_sensible() {
    let _lock = env_guard();
    clear_env_vars();
    let cfg = try_load().unwrap();
    assert_eq!(cfg.max_context_length, 4096);
    assert_eq!(cfg.request_queue.max_pending, 256);
    assert_eq!(cfg.request_queue.max_context_tokens, 4096);
    assert_eq!(cfg.resource_limits.max_memory_per_call, 1024 * 1024 * 1024);
    assert_eq!(
        cfg.resource_limits.max_total_memory,
        crate::memory::host::default_total_memory()
    );
    assert!(cfg.resource_limits.max_total_memory >= 2 * 1024 * 1024 * 1024);
    assert_eq!(cfg.resource_limits.max_concurrent, 2);
    assert_eq!(cfg.batch.max_batch_size, 8);
    assert_eq!(cfg.batch.max_total_tokens, 4096);
    assert_eq!(cfg.shutdown_timeout.as_secs(), 30);
    assert_eq!(cfg.session_timeout.as_secs(), 3600);
    assert_eq!(cfg.gguf.n_ctx, 2048);
    assert_eq!(cfg.gguf.n_threads, 0);
    assert_eq!(cfg.ipc_server.max_frame_size, 16 * 1024 * 1024);
    assert_eq!(cfg.connections.max_connections, 64);
}

#[test]
fn test_ipc_frame_limit_override() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_IPC_FRAME_LIMIT", "33554432"); // 32 MiB
    std::env::set_var("GG_CORE_MAX_CONNECTIONS", "128");
    let cfg = try_load().unwrap();
    assert_eq!(cfg.ipc_server.max_frame_size, 33_554_432);
    assert_eq!(cfg.connections.max_connections, 128);
    clear_env_vars();
}

#[test]
fn test_ipc_frame_limit_floor() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_IPC_FRAME_LIMIT", "0");
    let cfg = try_load().unwrap();
    assert!(
        cfg.ipc_server.max_frame_size >= 4096,
        "frame limit must have floor"
    );
    clear_env_vars();
}

#[test]
fn test_env_vars_override_defaults() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_CONTEXT", "8192");
    std::env::set_var("GG_CORE_MAX_QUEUE_DEPTH", "512");
    std::env::set_var("GG_CORE_MAX_CONCURRENT", "4");
    std::env::set_var("GG_CORE_SHUTDOWN_TIMEOUT", "60");
    std::env::set_var("GG_CORE_N_CTX", "4096");
    let cfg = try_load().unwrap();
    assert_eq!(cfg.max_context_length, 8192);
    assert_eq!(cfg.request_queue.max_pending, 512);
    assert_eq!(cfg.resource_limits.max_concurrent, 4);
    assert_eq!(cfg.shutdown_timeout.as_secs(), 60);
    assert_eq!(cfg.gguf.n_ctx, 4096);
    clear_env_vars();
}

#[test]
fn test_invalid_env_falls_back_to_default() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_CONTEXT", "not_a_number");
    std::env::set_var("GG_CORE_MAX_QUEUE_DEPTH", "abc");
    std::env::set_var("GG_CORE_N_CTX", "xyz");
    let cfg = try_load().unwrap();
    assert_eq!(cfg.max_context_length, 4096);
    assert_eq!(cfg.request_queue.max_pending, 256);
    assert_eq!(cfg.gguf.n_ctx, 2048);
    clear_env_vars();
}

#[test]
fn test_max_context_length_validation() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_CONTEXT", "0");
    let cfg = try_load().unwrap();
    assert!(
        cfg.max_context_length >= 1,
        "max_context_length must be > 0"
    );

    std::env::set_var("GG_CORE_MAX_CONTEXT", "9999999");
    let cfg = try_load().unwrap();
    assert!(cfg.max_context_length <= 1_000_000, "must be clamped");
    clear_env_vars();
}

#[test]
fn test_effective_config_contains_all_fields() {
    let _lock = env_guard();
    clear_env_vars();
    let cfg = try_load().unwrap();
    let eff = cfg.effective_config();
    assert!(eff.max_context_length > 0);
    assert!(eff.queue_max_pending > 0);
    assert!(eff.queue_max_context_tokens > 0);
    assert!(eff.resource_max_memory_per_call > 0);
    assert!(eff.resource_max_total_memory >= eff.resource_max_memory_per_call);
    assert!(eff.resource_max_concurrent > 0);
    assert!(eff.batch_max_requests > 0);
    assert!(eff.batch_max_tokens > 0);
    assert!(eff.shutdown_timeout_secs > 0);
    assert!(eff.session_timeout_secs > 0);
    assert!(eff.n_ctx >= 128);
    assert!(eff.ipc_frame_limit > 0);
    assert!(eff.max_connections > 0);
}

#[test]
fn explicit_memory_values_are_strict_issue_182() {
    let _lock = env_guard();
    clear_env_vars();
    assert_eq!(try_load().unwrap().per_call_ceiling, None);
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "2147483648");
    assert_eq!(try_load().unwrap().per_call_ceiling, Some(2_147_483_648));
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "10");
    let below = try_load().unwrap_err();
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "not-a-number");
    let invalid = try_load().unwrap_err();
    clear_env_vars();
    assert!(matches!(
        below,
        ConfigError::Memory(MemoryConfigError::BelowMinimum { .. })
    ));
    assert!(matches!(
        invalid,
        ConfigError::Memory(MemoryConfigError::Invalid { .. })
    ));
}

#[test]
fn try_load_total_only_is_exact() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_TOTAL_MEMORY", "268435456");
    let cfg = try_load().unwrap();
    clear_env_vars();
    assert_eq!(cfg.resource_limits.max_total_memory, 268_435_456);
    assert_eq!(cfg.resource_limits.max_memory_per_call, 268_435_456);
    assert_eq!(cfg.per_call_ceiling, None);
}

#[test]
fn load_panics_on_invalid_memory_setting() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_TOTAL_MEMORY", "0");
    let result = std::panic::catch_unwind(load);
    clear_env_vars();
    assert!(result.is_err(), "load() must not substitute a limit");
}

#[test]
#[allow(deprecated)]
fn deprecated_per_call_ceiling_is_strict() {
    let _lock = env_guard();
    clear_env_vars();
    let unset = per_call_ceiling_from_env();
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "5242880");
    let set = per_call_ceiling_from_env();
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "abc");
    let invalid = std::panic::catch_unwind(per_call_ceiling_from_env);
    clear_env_vars();
    assert_eq!(unset, None);
    assert_eq!(set, Some(5_242_880));
    assert!(invalid.is_err(), "invalid must not read as no ceiling");
}

/// HOL plan Addendum B: an explicit `GG_CORE_N_THREADS` reaches the GGUF
/// config and is used per context, not divided by the worker count.
#[test]
fn explicit_thread_env_override_is_honoured() {
    let _lock = env_guard();
    clear_env_vars();
    std::env::set_var("GG_CORE_N_THREADS", "5");
    let n_threads = load_gguf_config().n_threads;
    std::env::remove_var("GG_CORE_N_THREADS");
    assert_eq!(n_threads, 5);
    for workers in [1, 2, 4, 8] {
        let per = crate::engine::gguf::threads::per_context_for_host(n_threads, workers);
        assert_eq!(per, 5, "workers {workers}");
    }
}
