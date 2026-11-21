//! S3 observation: emitted exactly when a stop is seen, with the last
//! decode of this request only (audit A1), and nothing sensitive.

use super::*;
use crate::telemetry::stop_trace::{channel, subscriber};

fn capture<T>(f: impl FnOnce() -> T) -> (T, Vec<serde_json::Value>) {
    let (writer, rx) = channel(256);
    let out = tracing::subscriber::with_default(subscriber(writer), f);
    let lines = rx
        .try_iter()
        .map(|l| serde_json::from_slice(&l).unwrap())
        .collect();
    (out, lines)
}

#[test]
fn no_event_and_no_reset_while_not_stopped() {
    record_for_test(1_500, 64);
    let never = || false;
    let (seen, lines) = capture(|| stopped(Some(&never), Phase::Prefill, 64, 100));
    assert!(!seen);
    assert!(lines.is_empty());
    assert_eq!(
        last_decode(),
        Some((1_500, 64)),
        "kept for the next checkpoint"
    );
    assert!(
        !stopped(None, Phase::Decode, 0, 0),
        "no stop check, no stop"
    );
}

#[test]
fn a_stop_reports_the_in_flight_decode_then_resets() {
    record_for_test(812_250, 128);
    let stop = || true;
    let (seen, lines) = capture(|| stopped(Some(&stop), Phase::Prefill, 256, 300));
    assert!(seen);
    assert_eq!(lines.len(), 1);
    let f = &lines[0]["fields"];
    assert_eq!(f["message"], "backend stop observed");
    assert_eq!(f["phase"], "prefill");
    assert_eq!(
        (f["done"].as_u64(), f["remaining"].as_u64()),
        (Some(256), Some(300))
    );
    assert_eq!(f["last_decode_tokens"], 128);
    assert!((f["last_decode_ms"].as_f64().unwrap() - 812.25).abs() < 1e-9);
    assert!(f["dropped"].is_u64());
    assert_eq!(last_decode(), None);
}

#[test]
fn request_entry_resets_even_without_a_stop_audit_a1() {
    // Previous request on this pooled thread decoded and finished normally.
    record_for_test(40_000, 512);
    let never = || false;
    assert!(!setup_stop(Some(&never)));
    assert_eq!(last_decode(), None, "reset on entry, stop or not");
    // Next checkpoint of the new request stops before its first decode.
    let stop = || true;
    let (_, lines) = capture(|| stopped(Some(&stop), Phase::Prefill, 0, 900));
    let f = &lines[0]["fields"];
    assert!(f["last_decode_tokens"].is_null(), "{f}");
    assert!(f["last_decode_ms"].is_null(), "{f}");
}

#[test]
fn a_setup_stop_is_reported_as_setup() {
    record_for_test(9, 1);
    let stop = || true;
    let (seen, lines) = capture(|| setup_stop(Some(&stop)));
    assert!(seen);
    let f = &lines[0]["fields"];
    assert_eq!(f["phase"], "setup");
    assert!(f["last_decode_ms"].is_null());
}

#[test]
fn cancelled_or_disconnected_stream_never_flushes_sanitizer_tail() {
    let stop = || true;
    let active = || false;
    assert!(!may_flush_stream(true, None), "closed receiver cannot flush");
    assert!(
        !may_flush_stream(true, Some(&active)),
        "stalled producer cannot flush even if cancellation was not set"
    );
    assert!(
        !may_flush_stream(false, Some(&stop)),
        "cancelled producer cannot flush withheld text"
    );
    assert!(may_flush_stream(false, Some(&active)));
    assert!(may_flush_stream(false, None));
}
