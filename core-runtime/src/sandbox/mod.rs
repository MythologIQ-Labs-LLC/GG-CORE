//! Process sandbox for CORE Runtime.
//!
//! Platform-specific process isolation to enforce resource limits and security.

#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;

#[cfg(windows)]
pub use windows::WindowsSandbox;
#[cfg(unix)]
pub use unix::UnixSandbox;

/// Configuration for process sandboxing.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory in bytes (enforced by OS).
    pub max_memory_bytes: usize,
    /// Maximum CPU time in milliseconds.
    pub max_cpu_time_ms: u64,
    /// Whether to enable the sandbox (false = dry run).
    pub enabled: bool,
    /// Whether GPU device access is permitted (adds ioctl to seccomp whitelist).
    pub gpu_enabled: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 2 * 1024 * 1024 * 1024, // 2GB
            max_cpu_time_ms: 30_000,                   // 30s
            enabled: true,
            gpu_enabled: false,
        }
    }
}

/// Sandbox result with optional error details.
#[derive(Debug)]
pub struct SandboxResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Platform-agnostic sandbox trait.
pub trait Sandbox: Send + Sync {
    /// Apply sandbox restrictions to the current process.
    fn apply(&self) -> SandboxResult;

    /// Check if sandbox is currently active.
    fn is_active(&self) -> bool;

    /// Get current resource usage if available.
    fn get_usage(&self) -> Option<SandboxUsage>;
}

/// Resource usage reported by sandbox.
#[derive(Debug, Clone, Default)]
pub struct SandboxUsage {
    pub memory_bytes: usize,
    pub cpu_time_ms: u64,
}

/// Create the appropriate sandbox for the current platform.
pub fn create_sandbox(config: SandboxConfig) -> Box<dyn Sandbox> {
    #[cfg(windows)]
    {
        Box::new(WindowsSandbox::new(config))
    }
    #[cfg(unix)]
    {
        Box::new(UnixSandbox::new(config))
    }
    #[cfg(not(any(windows, unix)))]
    {
        Box::new(NoopSandbox::new(config))
    }
}

/// No-op sandbox for unsupported platforms.
#[cfg(not(any(windows, unix)))]
pub struct NoopSandbox {
    config: SandboxConfig,
}

#[cfg(not(any(windows, unix)))]
impl NoopSandbox {
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }
}

#[cfg(not(any(windows, unix)))]
impl Sandbox for NoopSandbox {
    fn apply(&self) -> SandboxResult {
        SandboxResult {
            success: true,
            error: Some("sandbox not supported on this platform".into()),
        }
    }

    fn is_active(&self) -> bool {
        false
    }

    fn get_usage(&self) -> Option<SandboxUsage> {
        None
    }
}
