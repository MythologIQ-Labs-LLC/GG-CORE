//! Runtime configuration loading from environment variables.
//!
//! All configuration values are loaded from `GG_CORE_*` environment variables
//! with existing defaults when absent. Explicit memory and resource limits
//! are strict (#182, #194): invalid values fail `try_load`, never silently
//! substitute a default or clamp operator intent.
//!
//! # Environment Variables
//!
//! | Variable | Default | Description |
//! |---|---|---|
//! | `GG_CORE_MAX_CONTEXT` | 4096 | Max context length (tokens) |
//! | `GG_CORE_MAX_QUEUE_DEPTH` | 256 | Max pending requests |
//! | `GG_CORE_MAX_CONTEXT_TOKENS` | 4096 | Max context tokens per request |
//! | `GG_CORE_MAX_MEMORY_PER_CALL` | unset | Optional per-request working-memory ceiling (bytes, >= 1 MiB, <= total). Unset: a request reserves its model's metadata-derived need, bounded only by the total; models without an estimate reserve min(1 GiB, total) (or this value when set) |
//! | `GG_CORE_MAX_TOTAL_MEMORY` | max(2 GiB, 50% RAM) | Exact ceiling for resident weights + working memory (bytes, >= 1 MiB); also the Windows Job Object cap (unless `GG_CORE_SANDBOX=off`). Never raised |
//! | `GG_CORE_MAX_CONCURRENT` | 2 | Max concurrent requests = scheduler worker loops |
//! | `GG_CORE_BATCH_MAX_REQUESTS` | 8 | Max requests per batch |
//! | `GG_CORE_BATCH_MAX_TOKENS` | 4096 | Max tokens per batch |
//! | `GG_CORE_SHUTDOWN_TIMEOUT` | 30 | Graceful shutdown timeout (secs) |
//! | `GG_CORE_SESSION_TIMEOUT` | 3600 | Session timeout (secs) |
//! | `GG_CORE_N_CTX` | 2048 | GGUF context window size |
//! | `GG_CORE_N_THREADS` | 0 | Threads per GGUF context; 0 = auto: daemon `max(1, min(logical, 16) / GG_CORE_MAX_CONCURRENT)`, embedded `min(logical, 16)` |
//! | `GG_CORE_IPC_FRAME_LIMIT` | 16777216 | Max IPC frame size (bytes) |
//! | `GG_CORE_MAX_CONNECTIONS` | 64 | Max concurrent IPC connections |
//! | `GG_CORE_STREAM_STALL_MS` | 5000 | Longest wait for a non-reading stream client (min 100) |

use std::path::PathBuf;
use std::time::Duration;

use crate::ipc::{ConnectionConfig, IpcServerConfig};
use crate::memory::ResourceLimitsConfig;
use crate::scheduler::{BatchConfig, RequestQueueConfig};

#[path = "config_memory.rs"]
pub mod memory;
#[path = "config_resource.rs"]
pub mod resource;
pub use memory::{ConfigError, MemoryConfigError, MemoryLimits};
pub use resource::ResourceConfigError;

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

/// Load queue configuration from environment.
fn load_queue_config() -> Result<RequestQueueConfig, ConfigError> {
    Ok(RequestQueueConfig {
        max_pending: resource::from_env("GG_CORE_MAX_QUEUE_DEPTH", 256usize, 1,
                                        usize::MAX, "decimal integer >= 1")?,
        max_context_tokens: resource::from_env("GG_CORE_MAX_CONTEXT_TOKENS", 4096usize,
                                               1, usize::MAX, "decimal integer >= 1")?,
    })
}

/// Deprecated: the explicit `GG_CORE_MAX_MEMORY_PER_CALL`, `None` when unset.
///
/// # Panics
/// When the variable is set but invalid (it used to read as "no ceiling").
#[deprecated(note = "use `config::try_load()?.per_call_ceiling`")]
pub fn per_call_ceiling_from_env() -> Option<usize> {
    memory::per_call_ceiling_strict()
}

