//! R1a (owner disposition 2026-11-13): a client that stops reading can hold
//! the connection reader on an inline reply for at most the writer-stall
//! bound. Then the writer is poisoned, the connection ends, and teardown
//! (not the unread `cancel_request`) stops its work and its release wait.
//! Driven through the real `handle_connection` with a small duplex buffer.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

use super::handle_connection;
use super::read_frame;
use super::release::{LIVE_WAITS, TEST_LOCK};
use super::release_daemon::{daemon_with, generate, Daemon, TOKEN};
use super::release_stub::{receive, send};
use crate::ipc::conn_writer::write_framed;
use crate::ipc::protocol::*;

const STALL: Duration = Duration::from_millis(300);
/// Holds the handshake ack and the unload reply (both read by the client)
/// but not more than three unread ~80-byte 409 frames.
const BUFFER: usize = 256;

fn wait_request() -> IpcMessage {
    IpcMessage::ModelReleaseWaitRequest(ModelReleaseWaitRequest::new("m", 10_000))
}

/// Authenticate, hold `m` with request 7, unload it, then start a release
/// wait that stays pending while request 7 runs.
async fn connect_holding(d: &Daemon) -> (DuplexStream, tokio::task::JoinHandle<()>) {
    let guard = d.pool.try_acquire_owned().expect("slot");
    let (mut client, server) = tokio::io::duplex(BUFFER);
    let task = tokio::spawn(handle_connection(server, d.handler.clone(), guard, 1 << 20));
    let hello = IpcMessage::Handshake {
        token: TOKEN.into(),
        protocol_version: None,
    };
    send(&mut client, &hello).await;
    assert!(matches!(
        receive(&mut client).await,
        IpcMessage::HandshakeAck { .. }
    ));
    send(&mut client, &generate(7)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let unload = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: "m".into(),
    });
    send(&mut client, &unload).await;
    let reply = receive(&mut client).await;
    assert!(matches!(reply, IpcMessage::ModelUnloadResponse(ref r) if r.success));
    send(&mut client, &wait_request()).await;
    (client, task)
}

/// Poll `done` until it holds, failing after three seconds.
async fn eventually(what: &str, done: impl Fn() -> bool) {
    let poll = async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(3), poll)
        .await
        .unwrap_or_else(|_| panic!("timed out: {what}"));
}

/// Every frame the client can still read: only duplicate-wait 409s (the last
/// one may be cut off by the poisoned write). No cancel or release reply.
async fn assert_only_refusals(mut reader: ReadHalf<DuplexStream>) -> usize {
    let mut refusals = 0;
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(1), read_frame(&mut reader, 1 << 20));
        let Ok(Ok(bytes)) = frame.await else {
            return refusals;
        };
        match decode_message(&bytes).expect("decode") {
            IpcMessage::Error { code: 409, .. } => refusals += 1,
            other => panic!("unexpected frame after the reader blocked: {other:?}"),
        }
    }
}

/// The client, no longer reading, floods duplicate waits and then cancels
/// request 7. Its writes may block or fail once the server stops reading.
fn spawn_flood(mut writer: WriteHalf<DuplexStream>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let dup = encode_message(&wait_request()).expect("encode");
        for _ in 0..12 {
            if write_framed(&mut writer, &dup).await.is_err() {
                return;
            }
        }
        let cancel = IpcMessage::CancelRequest {
            request_id: RequestId(7),
        };
        let _ = write_framed(&mut writer, &encode_message(&cancel).expect("encode")).await;
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocked_inline_reply_ends_the_connection_within_the_stall_bound() {
    let _l = TEST_LOCK.lock().await;
    let d = daemon_with(STALL).await;
    drop(d.holder); // only request 7 holds `m` now
    let (client, task) = connect_holding(&d).await;
    eventually("release wait pending", || {
        LIVE_WAITS.load(Ordering::SeqCst) == 1
    })
    .await;
    let (reader, writer) = tokio::io::split(client);
    let started = Instant::now();
    let flood = spawn_flood(writer);
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("connection ended within the stall bound plus margin")
        .expect("connection task");
    let elapsed = started.elapsed();
    eprintln!(
        "R1A stall_ms={} connection_end_ms={}",
        STALL.as_millis(),
        elapsed.as_millis()
    );
    assert!(
        elapsed >= STALL,
        "the reader really blocked on an inline reply"
    );
    let lifecycle = d.lifecycle.clone();
    eventually("teardown released m", || {
        lifecycle.releasing_models().is_empty()
    })
    .await;
    eventually("release wait ended", || {
        LIVE_WAITS.load(Ordering::SeqCst) == 0
    })
    .await;
    assert!(assert_only_refusals(reader).await >= 1);
    flood.abort();
    d.shutdown.cancel();
}
