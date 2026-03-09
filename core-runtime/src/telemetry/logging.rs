//! Logging configuration and initialization for CORE Runtime.
//!
//! Supports JSON and pretty-printed formats with configurable output paths.

use std::path::PathBuf;
use thiserror::Error;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Log output format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// JSON structured logging (default for production).
    #[default]
    Json,
    /// Human-readable pretty printing (for development).
    Pretty,
}

/// Logging configuration.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Output format (JSON or Pretty).
    pub format: LogFormat,
    /// Log level filter (e.g., "info", "debug", "core_runtime=trace").
    pub level: String,
    /// Optional file path for log output. If None, logs to stderr.
    pub output_path: Option<PathBuf>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Json,
            level: "info".to_string(),
            output_path: None,
        }
    }
}

/// Errors that can occur during logging initialization.
#[derive(Debug, Error)]
pub enum LogError {
    #[error("Invalid log filter: {0}")]
    InvalidFilter(String),
    #[error("Failed to open log file: {0}")]
    FileOpen(String),
    #[error("Subscriber already initialized")]
    AlreadyInitialized,
}

/// Initialize the tracing subscriber with the given configuration.
///
/// This should be called once at application startup.
pub fn init_logging(config: &LogConfig) -> Result<(), LogError> {
    let filter =
        EnvFilter::try_new(&config.level).map_err(|e| LogError::InvalidFilter(e.to_string()))?;

    match config.format {
        LogFormat::Json => init_json_subscriber(filter, &config.output_path),
        LogFormat::Pretty => init_pretty_subscriber(filter),
    }
}

fn init_json_subscriber(filter: EnvFilter, path: &Option<PathBuf>) -> Result<(), LogError> {
    let registry = tracing_subscriber::registry().with(filter);

    if let Some(path) = path {
        let file = std::fs::File::create(path).map_err(|e| LogError::FileOpen(e.to_string()))?;
        registry
            .with(fmt::layer().json().with_writer(std::sync::Mutex::new(file)))
            .try_init()
            .map_err(|_| LogError::AlreadyInitialized)?;
    } else {
        registry
            .with(fmt::layer().json())
            .try_init()
            .map_err(|_| LogError::AlreadyInitialized)?;
    }

    Ok(())
}

fn init_pretty_subscriber(filter: EnvFilter) -> Result<(), LogError> {
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().pretty())
        .try_init()
        .map_err(|_| LogError::AlreadyInitialized)?;
    Ok(())
}
