//! Daemon sandbox activation (closes SECURITY.md Known Gap 1).
//!
//! `gg-core-cli serve` applies the process sandbox after model preload and
//! before the IPC server starts (model preload mmaps files; the listener's
//! bind/listen/accept4 are on the seccomp allowlist, so server start works
//! under the filter). Controlled by `GG_CORE_SANDBOX`:
//!
//! - `seccomp` — syscall allowlist only (no privileges needed). Default on
//!   Linux x86_64.
//! - `enforce` — seccomp + cgroup v2 memory/CPU limits (needs root or cgroup
//!   delegation). On Windows this is Job Objects. Default on Windows.
//! - `off` — no sandbox (logged loudly). Default on platforms without an
//!   implementation (macOS, non-x86_64 Linux).
//!
//! A requested sandbox that cannot be applied ABORTS startup (fail-loud);
//! the error names `GG_CORE_SANDBOX=off` as the explicit opt-out.

use gg_core::sandbox::{create_sandbox, SandboxConfig};
use gg_core::RuntimeConfig;

pub const SANDBOX_ENV: &str = "GG_CORE_SANDBOX";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    Enforce,
    Seccomp,
    Off,
}

fn platform_default() -> SandboxMode {
    if cfg!(windows) {
        SandboxMode::Enforce // Job Objects need no special privileges
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        SandboxMode::Seccomp
    } else {
        SandboxMode::Off
    }
}

/// Parse `GG_CORE_SANDBOX` (unset → platform default; unknown value → Err).
pub fn sandbox_mode_from_env() -> Result<SandboxMode, String> {
    match std::env::var(SANDBOX_ENV) {
        Err(_) => Ok(platform_default()),
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "enforce" => Ok(SandboxMode::Enforce),
            "seccomp" => Ok(SandboxMode::Seccomp),
            "off" => Ok(SandboxMode::Off),
            other => Err(format!(
                "invalid {SANDBOX_ENV} value '{other}' (expected enforce|seccomp|off)"
            )),
        },
    }
}

/// Sandbox configuration for the long-lived serving daemon.
///
/// `max_cpu_time_ms` is deliberately 0: the library default (30 s) is a
/// per-job cumulative CPU budget meant for short-lived embedded workloads.
/// Windows enforces it with `JOB_OBJECT_LIMIT_JOB_TIME`, which terminates
/// the whole job once exhausted — a serving daemon would be killed a few
/// seconds into real inference. Memory is the daemon's resource gate.
pub fn daemon_sandbox_config(mode: SandboxMode, runtime_config: &RuntimeConfig) -> SandboxConfig {
    SandboxConfig {
        max_memory_bytes: runtime_config.resource_limits.max_total_memory,
        max_cpu_time_ms: 0,
        cgroup_enabled: mode == SandboxMode::Enforce,
        gpu_enabled: false,
        ..Default::default()
    }
}

/// Apply the daemon sandbox per mode. Returns a human-readable status line
/// on success; an `Err` must abort startup.
pub fn apply_daemon_sandbox(
    mode: SandboxMode,
    runtime_config: &RuntimeConfig,
) -> Result<String, String> {
    if mode == SandboxMode::Off {
        return Ok(format!(
            "Sandbox: OFF ({}persistent process isolation is NOT applied)",
            if platform_default() == SandboxMode::Off {
                "no implementation for this platform; "
            } else {
                ""
            }
        ));
    }

    let result = create_sandbox(daemon_sandbox_config(mode, runtime_config)).apply();
    if result.success {
        Ok(match mode {
            SandboxMode::Enforce => "Sandbox: ENFORCE (resource limits + syscall allowlist)",
            SandboxMode::Seccomp => "Sandbox: SECCOMP (syscall allowlist)",
            SandboxMode::Off => unreachable!(),
        }
        .to_string())
    } else {
        Err(format!(
            "sandbox activation failed: {}. Set {SANDBOX_ENV}=off to run without \
             process isolation (not recommended), or fix the underlying cause.",
            result.error.unwrap_or_else(|| "unknown error".into())
        ))
    }
}

#[cfg(test)]
#[path = "sandbox_boot_tests.rs"]
mod tests;
