// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Strict parsing of non-memory resource settings (#194).
//!
//! Absent settings preserve existing defaults. Explicit settings must be
//! plain decimal integers in their documented ranges. Never silently
//! substitute a more permissive value for invalid operator input.

use std::ffi::OsStr;
use std::fmt;
use std::str::FromStr;

/// A present resource variable that cannot be used as written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResourceConfigError {
    /// The value was empty, not a plain decimal number, or not representable.
    Invalid {
        var: &'static str,
        expected: &'static str,
    },
    /// A decimal value was representable but outside the allowed range.
    OutOfRange {
        var: &'static str,
        expected: &'static str,
    },
}

impl fmt::Display for ResourceConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { var, expected } => {
                write!(f, "{var} is invalid; expected {expected}")
            }
            Self::OutOfRange { var, expected } => {
                write!(f, "{var} is outside its allowed range; expected {expected}")
            }
        }
    }
}

impl std::error::Error for ResourceConfigError {}

/// Pure parser: absent uses the old default; explicit values are exact or fail.
///
/// `T` is used only with unsigned integer types (`usize`, `u32`, `u64`).
/// Its bounds and expected range come from the caller's variable policy.
pub fn parse_decimal<T>(
    var: &'static str,
    raw: Option<&OsStr>,
    default: T,
    min: T,
    max: T,
    expected: &'static str,
) -> Result<T, ResourceConfigError>
where
    T: FromStr + PartialOrd + Copy,
{
    let Some(raw) = raw else {
        return Ok(default);
    };
    let invalid = || ResourceConfigError::Invalid { var, expected };
    let text = raw.to_str().ok_or_else(invalid)?;
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let value = text.parse::<T>().map_err(|_| invalid())?;
    if value < min || value > max {
        return Err(ResourceConfigError::OutOfRange { var, expected });
    }
    Ok(value)
}

/// Resolve a process variable through the same pure, typed parser.
pub fn from_env<T>(
    var: &'static str,
    default: T,
    min: T,
    max: T,
    expected: &'static str,
) -> Result<T, ResourceConfigError>
where
    T: FromStr + PartialOrd + Copy,
{
    let raw = std::env::var_os(var);
    parse_decimal(var, raw.as_deref(), default, min, max, expected)
}

#[cfg(test)]
#[path = "config_resource_tests.rs"]
mod tests;
