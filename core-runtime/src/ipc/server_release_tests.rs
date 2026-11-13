//! `model_release_wait_request` through the real connection loop
//! (MODEL_RELEASE_V1_PLAN A9, R2, S1/S2): authenticated, one per connection,
//! off the reader (cancel is served while it waits), ended by disconnect, and
//! a listing shows a held instance exactly once.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tokio::io::DuplexStream;

use super::handle_connection;
use super::release::{LIVE_WAITS, TEST_LOCK};
use super::release_daemon::{daemon, generate, Daemon, TOKEN};
use super::release_stub::{receive, send};
use crate::ipc::protocol::*;

async fn connect(d: &Daemon, auth: bool) -> (DuplexStream, tokio::task::JoinHandle<()>) {
    let guard = d.pool.try_acquire_owned().expect("slot");
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(handle_connection(server, d.handler.clone(), guard, 1 << 20));
    if auth {
        let hello = IpcMessage::Handshake {
            token: TOKEN.into(),
            protocol_version: None,
        };
        send(&mut client, &hello).await;
        assert!(matches!(
            receive(&mut client).await,
            IpcMessage::HandshakeAck { .. }
        ));
    }
    (client, task)
}

fn wait(ms: u64) -> IpcMessage {
    IpcMessage::ModelReleaseWaitRequest(ModelReleaseWaitRequest::new("m", ms))
}

/// Start a request on `m`, give it time to be admitted, then unload `m`.
async fn hold_and_unload(client: &mut DuplexStream) {
    send(client, &generate(7)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let unload = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: "m".into(),
    });
    send(client, &unload).await;
    let reply = receive(client).await;
    assert!(
        matches!(reply, IpcMessage::ModelUnloadResponse(ref r) if r.success),
        "{reply:?}"
    );
}

/// The listing over IPC, on the same connection.
async fn listing(client: &mut DuplexStream) -> ModelsListResponse {
    send(client, &IpcMessage::ModelsRequest).await;
    match receive(client).await {
        IpcMessage::ModelsResponse(r) => r,
        other => panic!("expected models_response, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unauthenticated_wait_is_refused_and_the_connection_stays_open() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon().await;
    let (mut c, _t) = connect(&d, false).await;
    send(&mut c, &wait(0)).await;
    let reply = receive(&mut c).await;
    assert!(
        matches!(reply, IpcMessage::Error { code: 401, .. }),
        "{reply:?}"
    );
    assert_eq!(
        listing(&mut c).await.models.len(),
        1,
        "connection still open"
    );
    d.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_is_served_during_a_wait_which_then_reports_released() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon().await;
    let (mut c, _t) = connect(&d, true).await;
    hold_and_unload(&mut c).await;
    send(&mut c, &wait(10_000)).await;
    send(&mut c, &wait(10_000)).await; // a second pending wait on this connection
    let reply = receive(&mut c).await;
    assert!(
        matches!(reply, IpcMessage::Error { code: 409, .. }),
        "{reply:?}"
    );

    assert_listed_once(&mut c).await;

    drop(d.holder); // only the in-flight request holds the model now
    cancel_and_await_release(&mut c).await;
    assert!(d.lifecycle.releasing_models().is_empty());
    assert!(listing(&mut c).await.models.is_empty());
    d.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_wait_ends_when_its_connection_does() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon().await;
    let (mut c, task) = connect(&d, true).await;
    // Held by `d.holder` for the whole test: only the disconnect can end it.
    let unload = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: "m".into(),
    });
    send(&mut c, &unload).await;
    assert!(matches!(receive(&mut c).await, IpcMessage::ModelUnloadResponse(r) if r.success));
    send(&mut c, &wait(60_000)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(LIVE_WAITS.load(Ordering::SeqCst), 1);
    drop(c);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("connection closed")
        .expect("joined");
    let deadline = Instant::now() + Duration::from_secs(1);
    while LIVE_WAITS.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        LIVE_WAITS.load(Ordering::SeqCst),
        0,
        "wait outlived its connection"
    );
    d.shutdown.cancel();
}

/// The held instance appears exactly once, as unloading, by metadata name.
async fn assert_listed_once(c: &mut DuplexStream) {
    let listed = listing(c).await;
    assert_eq!(
        listed.models.len(),
        1,
        "held instance listed once: {listed:?}"
    );
    assert_eq!(listed.models[0].state, "unloading");
    assert_eq!(
        listed.models[0].name, "blocking-weights",
        "never the model id"
    );
    assert_eq!(listed.total_memory_bytes, 4096);
}

/// Cancel the holding request; collect its three outcomes in any order.
async fn cancel_and_await_release(c: &mut DuplexStream) {
    let start = Instant::now();
    let cancel = IpcMessage::CancelRequest {
        request_id: RequestId(7),
    };
    send(c, &cancel).await;
    let (mut acked, mut stopped, mut released) = (false, false, false);
    while !(acked && stopped && released) {
        match receive(c).await {
            IpcMessage::CancelResponse { cancelled, .. } => acked = cancelled,
            IpcMessage::InferenceResponse(r) => {
                stopped = r.error_code == Some(InferenceErrorCode::Cancelled);
            }
            IpcMessage::ModelReleaseWaitResponse(w) => {
                assert!(w.released && w.pending_instances == 0, "{w:?}");
                released = true;
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert!(start.elapsed() < Duration::from_secs(10));
}
