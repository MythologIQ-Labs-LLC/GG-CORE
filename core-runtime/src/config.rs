//! Runtime configuration loading from environment variables.
//!
//! All configuration values are loaded from `GG_CORE_*` environment variables
//! with sensible defaults. Invalid values fall back to defaults without crashing.
//!
//! # Environment Variables
//!
//! | Variable | Default | Description |
//! |---|---|---|
//! | `GG_CORE_MAX_CONTEXT` | 4096 | Max context length (tokens) |
//! | `GG_CORE_MAX_QUEUE_DEPTH` | 256 | Max pending requests |
//! | `GG_CORE_MAX_CONTEXT_TOKENS` | 4096 | Max context tokens per request |
//! | `GG_CORE_MAX_MEMORY_PER_CALL` | 1073741824 | Max memory per call (bytes) |
//! | `GG_CORE_MAX_TOTAL_MEMORY` | 2147483648 | Max total memory (bytes) |
//! | `GG_CORE_MAX_CONCURRENT` | 2 | Max concurrent requests |
//! | `GG_CORE_BATCH_MAX_REQUESTS` | 8 | Max requests per batch |
//! | `GG_CORE_BATCH_MAX_TOKENS` | 4096 | Max tokens per batch |
//! | `GG_CORE_SHUTDOWN_TIMEOUT` | 30 | Graceful shutdown timeout (secs) |
//! | `GG_CORE_SESSION_TIMEOUT` | 3600 | Session timeout (secs) |
//! | `GG_CORE_N_CTX` | 2048 | GGUF context window size |
//! | `GG_CORE_N_THREADS` | 0 | Inference threads (0 = auto) |
//! | `GG_CORE_IPC_FRAME_LIMIT` | 16777216 | Max IPC frame size (bytes) |
//! | `GG_CORE_MAX_CONNECTIONS` | 64 | Max concurrent IPC connections |

use std::path::PathBuf;
use std::time::Duration;

use crate::ipc::{ConnectionConfig, IpcServerConfig};
use crate::memory::ResourceLimitsConfig;
use crate::scheduler::{BatchConfig, RequestQueueConfig};

/// Effective runtime configuration summary (serializable).
#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    pub max_context_length: usize,
    pub queue_max_pending: usize,
    pub queue_max_context_tokens: usize,
    pub resource_max_memory_per_call: usize,
    pub resource_max_total_memory: usize,
    pub resource_max_concurrent: usize,
    pub batch_max_requests: usize,
    pub batch_max_tokens: usize,
    pub shutdown_timeout_secs: u64,
    pub session_timeout_secs: u64,
    pub n_ctx: u32,
    pub n_threads: u32,
    pub ipc_frame_limit: usize,
    pub max_connections: usize,
}

/// GGUF-specific configuration loaded from env.
#[derive(Debug, Clone)]
pub struct GgufEnvConfig {
    pub n_ctx: u32,
    pub n_threads: u32,
}

impl Default for GgufEnvConfig {
    fn default() -> Self {
        Self { n_ctx: 2048, n_threads: 0 }
    }
}

/// All runtime configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct EnvConfig {
    pub base_path: PathBuf,
    pub auth_token: String,
    pub max_context_length: usize,
    pub request_queue: RequestQueueConfig,
    pub resource_limits: ResourceLimitsConfig,
    pub batch: BatchConfig,
    pub shutdown_timeout: Duration,
    pub session_timeout: Duration,
    pub gguf: GgufEnvConfig,
    pub ipc_server: IpcServerConfig,
    pub connections: ConnectionConfig,
}

/// Parse a `usize` env var, returning `default` on missing or invalid.
fn parse_usize(key: &str, default: usize) -> usize {
    match std::env::var(key) {
        Ok(val) => val.parse::<usize>().unwrap_or(default),
        Err(_) => default,
    }
}

/// Parse a `u32` env var, returning `default` on missing or invalid.
fn parse_u32(key: &str, default: u32) -> u32 {
    match std::env::var(key) {
        Ok(val) => val.parse::<u32>().unwrap_or(default),
        Err(_) => default,
    }
}

/// Parse a `u64` env var, returning `default` on missing or invalid.
fn parse_u64(key: &str, default: u64) -> u64 {
    match std::env::var(key) {
        Ok(val) => val.parse::<u64>().unwrap_or(default),
        Err(_) => default,
    }
}

/// Load queue configuration from environment.
fn load_queue_config() -> RequestQueueConfig {
    let max_pending = parse_usize("GG_CORE_MAX_QUEUE_DEPTH", 256);
    let max_context_tokens = parse_usize("GG_CORE_MAX_CONTEXT_TOKENS", 4096);
    let max_pending = max_pending.max(1);
    let max_context_tokens = max_context_tokens.max(1);
    RequestQueueConfig { max_pending, max_context_tokens }
}

