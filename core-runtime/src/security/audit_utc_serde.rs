//! Canonical UTC wire behavior for audit timestamps.
//! Previous chrono::DateTime<Utc> accepted RFC3339 offsets and normalized
//! them to UTC. Bare time::serde::rfc3339 preserves offsets and would make
//! human audit strings labeled UTC incorrect after a historical import.

use serde::{Deserializer, Serializer};
use time::{OffsetDateTime, UtcOffset};

pub(super) fn serialize<S>(value: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    time::serde::rfc3339::serialize(&value.to_offset(UtcOffset::UTC), serializer)
}

pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    time::serde::rfc3339::deserialize(deserializer)
        .map(|timestamp| timestamp.to_offset(UtcOffset::UTC))
}
