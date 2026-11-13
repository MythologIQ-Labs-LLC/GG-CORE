//! Regression: a release-wait response blocked behind the connection's
//! writer must keep the one-wait gate held until the frame has been written.
//! This uses a real authenticated handler and the actual framed writer, but
//! does not need native inference or a model fixture.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::ipc::protocol::{decode_message, IpcMessage};
use crate::{Runtime, RuntimeConfig};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_release_wait_is_refused_while_first_reply_is_write_blocked() {
    let _test_lock = TEST_LOCK.lock().await;
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: "release-wait-order-test-token".into(),
        ..RuntimeConfig::default()
    });
    let handler = Arc::new(runtime.ipc_handler);
    let session = handler
        .auth
        .authenticate("release-wait-order-test-token")
        .await
        .expect("test authentication");

    let (mut reader, write_side) = tokio::io::duplex(1024);
    let writer = Arc::new(ConnWriter::new(write_side, Duration::from_secs(3)));
    let waits = Arc::new(ReleaseWaits::default());

    // Deliberately block every frame write. An unknown model has no held
    // instances, so the first wait finishes immediately but cannot reply.
    let held_writer = writer.hold().await;
    start(
        &handler,
        Some(&session),
        &writer,
        &waits,
        ModelReleaseWaitRequest::new("never-loaded", 0),
    )
    .await;

    // Observe the precise point after the lifecycle wait finished and
    // before the response tries to acquire the held writer. No clock races.
    tokio::time::timeout(Duration::from_secs(2), waits.reply_ready.notified())
        .await
        .expect("first wait reached its reply");
    assert_eq!(LIVE_WAITS.load(Ordering::SeqCst), 1);
    assert!(
        waits.pending.load(Ordering::SeqCst),
        "the first wait must remain pending until its frame is written"
    );

    let second_writer = Arc::clone(&writer);
    let second_handler = Arc::clone(&handler);
    let second_waits = Arc::clone(&waits);
    let second_session = session.clone();
    let second = tokio::spawn(async move {
        start(
            &second_handler,
            Some(&second_session),
            &second_writer,
            &second_waits,
            ModelReleaseWaitRequest::new("never-loaded", 0),
        )
        .await;
    });
    // The second request must be rejected without spawning another wait,
    // even if its rejection frame itself is blocked by the shared writer.
    tokio::time::timeout(Duration::from_secs(2), async {
        while waits.duplicate_rejections.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the second wait was refused");
    assert_eq!(LIVE_WAITS.load(Ordering::SeqCst), 1);
    drop(held_writer);

    let mut release_responses = 0;
    let mut duplicate_rejections = 0;
    for _ in 0..2 {
        let bytes = tokio::time::timeout(
            Duration::from_secs(3),
            super::super::read_frame(&mut reader, 4096),
        )
        .await
        .expect("response frame deadline")
        .expect("response frame");
        match decode_message(&bytes).expect("valid framed reply") {
            IpcMessage::ModelReleaseWaitResponse(w) => {
                assert_eq!(w.model_id, "never-loaded");
                assert!(w.released && w.pending_instances == 0);
                release_responses += 1;
            }
            IpcMessage::Error { code: 409, .. } => duplicate_rejections += 1,
            other => panic!("unexpected release-wait frame: {other:?}"),
        }
    }
    second.await.expect("rejection task finished");
    assert_eq!((release_responses, duplicate_rejections), (1, 1));
    waits.close();
    tokio::time::timeout(Duration::from_secs(2), async {
        while LIVE_WAITS.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("release waiter finished");
}
