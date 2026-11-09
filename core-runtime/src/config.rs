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
//! | `GG_CORE_MAX_MEMORY_PER_CALL` | unset | Optional per-request working-memory ceiling (bytes). Unset: a request reserves its model's metadata-derived need, bounded only by the total; models without an estimate reserve a 1 GiB fallback (or this value when set) |
//! | `GG_CORE_MAX_TOTAL_MEMORY` | max(2 GiB, 50% RAM) | Resident weights + working memory; Job Object cap (bytes) |
//! | `GG_CORE_MAX_CONCURRENT` | 2 | Max concurrent requests |
//! | `GG_CORE_BATCH_MAX_REQUESTS` | 8 | Max requests per batch |
//! | `GG_CORE_BATCH_MAX_TOKENS` | 4096 | Max tokens per batch |
//! | `GG_CORE_SHUTDOWN_TIMEOUT` | 30 | Graceful shutdown timeout (secs) |
//! | `GG_CORE_SESSION_TIMEOUT` | 3600 | Session timeout (secs) |
//! | `GG_CORE_N_CTX` | 2048 | GGUF context window size |
//! | `GG_CORE_N_THREADS` | 0 | Inference threads (0 = auto) |
//! | `GG_CORE_IPC_FRAME_LIMIT` | 16777216 | Max IPC frame size (bytes) |
//! | `GG_CORE_MAX_CONNECTIONS` | 64 | Max concurrent IPC connections |
//! | `GG_CORE_STREAM_STALL_MS` | 5000 | Longest wait for a non-reading stream client (min 100) |

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
    /// Explicit per-request ceiling; `None` = bounded only by the total.
    pub resource_per_call_ceiling: Option<usize>,
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
    pub stream_stall_ms: u64,
}

/// GGUF-specific configuration loaded from env.
#[derive(Debug, Clone)]
pub struct GgufEnvConfig {
    pub n_ctx: u32,
    pub n_threads: u32,
}

impl Default for GgufEnvConfig {
    fn default() -> Self {
        Self {
            n_ctx: 2048,
            n_threads: 0,
        }
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
    /// Explicit `GG_CORE_MAX_MEMORY_PER_CALL` (daemon per-request ceiling).
    pub per_call_ceiling: Option<usize>,
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
    RequestQueueConfig {
        max_pending,
        max_context_tokens,
    }
}

/// The operator per-request ceiling: `Some` only when
/// `GG_CORE_MAX_MEMORY_PER_CALL` is explicitly set (same parse and 1 MiB
/// floor as the fallback reservation). Unset: no separate ceiling.
pub fn per_call_ceiling_from_env() -> Option<usize> {
    let value = std::env::var("GG_CORE_MAX_MEMORY_PER_CALL").ok()?;
    Some(value.parse::<usize>().ok()?.max(1024 * 1024))
}

/// Load resource limits configuration from environment.
fn load_resource_limits() -> ResourceLimitsConfig {
    let per_call = parse_usize("GG_CORE_MAX_MEMORY_PER_CALL", 1024 * 1024 * 1024);
    let total = parse_usize(
        "GG_CORE_MAX_TOTAL_MEMORY",
        crate::memory::host::default_total_memory(),
    );
    let concurrent = parse_usize("GG_CORE_MAX_CONCURRENT", 2);
    let per_call = per_call.max(1024 * 1024); // floor: 1MB
    let total = total.max(per_call); // total >= per_call
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
    BatchConfig {
        max_batch_size,
        max_total_tokens,
    }
}

/// Load GGUF engine configuration from environment.
pub(crate) fn load_gguf_config() -> GgufEnvConfig {
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
    let stall = parse_u64("GG_CORE_STREAM_STALL_MS", 5_000).max(100);
    let stream_stall = Duration::from_millis(stall);
    IpcServerConfig {
        max_frame_size,
        stream_stall,
    }
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
        per_call_ceiling: per_call_ceiling_from_env(),
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
            resource_per_call_ceiling: self.per_call_ceiling,
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
            stream_stall_ms: self.ipc_server.stream_stall.as_millis() as u64,
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
