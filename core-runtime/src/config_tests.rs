//! Environment configuration tests (moved out of config.rs for the Section 4 Razor).

use super::*;
use std::sync::Mutex;

// Serialize env-mutating tests to avoid cross-test pollution.
static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    let cfg = load();
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
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    std::env::set_var("GG_CORE_IPC_FRAME_LIMIT", "33554432"); // 32 MiB
    std::env::set_var("GG_CORE_MAX_CONNECTIONS", "128");
    let cfg = load();
    assert_eq!(cfg.ipc_server.max_frame_size, 33_554_432);
    assert_eq!(cfg.connections.max_connections, 128);
    clear_env_vars();
}

#[test]
fn test_ipc_frame_limit_floor() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    std::env::set_var("GG_CORE_IPC_FRAME_LIMIT", "0");
    let cfg = load();
    assert!(
        cfg.ipc_server.max_frame_size >= 4096,
        "frame limit must have floor"
    );
    clear_env_vars();
}

#[test]
fn test_env_vars_override_defaults() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_CONTEXT", "8192");
    std::env::set_var("GG_CORE_MAX_QUEUE_DEPTH", "512");
    std::env::set_var("GG_CORE_MAX_CONCURRENT", "4");
    std::env::set_var("GG_CORE_SHUTDOWN_TIMEOUT", "60");
    std::env::set_var("GG_CORE_N_CTX", "4096");
    let cfg = load();
    assert_eq!(cfg.max_context_length, 8192);
    assert_eq!(cfg.request_queue.max_pending, 512);
    assert_eq!(cfg.resource_limits.max_concurrent, 4);
    assert_eq!(cfg.shutdown_timeout.as_secs(), 60);
    assert_eq!(cfg.gguf.n_ctx, 4096);
    clear_env_vars();
}

#[test]
fn test_invalid_env_falls_back_to_default() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_CONTEXT", "not_a_number");
    std::env::set_var("GG_CORE_MAX_QUEUE_DEPTH", "abc");
    std::env::set_var("GG_CORE_N_CTX", "xyz");
    let cfg = load();
    assert_eq!(cfg.max_context_length, 4096);
    assert_eq!(cfg.request_queue.max_pending, 256);
    assert_eq!(cfg.gguf.n_ctx, 2048);
    clear_env_vars();
}

#[test]
fn test_max_context_length_validation() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    std::env::set_var("GG_CORE_MAX_CONTEXT", "0");
    let cfg = load();
    assert!(
        cfg.max_context_length >= 1,
        "max_context_length must be > 0"
    );

    std::env::set_var("GG_CORE_MAX_CONTEXT", "9999999");
    let cfg = load();
    assert!(cfg.max_context_length <= 1_000_000, "must be clamped");
    clear_env_vars();
}

#[test]
fn test_effective_config_contains_all_fields() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    let cfg = load();
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
fn per_call_ceiling_is_none_unless_explicitly_set() {
    let _lock = ENV_LOCK.lock().unwrap();
    clear_env_vars();
    assert_eq!(per_call_ceiling_from_env(), None);
    assert_eq!(load().per_call_ceiling, None);
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "2147483648");
    assert_eq!(per_call_ceiling_from_env(), Some(2_147_483_648));
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "10");
    assert_eq!(
        per_call_ceiling_from_env(),
        Some(1024 * 1024),
        "1 MiB floor"
    );
    std::env::set_var("GG_CORE_MAX_MEMORY_PER_CALL", "not-a-number");
    assert_eq!(
        per_call_ceiling_from_env(),
        None,
        "unparsable: no ceiling (fallback default)"
    );
    clear_env_vars();
}
