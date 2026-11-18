//! External Rust consumer audit timestamp compatibility (#112/#213).
//! The public Rust timestamp type changes pre-v1, but historic RFC3339 UTC
//! wire/log semantics must survive the chrono -> time migration.

use gg_core::security::audit::{AuditConfig, AuditEvent, AuditLogger};
use gg_core::security::{AuditCategory, AuditSeverity};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

fn event() -> AuditEvent {
    AuditEvent::builder()
        .severity(AuditSeverity::Info)
        .category(AuditCategory::System)
        .event_type("compatibility")
        .message("Legacy UTC audit timestamp")
        .source("external_consumer_test")
        .build()
        .expect("valid audit event")
}

#[test]
fn historic_rfc3339_offsets_normalize_to_utc_without_time_shift() {
    let mut legacy = serde_json::to_value(event()).expect("serialize");
    legacy["timestamp"] = json!("2026-11-17T07:34:56.123456-05:00");
    let loaded: AuditEvent = serde_json::from_value(legacy).expect("historic offset");
    let expected =
        OffsetDateTime::parse("2026-11-17T12:34:56.123456Z", &Rfc3339).expect("expected timestamp");
    assert_eq!(loaded.timestamp, expected);
    assert_eq!(loaded.timestamp.offset(), UtcOffset::UTC);
    assert_eq!(
        serde_json::to_value(&loaded).unwrap()["timestamp"],
        json!("2026-11-17T12:34:56.123456Z")
    );
    assert!(loaded.to_log_string().contains("2026-11-17 12:34:56 UTC"));
}

#[test]
fn directly_assigned_non_utc_timestamp_cannot_mislabel_log_or_wire() {
    let mut value = event();
    value.timestamp = OffsetDateTime::parse("2026-11-17T07:34:56-05:00", &Rfc3339).unwrap();
    let emitted: Value = serde_json::to_value(&value).unwrap();
    assert_eq!(emitted["timestamp"], json!("2026-11-17T12:34:56Z"));
    assert!(value.to_log_string().contains("2026-11-17 12:34:56 UTC"));
}

#[tokio::test]
async fn exported_legacy_offset_and_new_utc_events_query_same_instant() {
    let logger = AuditLogger::new(AuditConfig {
        log_to_stdout: false,
        ..AuditConfig::default()
    });
    let mut value = event();
    value.timestamp = OffsetDateTime::parse("2026-11-17T07:34:56-05:00", &Rfc3339).unwrap();
    logger.log(value).await;
    let begin = OffsetDateTime::parse("2026-11-17T12:34:56Z", &Rfc3339).unwrap();
    assert_eq!(logger.get_events_by_time(begin, begin).await.len(), 1);
    let json = logger.export_json().await.expect("export");
    assert!(json.contains("2026-11-17T12:34:56Z"));
    assert!(!json.contains("07:34:56-05:00"));
}

#[test]
fn malformed_historical_timestamp_remains_a_refusal() {
    let mut legacy = serde_json::to_value(event()).unwrap();
    legacy["timestamp"] = json!("not a timestamp");
    assert!(serde_json::from_value::<AuditEvent>(legacy).is_err());
}

#[test]
fn external_rust_type_is_explicitly_offset_date_time_not_legacy_chrono() {
    let mut external = event();
    let timestamp: OffsetDateTime = external.timestamp;
    external.timestamp = timestamp.to_offset(UtcOffset::UTC);
    assert_eq!(external.timestamp.offset(), UtcOffset::UTC);
}

#[test]
fn historical_chrono_fraction_widths_are_stable_for_external_consumers() {
    // These string forms are part of persisted audit JSON compatibility.
    for historical in [
        "2026-11-17T12:34:56Z",
        "2026-11-17T12:34:56.120Z",
        "2026-11-17T12:34:56.000100Z",
        "2026-11-17T12:34:56.123456700Z",
        "1970-01-01T00:00:00.000000001Z",
    ] {
        let mut payload = serde_json::to_value(event()).unwrap();
        payload["timestamp"] = json!(historical);
        let restored: AuditEvent = serde_json::from_value(payload).expect("historical audit JSON");
        let emitted = serde_json::to_value(restored).unwrap();
        assert_eq!(
            emitted["timestamp"],
            json!(historical),
            "wire drift for {historical}"
        );
    }
}

#[test]
fn utc_date_overflow_is_an_external_deserialization_error_not_a_panic() {
    let mut payload = serde_json::to_value(event()).unwrap();
    payload["timestamp"] = json!("9999-12-31T23:30:00-01:00");
    assert!(serde_json::from_value::<AuditEvent>(payload).is_err());
}
