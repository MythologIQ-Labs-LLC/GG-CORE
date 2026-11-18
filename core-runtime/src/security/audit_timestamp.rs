//! Audit timestamp wire and log format (#213).
//!
//! Byte-compatible with the former `chrono::DateTime<Utc>` serde output:
//! RFC 3339 in UTC with a `Z` suffix, and fractional seconds written as
//! 0, 3, 6 or 9 digits (chrono `SecondsFormat::AutoSi`). An input with a
//! non-zero offset is converted to the same instant in UTC, as before.
//! Instants with no representable UTC date are rejected, never panicked on.

use serde::{de::Error as _, ser::Error as _, Deserialize, Deserializer, Serializer};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// The same instant in UTC, or `None` when that date is out of range.
fn to_utc(value: OffsetDateTime) -> Option<OffsetDateTime> {
    value.checked_to_offset(UtcOffset::UTC)
}

/// Fraction digits exactly as chrono's `SecondsFormat::AutoSi` wrote them.
fn auto_si_fraction(nano: u32) -> String {
    if nano == 0 {
        String::new()
    } else if nano.is_multiple_of(1_000_000) {
        format!(".{:03}", nano / 1_000_000)
    } else if nano.is_multiple_of(1_000) {
        format!(".{:06}", nano / 1_000)
    } else {
        format!(".{nano:09}")
    }
}

/// RFC 3339 UTC text, or `None` outside the four-digit-year range.
pub(super) fn format_utc_rfc3339(value: OffsetDateTime) -> Option<String> {
    let utc = to_utc(value)?;
    let whole = utc.replace_nanosecond(0).ok()?.format(&Rfc3339).ok()?;
    let stem = whole.strip_suffix('Z')?;
    Some(format!("{stem}{}Z", auto_si_fraction(utc.nanosecond())))
}

/// Human audit-log form, always rendered in UTC.
pub(super) fn format_utc_human(value: OffsetDateTime) -> String {
    let human = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    to_utc(value)
        .and_then(|utc| utc.format(&human).ok())
        .unwrap_or_else(|| "invalid timestamp".to_string())
}

pub(super) fn serialize<S: Serializer>(value: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    let text = format_utc_rfc3339(*value)
        .ok_or_else(|| S::Error::custom("audit timestamp is outside the RFC 3339 range"))?;
    s.serialize_str(&text)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<OffsetDateTime, D::Error> {
    let text = String::deserialize(d)?;
    let parsed = OffsetDateTime::parse(&text, &Rfc3339).map_err(D::Error::custom)?;
    to_utc(parsed).ok_or_else(|| D::Error::custom("audit timestamp has no UTC representation"))
}

#[cfg(test)]
#[path = "audit_timestamp_tests.rs"]
mod tests;
