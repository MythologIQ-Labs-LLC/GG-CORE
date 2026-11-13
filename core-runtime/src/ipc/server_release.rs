//! `model_release_wait_request` on a connection (Section 4 Razor sidecar).
//!
//! The wait runs as a spawned task that writes through the connection's
//! shared writer, so the reader keeps serving `cancel_request` (including a
//! cancel for the very request holding the model) and sees disconnects. At
//! most one wait is pending per connection, and every wait ends when its
//! connection does (within the writer-stall bound if its reply is being
//! written), so pending waits never exceed live connections.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::encode_message;
use crate::ipc::auth::SessionToken;
use crate::ipc::conn_writer::ConnWriter;
use crate::ipc::handler::IpcHandler;
use crate::ipc::protocol::{IpcMessage, ModelReleaseWaitRequest, ModelReleaseWaitResponse};

/// Release waits currently running in this process (observability/tests).
pub(super) static LIVE_WAITS: AtomicUsize = AtomicUsize::new(0);

/// Per-connection release-wait state.
#[derive(Default)]
pub(super) struct ReleaseWaits {
    pending: Arc<AtomicBool>,
    closed: CancellationToken,
    /// Per-connection observation of the first wait reaching its reply.
    #[cfg(test)]
    reply_ready: Arc<tokio::sync::Notify>,
    /// Number of duplicate waits refused while a reply is still pending.
    #[cfg(test)]
    duplicate_rejections: Arc<AtomicUsize>,
}

impl ReleaseWaits {
    /// End every wait of this connection (call on every connection exit).
    pub(super) fn close(&self) {
        self.closed.cancel();
    }
}

/// Authenticate, enforce one pending wait, then spawn it. Failures answer
/// with an error frame and keep the connection open.
pub(super) async fn start<W: AsyncWriteExt + Unpin + Send + 'static>(
    handler: &Arc<IpcHandler>,
    session: Option<&SessionToken>,
    writer: &Arc<ConnWriter<W>>,
    waits: &ReleaseWaits,
    request: ModelReleaseWaitRequest,
) {
    let authed = match session {
        Some(sess) => handler.auth.validate(sess).await.is_ok(),
        None => false,
    };
    if !authed {
        let err = r#"{"type":"error","code":401,"message":"Not authenticated"}"#;
        let _ = writer.write_frame(err.as_bytes()).await;
        return;
    }
    if waits.pending.swap(true, Ordering::SeqCst) {
        #[cfg(test)]
        waits.duplicate_rejections.fetch_add(1, Ordering::SeqCst);
        let err = r#"{"type":"error","code":409,"message":"release wait already pending"}"#;
        let _ = writer.write_frame(err.as_bytes()).await;
        return;
    }
    let task = Wait {
        handler: Arc::clone(handler),
        writer: Arc::clone(writer),
        pending: Arc::clone(&waits.pending),
        closed: waits.closed.clone(),
        #[cfg(test)]
        reply_ready: Arc::clone(&waits.reply_ready),
    };
    tokio::spawn(task.run(request));
}

struct Wait<W> {
    handler: Arc<IpcHandler>,
    writer: Arc<ConnWriter<W>>,
    pending: Arc<AtomicBool>,
    closed: CancellationToken,
    #[cfg(test)]
    reply_ready: Arc<tokio::sync::Notify>,
}

impl<W: AsyncWriteExt + Unpin + Send + 'static> Wait<W> {
    async fn run(self, request: ModelReleaseWaitRequest) {
        LIVE_WAITS.fetch_add(1, Ordering::SeqCst);
        let lifecycle = Arc::clone(&self.handler.model_lifecycle);
        let wait = lifecycle.wait_released(&request.model_id, request.clamped_wait());
        let outcome = tokio::select! {
            () = self.closed.cancelled() => None,
            outcome = wait => Some(outcome),
        };
        // Test-only signal: the result is known and the reply has not yet
        // acquired the connection writer. No production timing dependency.
        #[cfg(test)]
        self.reply_ready.notify_one();
        if let Some((released, pending)) = outcome {
            let body = ModelReleaseWaitResponse::new(request.model_id, released, pending);
            if let Ok(bytes) = encode_message(&IpcMessage::ModelReleaseWaitResponse(body)) {
                // Keep the one-wait gate held until the reply is fully written.
                // This prevents two successful waits from overlapping. A
                // separate 409 rejection can still arrive in either order;
                // clients must not pipeline release waits on one connection.
                let _ = self.writer.write_frame(&bytes).await;
            }
        }
        self.pending.store(false, Ordering::SeqCst);
        LIVE_WAITS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Serialises tests that start release waits (so [`LIVE_WAITS`] is exact)
/// and real-model daemon tests (so CPU contention cannot skew their timing).
#[cfg(test)]
pub(super) static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
#[path = "server_release_order_tests.rs"]
mod order_tests;

#[cfg(test)]
#[path = "server_release_stall_tests.rs"]
mod stall_tests;