/// Load resource limits configuration from environment.
fn load_resource_limits() -> ResourceLimitsConfig {
    let per_call = parse_usize("GG_CORE_MAX_MEMORY_PER_CALL", 1024 * 1024 * 1024);
    let total = parse_usize("GG_CORE_MAX_TOTAL_MEMORY", 2 * 1024 * 1024 * 1024);
    let concurrent = parse_usize("GG_CORE_MAX_CONCURRENT", 2);
    let per_call = per_call.max(1024 * 1024); // floor: 1MB
    let total = total.max(per_call);          // total >= per_call
    let concurrent = concurrent.max(1);
    ResourceLimitsConfig {
        max_memory_per_call: per_call,
        max_total_memory: total,
        max_concurrent: concurrent,
    }
}

/// Load batch configuration from environment.
fn load_batch_config() -> BatchConfig {
    let max_batch_size = parse_usize("GG_CORE_BATCH_MAX_REQUESTS", 8);
    let max_total_tokens = parse_usize("GG_CORE_BATCH_MAX_TOKENS", 4096);
    let max_batch_size = max_batch_size.max(1);
    let max_total_tokens = max_total_tokens.max(1);
    BatchConfig { max_batch_size, max_total_tokens }
}

/// Load GGUF engine configuration from environment.
fn load_gguf_config() -> GgufEnvConfig {
    let n_ctx = parse_u32("GG_CORE_N_CTX", 2048);
    let n_threads = parse_u32("GG_CORE_N_THREADS", 0);
    let n_ctx = n_ctx.max(128); // floor: 128 tokens minimum
    GgufEnvConfig { n_ctx, n_threads }
}

/// Load IPC server configuration from environment.
fn load_ipc_server_config() -> IpcServerConfig {
    const DEFAULT_FRAME: usize = 16 * 1024 * 1024; // 16 MiB
    const MIN_FRAME: usize = 4096; // floor: 4 KiB
    let max_frame_size = parse_usize("GG_CORE_IPC_FRAME_LIMIT", DEFAULT_FRAME);
    let max_frame_size = max_frame_size.max(MIN_FRAME);
    IpcServerConfig { max_frame_size }
}

/// Load connection pool configuration from environment.
fn load_connection_config() -> ConnectionConfig {
    let max_connections = parse_usize("GG_CORE_MAX_CONNECTIONS", 64);
    let max_connections = max_connections.max(1);
    ConnectionConfig { max_connections }
}

/// Load all configuration from environment variables.
///
/// Missing or invalid values fall back to safe defaults without panicking.
pub fn load() -> EnvConfig {
    let auth_token = std::env::var("CORE_AUTH_TOKEN").unwrap_or_default();
    let max_context_length = parse_usize("GG_CORE_MAX_CONTEXT", 4096);
    let max_context_length = max_context_length.clamp(1, 1_000_000);
    let shutdown_secs = parse_u64("GG_CORE_SHUTDOWN_TIMEOUT", 30);
    let session_secs = parse_u64("GG_CORE_SESSION_TIMEOUT", 3600);
    let shutdown_secs = shutdown_secs.max(1);
    let session_secs = session_secs.max(1);

    EnvConfig {
        base_path: PathBuf::from("."),
        auth_token,
        max_context_length,
        request_queue: load_queue_config(),
        resource_limits: load_resource_limits(),
        batch: load_batch_config(),
        shutdown_timeout: Duration::from_secs(shutdown_secs),
        session_timeout: Duration::from_secs(session_secs),
        gguf: load_gguf_config(),
        ipc_server: load_ipc_server_config(),
        connections: load_connection_config(),
    }
}

impl EnvConfig {
    /// Return a serializable summary of all effective values.
    pub fn effective_config(&self) -> EffectiveConfig {
        EffectiveConfig {
            max_context_length: self.max_context_length,
            queue_max_pending: self.request_queue.max_pending,
            queue_max_context_tokens: self.request_queue.max_context_tokens,
            resource_max_memory_per_call: self.resource_limits.max_memory_per_call,
            resource_max_total_memory: self.resource_limits.max_total_memory,
            resource_max_concurrent: self.resource_limits.max_concurrent,
            batch_max_requests: self.batch.max_batch_size,
            batch_max_tokens: self.batch.max_total_tokens,
            shutdown_timeout_secs: self.shutdown_timeout.as_secs(),
            session_timeout_secs: self.session_timeout.as_secs(),
            n_ctx: self.gguf.n_ctx,
            n_threads: self.gguf.n_threads,
            ipc_frame_limit: self.ipc_server.max_frame_size,
            max_connections: self.connections.max_connections,
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(cfg.resource_limits.max_total_memory, 2 * 1024 * 1024 * 1024);
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
        assert!(cfg.ipc_server.max_frame_size >= 4096, "frame limit must have floor");
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
        assert!(cfg.max_context_length >= 1, "max_context_length must be > 0");

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
}
