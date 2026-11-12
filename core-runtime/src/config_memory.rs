// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Strict resolution of the memory limits (#182).
//!
//! `GG_CORE_MAX_TOTAL_MEMORY` and `GG_CORE_MAX_MEMORY_PER_CALL` are resource
//! authority. An explicitly set value is used exactly or refused: it is never
//! floored, defaulted, clamped or raised. An explicit total is the hard
//! ceiling (and the Windows Job Object cap); nothing on this path increases it.

use std::ffi::OsStr;
use std::fmt;

use crate::memory::ResourceLimitsConfig;

/// Variable naming the total memory ceiling (bytes).
pub const TOTAL_VAR: &str = "GG_CORE_MAX_TOTAL_MEMORY";
/// Variable naming the optional per-request ceiling (bytes).
pub const PER_CALL_VAR: &str = "GG_CORE_MAX_MEMORY_PER_CALL";
/// Smallest explicit value accepted for either variable (1 MiB).
pub const MIN_EXPLICIT_MEMORY: usize = 1 << 20;
/// Working-memory reservation for a request without an estimate (1 GiB),
/// lowered to the total when the total is smaller.
pub const DEFAULT_PER_CALL_FALLBACK: usize = 1 << 30;

/// The resolved memory limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLimits {
    /// Effective total: resident weights plus working memory.
    pub total: usize,
    /// Reservation for a request whose model has no working-memory estimate.
    pub per_call_fallback: usize,
    /// Explicit per-request ceiling; `None` = bounded only by the total.
    pub per_call_ceiling: Option<usize>,
}

/// An explicitly set memory variable that cannot be applied as written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemoryConfigError {
    /// Not a plain decimal byte count (empty, sign, space, suffix,
    /// non-UTF-8, or larger than the platform's `usize`).
    Invalid {
        var: &'static str,
        reason: &'static str,
    },
    /// Below [`MIN_EXPLICIT_MEMORY`]; `0` is refused here, never "unlimited".
    BelowMinimum {
        var: &'static str,
        value: usize,
        min: usize,
    },
    /// The per-request ceiling exceeds the total.
    PerCallExceedsTotal {
        per_call: usize,
        total: usize,
        total_explicit: bool,
    },
}

impl fmt::Display for MemoryConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { var, reason } => {
                write!(f, "{var} is invalid: {reason}; expected a decimal byte count")
            }
            Self::BelowMinimum { var, value, min } => {
                write!(f, "{var}={value} is below the minimum of {min} bytes")
            }
            Self::PerCallExceedsTotal {
                per_call,
                total,
                total_explicit: true,
            } => write!(
                f,
                "{PER_CALL_VAR}={per_call} exceeds {TOTAL_VAR}={total}; lower the per-call value or raise the total"
            ),
            Self::PerCallExceedsTotal { per_call, total, .. } => write!(
                f,
                "{PER_CALL_VAR}={per_call} exceeds the default total of {total} bytes; set {TOTAL_VAR} explicitly"
            ),
        }
    }
}

impl std::error::Error for MemoryConfigError {}

/// Parse one explicitly present value. Absent (`None`) stays `None`.
fn parse_explicit(
    var: &'static str,
    raw: Option<&OsStr>,
) -> Result<Option<usize>, MemoryConfigError> {
    let Some(raw) = raw else { return Ok(None) };
    let invalid = |reason| MemoryConfigError::Invalid { var, reason };
    let text = raw.to_str().ok_or(invalid("not valid UTF-8"))?;
    if text.is_empty() {
        return Err(invalid("empty value"));
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid("not a plain decimal number"));
    }
    let value = text
        .parse::<usize>()
        .map_err(|_| invalid("too large for this platform"))?;
    if value < MIN_EXPLICIT_MEMORY {
        return Err(MemoryConfigError::BelowMinimum {
            var,
            value,
            min: MIN_EXPLICIT_MEMORY,
        });
    }
    Ok(Some(value))
}

/// Resolve the limits from raw variable values. Pure: no environment access.
pub fn resolve(
    raw_total: Option<&OsStr>,
    raw_per_call: Option<&OsStr>,
    host_default: usize,
) -> Result<MemoryLimits, MemoryConfigError> {
    let explicit_total = parse_explicit(TOTAL_VAR, raw_total)?;
    let ceiling = parse_explicit(PER_CALL_VAR, raw_per_call)?;
    let total = explicit_total.unwrap_or(host_default);
    if let Some(per_call) = ceiling.filter(|&p| p > total) {
        return Err(MemoryConfigError::PerCallExceedsTotal {
            per_call,
            total,
            total_explicit: explicit_total.is_some(),
        });
    }
    Ok(MemoryLimits {
        total,
        per_call_fallback: ceiling.unwrap_or(DEFAULT_PER_CALL_FALLBACK.min(total)),
        per_call_ceiling: ceiling,
    })
}

/// Resolve the limits from the process environment.
pub fn from_env() -> Result<MemoryLimits, MemoryConfigError> {
    let total = std::env::var_os(TOTAL_VAR);
    let per_call = std::env::var_os(PER_CALL_VAR);
    resolve(
        total.as_deref(),
        per_call.as_deref(),
        crate::memory::host::default_total_memory(),
    )
}

impl MemoryLimits {
    /// The admission configuration these limits describe.
    pub fn to_resource_limits(&self, max_concurrent: usize) -> ResourceLimitsConfig {
        ResourceLimitsConfig {
            max_memory_per_call: self.per_call_fallback,
            max_total_memory: self.total,
            max_concurrent,
        }
    }
}

/// The explicit per-call ceiling alone (no comparison with the total).
///
/// # Panics
/// When `GG_CORE_MAX_MEMORY_PER_CALL` is set but invalid: an invalid limit
/// is never read as "no ceiling".
pub fn per_call_ceiling_strict() -> Option<usize> {
    let raw = std::env::var_os(PER_CALL_VAR);
    parse_explicit(PER_CALL_VAR, raw.as_deref()).unwrap_or_else(|e| panic!("{e}"))
}

/// A runtime configuration that cannot be applied without relaxing a limit.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// An invalid or contradictory memory limit.
    Memory(MemoryConfigError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Memory(e) => Some(e),
        }
    }
}

impl From<MemoryConfigError> for ConfigError {
    fn from(e: MemoryConfigError) -> Self {
        Self::Memory(e)
    }
}

#[cfg(test)]
#[path = "config_memory_tests.rs"]
mod tests;
