//! Cgroup v2 resource-limit enforcement for `UnixSandbox` (B-16 split from `unix.rs`).
//!
//! Applies memory and CPU limits via Linux cgroups v2. Relocated verbatim;
//! behavior is unchanged.

use super::UnixSandbox;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Cgroup base path for v2
const CGROUP_V2_BASE: &str = "/sys/fs/cgroup";

/// Sandbox cgroup name
const SANDBOX_CGROUP_NAME: &str = "gg-core-sandbox";

impl UnixSandbox {
    /// Check if cgroups v2 is available on this system
    pub(super) fn cgroups_v2_available() -> bool {
        Path::new(CGROUP_V2_BASE)
            .join("cgroup.controllers")
            .exists()
    }

    /// Create cgroup directory and apply limits
    pub(super) fn apply_cgroup_limits(&self) -> Result<String, String> {
        if !Self::cgroups_v2_available() {
            return Err("cgroups v2 not available on this system".to_string());
        }

        let cgroup_path = format!("{}/{}", CGROUP_V2_BASE, SANDBOX_CGROUP_NAME);

        // Create cgroup directory
        fs::create_dir_all(&cgroup_path)
            .map_err(|e| format!("Failed to create cgroup directory: {}", e))?;

        // Apply memory limit
        if self.config.max_memory_bytes > 0 {
            let memory_path = format!("{}/memory.max", cgroup_path);
            let mut file = OpenOptions::new()
                .write(true)
                .open(&memory_path)
                .map_err(|e| format!("Failed to open memory.max: {}", e))?;

            writeln!(file, "{}", self.config.max_memory_bytes)
                .map_err(|e| format!("Failed to write memory limit: {}", e))?;
        }

        // Apply CPU limit (in microseconds per second)
        if self.config.max_cpu_time_ms > 0 {
            // Convert ms to microseconds per second (quota/period)
            let quota_us = self.config.max_cpu_time_ms * 1000;
            let period_us = 1_000_000; // 1 second period

            let cpu_path = format!("{}/cpu.max", cgroup_path);
            let mut file = OpenOptions::new()
                .write(true)
                .open(&cpu_path)
                .map_err(|e| format!("Failed to open cpu.max: {}", e))?;

            writeln!(file, "{} {}", quota_us, period_us)
                .map_err(|e| format!("Failed to write CPU limit: {}", e))?;
        }

        // Add current process to cgroup
        let pid = std::process::id();
        let procs_path = format!("{}/cgroup.procs", cgroup_path);
        let mut file = OpenOptions::new()
            .write(true)
            .open(&procs_path)
            .map_err(|e| format!("Failed to open cgroup.procs: {}. Note: This may require root or cgroup delegation.", e))?;

        writeln!(file, "{}", pid).map_err(|e| format!("Failed to add process to cgroup: {}", e))?;

        Ok(cgroup_path)
    }
}
