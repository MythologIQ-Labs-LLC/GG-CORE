//! Streaming S4 is decided by the control (a stopped stream can return
//! `Ok(())`), with `stream = true`; an unstopped stream emits nothing.

use super::*;
use crate::telemetry::stop_trace::test_capture;

fn control(queue_id: u64, cancelled: bool) -> StreamControl {
    let cancel = tokio_util::sync::CancellationToken::new();
    if cancelled {
        cancel.cancel();
    }
    StreamControl {
        cancel,
        deadline: None,
        queue_id,
    }
}

fn released(queue_id: u64) -> Vec<serde_json::Value> {
    test_capture::events()
        .into_iter()
        .filter(|e| e["fields"]["queue_id"] == queue_id)
        .map(|e| e["fields"].clone())
        .collect()
}

#[test]
fn a_cancelled_stream_reports_its_release() {
    test_capture::install();
    trace_stream_release(&control(9_184_101, true), 12);
    let s4 = released(9_184_101);
    assert_eq!(s4.len(), 1, "{s4:?}");
    assert_eq!(s4[0]["message"], "worker released after stop");
    assert_eq!(s4[0]["stream"], true);
    assert_eq!(s4[0]["cancel_flag"], true);
}

#[test]
fn an_expired_stream_reports_its_release() {
    test_capture::install();
    let mut c = control(9_184_102, false);
    c.deadline = Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
    trace_stream_release(&c, 3);
    let s4 = released(9_184_102);
    assert_eq!(s4.len(), 1);
    assert_eq!(s4[0]["cancel_flag"], false);
}

#[test]
fn an_unstopped_stream_emits_nothing() {
    test_capture::install();
    trace_stream_release(&control(9_184_103, false), 7);
    assert!(released(9_184_103).is_empty());
}
