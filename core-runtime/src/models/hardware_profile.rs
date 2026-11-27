//! Conservative, output-only local execution evidence for advisory selection (#50).
//!
//! The host can detect CPU architecture and available execution threads, but
//! Cargo features and installed GPU drivers do NOT prove a serving accelerator.
//! Unreserved memory is deliberately unknown until an owner of the real
//! admission/reservation accounting provides a trustworthy snapshot.
//!
//! This metadata is NOT an authority boundary: caller-manufactured profiles
//! never authorize model execution or bypass model, path or custody checks.

use serde::Serialize;

use super::backend_capabilities::ExecutionTarget;
use crate::memory::ResourceLimits;

pub const HARDWARE_PROFILE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetAvailability {
    Unknown,
    VerifiedAvailable,
    VerifiedUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HardwareProfile {
    pub schema_version: u16,
    pub cpu_architecture: String,
    /// Parallelism this process is allowed, not the installed socket count.
    pub available_cpu_threads: Option<usize>,
    pub cpu: TargetAvailability,
    pub cuda: TargetAvailability,
    pub metal: TargetAvailability,
    /// Shared admission-budget headroom. Not a physical free-RAM guarantee;
    /// must be rechecked by canonical memory reservation at execution.
    pub unreserved_ram_bytes: Option<u64>,
    /// Unreserved device headroom. None means no trusted measurement exists.
    pub unreserved_vram_bytes: Option<u64>,
}

impl HardwareProfile {
    /// Capture only facts available through standard-library local probes.
    ///
    /// GPU presence, accelerator memory and allocatable host memory remain
    /// unknown, even in CUDA/Metal-enabled builds.
    pub fn detect_local() -> Self {
        Self {
            schema_version: HARDWARE_PROFILE_SCHEMA_VERSION,
            cpu_architecture: std::env::consts::ARCH.to_string(),
            available_cpu_threads: std::thread::available_parallelism().ok().map(|n| n.get()),
            cpu: TargetAvailability::VerifiedAvailable,
            cuda: TargetAvailability::Unknown,
            metal: TargetAvailability::Unknown,
            unreserved_ram_bytes: None,
            unreserved_vram_bytes: None,
        }
    }

    /// Observe the current shared admission-budget headroom (#50).
    ///
    /// ResourceLimits accounts for both held model weights and in-flight
    /// working reservations using one atomic counter. This is NOT physical
    /// free RAM, a claim of loadability, or an execution reservation:
    /// concurrent admissions may consume the reported bytes immediately.
    /// Final model loading must acquire the same canonical reservation.
    ///
    /// With no configured resource limits, headroom stays unknown instead
    /// of treating an unlimited embedded budget as infinite physical RAM.
    pub fn detect_with_limits(limits: Option<&ResourceLimits>) -> Self {
        let mut profile = Self::detect_local();
        if let Some(limits) = limits {
            let unreserved = limits
                .max_total_memory()
                .saturating_sub(limits.current_memory());
            profile.unreserved_ram_bytes = u64::try_from(unreserved).ok();
        }
        profile
    }

    pub fn is_valid(&self) -> bool {
        self.schema_version == HARDWARE_PROFILE_SCHEMA_VERSION
            && !self.cpu_architecture.trim().is_empty()
            && self.available_cpu_threads != Some(0)
    }

    pub fn target_availability(&self, target: ExecutionTarget) -> TargetAvailability {
        match target {
            ExecutionTarget::Cpu => self.cpu,
            ExecutionTarget::Cuda => self.cuda,
            ExecutionTarget::Metal => self.metal,
        }
    }

    pub fn unreserved_bytes(&self, target: ExecutionTarget) -> Option<u64> {
        match target {
            ExecutionTarget::Cpu => self.unreserved_ram_bytes,
            ExecutionTarget::Cuda | ExecutionTarget::Metal => self.unreserved_vram_bytes,
        }
    }
}

#[cfg(test)]
#[path = "hardware_profile_limits_tests.rs"]
mod limits_tests;
