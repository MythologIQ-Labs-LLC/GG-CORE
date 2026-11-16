//! Stop trace: switch parsing, exact target filter, span correlation, and a
//! writer that never blocks the emitting thread.

use std::time::{Duration, Instant};

use super::*;

fn capture(f: impl FnOnce()) -> String {
    let (writer, rx) = channel(4096);
    tracing::subscriber::with_default(subscriber(writer), f);
    rx.try_iter()
        .map(|line| String::from_utf8(line).unwrap())
        .collect()
}

#[test]
fn only_the_exact_value_one_enables_the_trace() {
    assert!(enabled(Some("1")));
    for off in [
        None,
        Some(""),
        Some("0"),
        Some("true"),
        Some("1 "),
        Some("yes"),
    ] {
        assert!(!enabled(off), "{off:?}");
    }
}

#[test]
fn only_the_exact_stop_target_is_emitted() {
    let out = capture(|| {
        tracing::info!(target: "gg_core::stop", "kept-event");
        tracing::info!(target: "gg_core::stopx", "prefix-sibling");
        tracing::info!(target: "gg_core::stop::inner", "child-target");
        tracing::info!(target: "core_runtime", prompt = "secret prompt", "other-event");
        tracing::error!("default-target");
    });
    assert!(out.contains("kept-event"), "{out}");
    for absent in [
        "prefix-sibling",
        "child-target",
        "other-event",
        "secret prompt",
        "default-target",
    ] {
        assert!(!out.contains(absent), "{absent} leaked: {out}");
    }
    assert_eq!(out.lines().count(), 1);
}

#[test]
fn events_carry_the_stop_scope_span_and_an_uptime_stamp() {
    let out = capture(|| {
        let span = tracing::info_span!(target: "gg_core::stop", "stop_scope", queue_id = 42u64, stream = false);
        let _entered = span.enter();
        tracing::info!(target: "gg_core::stop", phase = "prefill", "backend stop observed");
    });
    let line: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(line["target"], "gg_core::stop");
    assert_eq!(line["span"]["queue_id"], 42);
    assert_eq!(line["span"]["stream"], false);
    assert_eq!(line["fields"]["phase"], "prefill");
    let stamp = line["timestamp"].as_str().unwrap();
    assert!(stamp.trim().ends_with('s'), "uptime stamp: {stamp}");
}

#[test]
fn spans_of_other_targets_add_no_fields() {
    let out = capture(|| {
        let other = tracing::info_span!("request", prompt = "secret prompt");
        let _entered = other.enter();
        tracing::info!(target: "gg_core::stop", "stop answered to caller");
    });
    assert!(out.contains("stop answered to caller"));
    assert!(!out.contains("secret prompt"), "{out}");
}

#[test]
fn a_stalled_sink_never_blocks_the_emitting_thread() {
    let (writer, _never_drained) = channel(8);
    let before = dropped();
    let start = Instant::now();
    tracing::subscriber::with_default(subscriber(writer), || {
        for i in 0..5_000u32 {
            tracing::info!(target: "gg_core::stop", i, "flood");
        }
    });
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    assert!(
        dropped() >= before + 4_992,
        "full channel lines are counted"
    );
}
