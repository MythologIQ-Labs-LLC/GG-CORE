//! #184 stop trace through the real connection loop: S1 only after
//! authentication, S2 carries the stop kind and pairs with S1 by
//! `request_id`, and no prompt text is ever emitted.

use super::*;
use crate::telemetry::stop_trace::test_capture;

fn stage(message: &str, request_id: u64) -> Vec<serde_json::Value> {
    test_capture::events()
        .into_iter()
        .filter(|e| e["fields"]["message"] == message && e["fields"]["request_id"] == request_id)
        .map(|e| e["fields"].clone())
        .collect()
}

async fn drain(client: &mut DuplexStream, frames: usize) {
    for _ in 0..frames {
        let _ = receive(client).await;
    }
}

#[tokio::test]
async fn cancel_emits_s1_then_s2_with_the_same_request_id() {
    test_capture::install();
    let (mut client, task, _queue) = start().await;
    authenticate(&mut client).await;
    send(&mut client, request(18_401, Some(60_000))).await;
    let cancel = IpcMessage::CancelRequest {
        request_id: RequestId(18_401),
    };
    send(&mut client, cancel).await;
    drain(&mut client, 2).await;
    let unknown = IpcMessage::CancelRequest {
        request_id: RequestId(18_499),
    };
    send(&mut client, unknown).await;
    drain(&mut client, 1).await;

    let s1 = stage("cancel request received", 18_401);
    assert_eq!(s1.len(), 1, "{s1:?}");
    assert_eq!(s1[0]["matched"], true);
    let s2 = stage("stop answered to caller", 18_401);
    assert_eq!(s2.len(), 1, "{s2:?}");
    assert_eq!(s2[0]["kind"], "cancelled");
    assert_eq!(s2[0]["stream"], false);
    assert!(s2[0]["queue_id"].is_u64());
    let miss = stage("cancel request received", 18_499);
    assert_eq!(miss.len(), 1);
    assert_eq!(miss[0]["matched"], false);
    assert!(
        !test_capture::raw().contains("must be cancellable"),
        "prompt text leaked"
    );
    drop(client);
    task.await.expect("connection task");
}

#[tokio::test]
async fn deadline_emits_s2_kind_deadline_exceeded_and_no_s1() {
    test_capture::install();
    let (mut client, task, _queue) = start().await;
    authenticate(&mut client).await;
    send(&mut client, request(18_402, Some(80))).await;
    drain(&mut client, 1).await;
    let s2 = stage("stop answered to caller", 18_402);
    assert_eq!(s2.len(), 1, "{s2:?}");
    assert_eq!(s2[0]["kind"], "deadline_exceeded");
    assert!(stage("cancel request received", 18_402).is_empty());
    drop(client);
    task.await.expect("connection task");
}

#[tokio::test]
async fn unauthenticated_cancel_emits_nothing() {
    test_capture::install();
    let (mut client, task, _queue) = start().await;
    let cancel = IpcMessage::CancelRequest {
        request_id: RequestId(18_403),
    };
    send(&mut client, cancel).await;
    let _ = receive(&mut client).await; // 401
    assert!(stage("cancel request received", 18_403).is_empty());
    drop(client);
    let _ = task.await;
}
