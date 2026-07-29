//! Unix sandbox using cgroups v2 and seccomp-bpf.
//!
//! Enforces memory and CPU limits via Linux cgroups.
//! Enforces syscall restrictions via seccomp-bpf.
//! Note: Requires root or cgroup delegation for full functionality.
//!
//! # Security Warning
//!
//! This implementation provides actual cgroups v2 enforcement when possible.
//! If cgroups cannot be applied, the sandbox returns an error rather than
//! silently succeeding (security-in-depth principle).
//!
//! # Seccomp-bpf
//!
//! When enabled, seccomp-bpf restricts the syscalls available to the process
//! to a minimal whitelist required for inference operations. This provides
//! defense-in-depth against code execution vulnerabilities.
//!
//! The cgroup and seccomp machinery live in the `cgroup` / `seccomp` child
//! modules (B-16 Section 4 Razor split); this file holds the type and the
//! `Sandbox` trait impl.

use super::{Sandbox, SandboxConfig, SandboxResult, SandboxUsage};
use crate::telemetry::{log_security_event, SecurityEvent};
use std::fs;

/// Unix sandbox implementation using cgroups v2.
pub struct UnixSandbox {
    config: SandboxConfig,
    active: bool,
    cgroup_path: Option<String>,
}

impl UnixSandbox {
    /// Create a new Unix sandbox with the given configuration.
    pub fn new(config: SandboxConfig) -> Self {
        Self {
            config,
            active: false,
            cgroup_path: None,
        }
    }
}

impl Sandbox for UnixSandbox {
    fn apply(&self) -> SandboxResult {
        if !self.config.enabled {
            return SandboxResult {
                success: true,
                error: Some("sandbox disabled by config".into()),
            };
        }

        // SECURITY: Apply cgroup limits first
        let cgroup_result = self.apply_cgroup_limits();

        // SECURITY: Apply seccomp filter for syscall restriction
        let seccomp_result = self.apply_seccomp_filter();

        match (&cgroup_result, &seccomp_result) {
            (Ok(cgroup_path), Ok(())) => {
                let max_memory_mb = self.config.max_memory_bytes / 1024 / 1024;
                let max_cpu_ms = self.config.max_cpu_time_ms;
                log_security_event(
                    SecurityEvent::SandboxViolation,
                    "Unix sandbox applied successfully (cgroups + seccomp)",
                    &[
                        ("max_memory_mb", &format!("{}", max_memory_mb)),
                        ("max_cpu_ms", &format!("{}", max_cpu_ms)),
                        ("cgroup_path", cgroup_path),
                        ("seccomp", "enabled"),
                    ],
                );
                SandboxResult {
                    success: true,
                    error: None,
                }
            }
            (Err(e), _) | (_, Err(e)) => {
                // SECURITY: Return error instead of silently succeeding
                log_security_event(
                    SecurityEvent::SandboxViolation,
                    "Failed to apply Unix sandbox",
                    &[("error", e)],
                );
                SandboxResult {
                    success: false,
                    error: Some(format!(
                        "Sandbox enforcement failed: {}. \
                         Either run with appropriate privileges (root/cgroup delegation) \
                         or disable sandbox explicitly if not required.",
                        e
                    )),
                }
            }
        }
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn get_usage(&self) -> Option<SandboxUsage> {
        if !self.active {
            return None;
        }

        // Read from cgroup files if available
        if let Some(ref cgroup_path) = self.cgroup_path {
            let memory_path = format!("{}/memory.current", cgroup_path);
            let cpu_path = format!("{}/cpu.stat", cgroup_path);

            let memory_bytes = fs::read_to_string(&memory_path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);

            // Parse cpu.stat for usage_usec
            let cpu_time_ms = fs::read_to_string(&cpu_path)
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("usage_usec"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(|us| us / 1000) // Convert to ms
                })
                .unwrap_or(0);

            return Some(SandboxUsage {
                memory_bytes,
                cpu_time_ms,
            });
        }

        Some(SandboxUsage::default())
    }
}

#[path = "unix_cgroup.rs"]
mod cgroup;
#[path = "unix_seccomp.rs"]
mod seccomp;

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;
