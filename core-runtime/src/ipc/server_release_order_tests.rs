//! Regression: a release-wait response blocked behind the connection's
//! writer must keep the one-wait gate held until the frame has been written.
//! This uses a real authenticated handler and the actual framed writer, but
//! does not need native inference or a model fixture.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::DuplexStream;

use super::*;
use crate::ipc::auth::SessionToken;
use crate::ipc::protocol::{decode_message, IpcMessage};
use crate::{Runtime, RuntimeConfig};

const TOKEN: &str = "release-wait-order-test-token";

type Writer = Arc<ConnWriter<DuplexStream>>;

/// A real handler with an authenticated session.
async fn authed_handler() -> (Arc<IpcHandler>, SessionToken) {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: TOKEN.into(),
        ..RuntimeConfig::default()
    });
    let handler = Arc::new(runtime.ipc_handler);
    let session = handler.auth.authenticate(TOKEN).await.expect("auth");
    (handler, session)
}

/// Poll `done` until it holds, failing after two seconds.
async fn eventually(what: &str, done: impl Fn() -> bool) {
    let poll = async {
        while !done() {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(2), poll)
        .await
        .unwrap_or_else(|_| panic!("timed out: {what}"));
}

/// Read one frame and decode it.
async fn next_frame(reader: &mut DuplexStream) -> IpcMessage {
    let frame = super::super::read_frame(reader, 4096);
    let bytes = tokio::time::timeout(Duration::from_secs(3), frame)
        .await
        .expect("frame deadline")
        .expect("frame");
    decode_message(&bytes).expect("valid framed reply")
}

/// Start a wait for `model` from a spawned task, as a second request would
/// arrive while the first reply is blocked.
fn spawn_wait(
    handler: &Arc<IpcHandler>,
    session: &SessionToken,
    writer: &Writer,
    waits: &Arc<ReleaseWaits>,
    model: &'static str,
) -> tokio::task::JoinHandle<()> {
    let (h, s, w, ws) = (
        handler.clone(),
        session.clone(),
        writer.clone(),
        waits.clone(),
    );
    tokio::spawn(async move {
        start(
            &h,
            Some(&s),
            &w,
            &ws,
            ModelReleaseWaitRequest::new(model, 0),
        )
        .await;
    })
}

/// Drain the first reply and the 409 in either order; return their counts.
async fn drain_reply_and_refusal(reader: &mut DuplexStream) -> (usize, usize) {
    let (mut replies, mut refusals) = (0, 0);
    for _ in 0..2 {
        match next_frame(reader).await {
            IpcMessage::ModelReleaseWaitResponse(w) => {
                assert_eq!(w.model_id, "never-loaded", "reply misattributed");
                assert!(w.released && w.pending_instances == 0);
                replies += 1;
            }
            IpcMessage::Error { code: 409, .. } => refusals += 1,
            other => panic!("unexpected release-wait frame: {other:?}"),
        }
    }
    (replies, refusals)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_release_wait_is_refused_while_first_reply_is_write_blocked() {
    let _test_lock = TEST_LOCK.lock().await;
    let (handler, session) = authed_handler().await;
    let (mut reader, write_side) = tokio::io::duplex(1024);
    let writer: Writer = Arc::new(ConnWriter::new(write_side, Duration::from_secs(3)));
    let waits = Arc::new(ReleaseWaits::default());

    // Block every frame write. An unknown model has no held instances, so
    // the first wait finishes at once but cannot reply.
    let held_writer = writer.hold().await;
    spawn_wait(&handler, &session, &writer, &waits, "never-loaded");
    // Test-only signal: the wait finished and its reply has not yet taken
    // the writer. No clock races.
    let ready = tokio::time::timeout(Duration::from_secs(2), waits.reply_ready.notified());
    ready.await.expect("first wait reached its reply");
    assert_eq!(LIVE_WAITS.load(Ordering::SeqCst), 1);
    assert!(
        waits.pending.load(Ordering::SeqCst),
        "pending until written"
    );

    // A second wait is refused without spawning, even though its 409 is
    // itself blocked behind the held writer.
    let second = spawn_wait(&handler, &session, &writer, &waits, "second-must-not-run");
    eventually("second wait refused", || {
        waits.duplicate_rejections.load(Ordering::SeqCst) == 1
    })
    .await;
    assert_eq!(LIVE_WAITS.load(Ordering::SeqCst), 1);
    drop(held_writer);
    assert_eq!(drain_reply_and_refusal(&mut reader).await, (1, 1));
    second.await.expect("rejection task finished");
    assert_third_wait_runs(&handler, &session, &writer, &waits, &mut reader).await;
}

/// Once the first reply is fully written the gate reopens: a third wait runs
/// and answers for its own model.
async fn assert_third_wait_runs(
    handler: &Arc<IpcHandler>,
    session: &SessionToken,
    writer: &Writer,
    waits: &Arc<ReleaseWaits>,
    reader: &mut DuplexStream,
) {
    eventually("first reply completed", || {
        !waits.pending.load(Ordering::SeqCst) && LIVE_WAITS.load(Ordering::SeqCst) == 0
    })
    .await;
    let third = ModelReleaseWaitRequest::new("third-model-can-run", 0);
    start(handler, Some(session), writer, waits, third).await;
    match next_frame(reader).await {
        IpcMessage::ModelReleaseWaitResponse(w) => {
            assert_eq!(w.model_id, "third-model-can-run");
            assert!(w.released && w.pending_instances == 0);
        }
        other => panic!("gate did not reopen: {other:?}"),
    }
    waits.close();
    eventually("release waiter finished", || {
        LIVE_WAITS.load(Ordering::SeqCst) == 0
    })
    .await;
}
