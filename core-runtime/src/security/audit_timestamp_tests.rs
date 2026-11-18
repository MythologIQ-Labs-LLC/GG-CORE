//! Audit timestamp compatibility with the former chrono wire (#213).

use super::super::audit::*;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

fn event_at(timestamp: &str) -> AuditEvent {
    let mut event = AuditEvent::builder()
        .severity(AuditSeverity::Info)
        .category(AuditCategory::System)
        .event_type("timestamp_contract")
        .message("Check UTC audit timestamp wire")
        .source("test")
        .build()
        .unwrap();
    event.timestamp = OffsetDateTime::parse(timestamp, &Rfc3339).expect("valid RFC 3339");
    event
}

fn wire(event: &AuditEvent) -> serde_json::Value {
    serde_json::to_value(event).unwrap()["timestamp"].clone()
}

fn reparse(json_timestamp: &str) -> Result<AuditEvent, serde_json::Error> {
    let mut value = serde_json::to_value(event_at("2026-01-01T00:00:00Z")).unwrap();
    value["timestamp"] = serde_json::Value::String(json_timestamp.into());
    serde_json::from_value(value)
}

#[test]
fn test_audit_timestamp_rfc3339_json_roundtrip() {
    let event = event_at("2026-11-17T12:34:56Z");
    assert_eq!(wire(&event), "2026-11-17T12:34:56Z");
    let restored: AuditEvent =
        serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap();
    assert_eq!(restored.timestamp, event.timestamp);
    assert!(restored.to_log_string().contains("2026-11-17 12:34:56 UTC"));
}

#[test]
fn historical_chrono_wire_roundtrips_byte_for_byte() {
    // Every fraction shape chrono's AutoSi serializer emitted.
    for text in [
        "2026-11-17T12:34:56Z",
        "2026-11-17T12:34:56.120Z",
        "2026-11-17T12:34:56.000100Z",
        "2026-11-17T12:34:56.123456700Z",
        "1970-01-01T00:00:00.000000001Z",
    ] {
        let event = reparse(text).expect("historical audit JSON must parse");
        assert_eq!(wire(&event), text, "wire drift for {text}");
    }
}

#[test]
fn nanosecond_precision_is_preserved() {
    let event = reparse("2026-11-17T12:34:56.987654321Z").unwrap();
    assert_eq!(
        event.timestamp.unix_timestamp_nanos(),
        1_794_918_896_987_654_321
    );
}

#[test]
fn offset_input_is_normalized_to_utc_like_chrono() {
    let event = reparse("2026-11-17T14:34:56.5+02:00").unwrap();
    assert_eq!(event.timestamp.offset(), time::UtcOffset::UTC);
    assert_eq!(wire(&event), "2026-11-17T12:34:56.500Z");
    assert!(event.to_log_string().contains("2026-11-17 12:34:56 UTC"));
}

#[test]
fn caller_supplied_offset_is_logged_and_serialized_in_utc() {
    let event = event_at("2026-11-17T07:34:56-05:00");
    assert_eq!(wire(&event), "2026-11-17T12:34:56Z");
    assert!(event.to_log_string().contains("2026-11-17 12:34:56 UTC"));
}

#[test]
fn non_rfc3339_and_unrepresentable_timestamps_are_rejected() {
    // RFC 3339 section 5.6 permits a space separator; chrono accepted it too.
    let spaced = reparse("2026-11-17 12:34:56Z").expect("space separator is RFC 3339");
    assert_eq!(wire(&spaced), "2026-11-17T12:34:56Z");
    for text in ["2026-11-17T12:34:56", "2026-11-17", "not a time", ""] {
        assert!(reparse(text).is_err(), "accepted {text:?}");
    }
    // Converting to UTC would leave the four-digit-year range: error, no panic.
    assert!(reparse("9999-12-31T23:30:00-01:00").is_err());
}

#[tokio::test]
async fn test_audit_timestamp_range_queries_compare_instants() {
    let logger = AuditLogger::new(AuditConfig {
        log_to_stdout: false,
        ..Default::default()
    });
    logger.log(event_at("2026-11-17T12:34:56Z")).await;
    let at = |t: &str| OffsetDateTime::parse(t, &Rfc3339).unwrap();
    let in_range = logger
        .get_events_by_time(at("2026-11-17T12:30:00Z"), at("2026-11-17T12:40:00Z"))
        .await;
    assert_eq!(in_range.len(), 1);
    // Same window expressed in another offset selects the same instant.
    let offset_window = logger
        .get_events_by_time(
            at("2026-11-17T14:30:00+02:00"),
            at("2026-11-17T14:40:00+02:00"),
        )
        .await;
    assert_eq!(offset_window.len(), 1);
    let late = logger
        .get_events_by_time(at("2026-11-17T12:40:00Z"), at("2026-11-17T13:00:00Z"))
        .await;
    assert!(late.is_empty());
}
