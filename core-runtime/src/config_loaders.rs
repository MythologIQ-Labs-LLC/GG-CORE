//! Per-subsystem strict loaders for `try_load` (#194), split from `config.rs`
//! to keep it within the Section 4 size limit. Behaviour is unchanged.

use std::time::Duration;

use super::{resource, ConfigError};
use crate::ipc::{ConnectionConfig, IpcServerConfig};
use crate::scheduler::{BatchConfig, RequestQueueConfig};

/// Load queue configuration from environment.
pub(super) fn load_queue_config() -> Result<RequestQueueConfig, ConfigError> {
    Ok(RequestQueueConfig {
        max_pending: resource::from_env(
            "GG_CORE_MAX_QUEUE_DEPTH",
            256usize,
            1,
            usize::MAX,
            "decimal integer >= 1",
        )?,
        max_context_tokens: resource::from_env(
            "GG_CORE_MAX_CONTEXT_TOKENS",
            4096usize,
            1,
            usize::MAX,
            "decimal integer >= 1",
        )?,
    })
}

/// Load batch configuration from environment.
pub(super) fn load_batch_config() -> Result<BatchConfig, ConfigError> {
    Ok(BatchConfig {
        max_batch_size: resource::from_env(
            "GG_CORE_BATCH_MAX_REQUESTS",
            8usize,
            1,
            usize::MAX,
            "decimal integer >= 1",
        )?,
        max_total_tokens: resource::from_env(
            "GG_CORE_BATCH_MAX_TOKENS",
            4096usize,
            1,
            usize::MAX,
            "decimal integer >= 1",
        )?,
    })
}

/// Load IPC server configuration from environment.
pub(super) fn load_ipc_server_config() -> Result<IpcServerConfig, ConfigError> {
    const DEFAULT_FRAME: usize = 16 * 1024 * 1024;
    let max_frame_size = resource::from_env(
        "GG_CORE_IPC_FRAME_LIMIT",
        DEFAULT_FRAME,
        4096usize,
        u32::MAX as usize,
        "decimal integer in 4096..=4294967295",
    )?;
    let stall = resource::from_env(
        "GG_CORE_STREAM_STALL_MS",
        5_000u64,
        100,
        u64::MAX,
        "decimal u64 >= 100",
    )?;
    Ok(IpcServerConfig {
        max_frame_size,
        stream_stall: Duration::from_millis(stall),
    })
}

/// Load connection pool configuration from environment.
pub(super) fn load_connection_config() -> Result<ConnectionConfig, ConfigError> {
    Ok(ConnectionConfig {
        max_connections: resource::from_env(
            "GG_CORE_MAX_CONNECTIONS",
            64usize,
            1,
            usize::MAX,
            "decimal integer >= 1",
        )?,
    })
}