/// Load batch configuration from environment.
fn load_batch_config() -> Result<BatchConfig, ConfigError> {
    Ok(BatchConfig {
        max_batch_size: resource::from_env("GG_CORE_BATCH_MAX_REQUESTS", 8usize, 1,
                                            usize::MAX, "decimal integer >= 1")?,
        max_total_tokens: resource::from_env("GG_CORE_BATCH_MAX_TOKENS", 4096usize, 1,
                                             usize::MAX, "decimal integer >= 1")?,
    })
}

/// Fallible GGUF settings, also used by the daemon's common resolver.
fn try_load_gguf_config() -> Result<GgufEnvConfig, ConfigError> {
    Ok(GgufEnvConfig {
        n_ctx: resource::from_env("GG_CORE_N_CTX", 2048u32, 128, u32::MAX,
                                  "decimal u32 >= 128")?,
        n_threads: resource::from_env("GG_CORE_N_THREADS", 0u32, 0, u32::MAX,
                                      "decimal u32 (0 = auto)")?,
    })
}

/// Compatibility helper: keep the existing signature but never silently
/// fall back when an explicit GGUF value is invalid.
pub(crate) fn load_gguf_config() -> GgufEnvConfig {
    try_load_gguf_config().unwrap_or_else(|e| panic!("invalid GGUF configuration: {e}"))
}

/// Load IPC server configuration from environment.
fn load_ipc_server_config() -> Result<IpcServerConfig, ConfigError> {
    const DEFAULT_FRAME: usize = 16 * 1024 * 1024;
    let max_frame_size = resource::from_env("GG_CORE_IPC_FRAME_LIMIT", DEFAULT_FRAME,
                                            4096usize, usize::MAX, "decimal integer >= 4096")?;
    let stall = resource::from_env("GG_CORE_STREAM_STALL_MS", 5_000u64, 100,
                                   u64::MAX, "decimal u64 >= 100")?;
    Ok(IpcServerConfig {
        max_frame_size,
        stream_stall: Duration::from_millis(stall),
    })
}

/// Load connection pool configuration from environment.
fn load_connection_config() -> Result<ConnectionConfig, ConfigError> {
    Ok(ConnectionConfig {
        max_connections: resource::from_env("GG_CORE_MAX_CONNECTIONS", 64usize, 1,
                                            usize::MAX, "decimal integer >= 1")?,
    })
}

/// Load all configuration from environment variables.
///
/// # Panics
/// When a memory or resource setting is invalid (see [`try_load`]):
/// this signature cannot report the error, and substituting a different
/// limit is exactly what #182 forbids. New callers should use [`try_load`].
pub fn load() -> EnvConfig {
    try_load().unwrap_or_else(|e| panic!("invalid GG-CORE configuration: {e}"))
}

/// Load all configuration from environment variables.
///
/// Absent values use documented defaults; invalid explicit values return
/// [`ConfigError`] before runtime initialization and IPC bind.
pub fn try_load() -> Result<EnvConfig, ConfigError> {
    let mem = memory::from_env()?;
    let concurrent = resource::from_env("GG_CORE_MAX_CONCURRENT", 2usize, 1,
                                        usize::MAX, "decimal integer >= 1")?;
    let auth_token = std::env::var("CORE_AUTH_TOKEN").unwrap_or_default();
    let max_context_length = resource::from_env("GG_CORE_MAX_CONTEXT", 4096usize,
                                                1, 1_000_000, "decimal integer in 1..=1000000")?;
    let shutdown_secs = resource::from_env("GG_CORE_SHUTDOWN_TIMEOUT", 30u64,
                                           1, u64::MAX, "decimal u64 >= 1")?;
    let session_secs = resource::from_env("GG_CORE_SESSION_TIMEOUT", 3600u64,
                                          1, u64::MAX, "decimal u64 >= 1")?;

    Ok(EnvConfig {
        base_path: PathBuf::from("."),
        auth_token,
        max_context_length,
        request_queue: load_queue_config()?,
        resource_limits: mem.to_resource_limits(concurrent),
        per_call_ceiling: mem.per_call_ceiling,
        batch: load_batch_config()?,
        shutdown_timeout: Duration::from_secs(shutdown_secs),
        session_timeout: Duration::from_secs(session_secs),
        gguf: try_load_gguf_config()?,
        ipc_server: load_ipc_server_config()?,
        connections: load_connection_config()?,
    })
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
