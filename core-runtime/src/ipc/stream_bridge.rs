//! Bridge between engine TokenStream and IPC wire protocol.
//!
//! Adapts the connection's single [`ConnWriter`] to send StreamChunk
//! messages as length-prefixed JSON frames.

use std::sync::Arc;

use tokio::io::AsyncWrite;
use tokio_util::sync::CancellationToken;

use super::conn_writer::ConnWriter;
use super::handler::{HandlerError, StreamSender};
use super::protocol::{encode_message, IpcMessage, RequestId};

/// Adapts an IPC connection's writer to the StreamSender trait, with
/// cancellation via CancellationToken.
pub struct IpcStreamBridge<W> {
    writer: Arc<ConnWriter<W>>,
    request_id: RequestId,
    cancel: CancellationToken,
}

impl<W> IpcStreamBridge<W> {
    /// Create a new stream bridge.
    pub fn new(
        writer: Arc<ConnWriter<W>>,
        request_id: RequestId,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            writer,
            request_id,
            cancel,
        }
    }

    /// Get the request ID this bridge is sending for.
    pub fn request_id(&self) -> RequestId {
        self.request_id
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Get the cancellation token for select! usage.
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> IpcStreamBridge<W> {
    /// Write one frame. The cancellation re-check runs under the same writer
    /// lock used by cancellation ACKs: a chunk that was waiting for the lock
    /// must not be emitted after cancellation was acknowledged.
    async fn write_frame(&self, data: &[u8], error_terminal: bool) -> Result<(), HandlerError> {
        let cancel = &self.cancel;
        self.writer
            .write_frame_if(data, || error_terminal || !cancel.is_cancelled())
            .await
            .map_err(|e| HandlerError::StreamSend(e.to_string()))
    }
}

#[async_trait::async_trait]
impl<W: AsyncWrite + Unpin + Send + 'static> StreamSender for IpcStreamBridge<W> {
    async fn send(&self, message: IpcMessage) -> Result<(), HandlerError> {
        // Cancellation blocks further data, but must not suppress the final
        // error frame that tells the client its stream actually stopped.
        let terminal = matches!(
            &message,
            IpcMessage::StreamChunk(chunk) if chunk.is_final && chunk.error.is_some()
        );
        if self.cancel.is_cancelled() && !terminal {
            return Err(HandlerError::StreamSend("cancelled".into()));
        }
        let bytes = encode_message(&message)?;
        self.write_frame(&bytes, terminal).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::Duration;
    use tokio::io::BufWriter;

    const STALL: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn test_stream_bridge_send() {
        // Use a Vec-backed writer that doesn't need a reader
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let writer = Arc::new(ConnWriter::new(BufWriter::new(cursor), STALL));
        let cancel = CancellationToken::new();
        let bridge = IpcStreamBridge::new(writer, RequestId(123), cancel);

        let chunk = super::super::protocol::StreamChunk::token(RequestId(123), 42);
        let result = bridge.send(IpcMessage::StreamChunk(chunk)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cancelled_stream_can_send_terminal_frame() {
        let writer = Arc::new(ConnWriter::new(
            BufWriter::new(Cursor::new(Vec::new())),
            STALL,
        ));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let bridge = IpcStreamBridge::new(writer, RequestId(123), cancel);
        let terminal =
            super::super::protocol::StreamChunk::error(RequestId(123), "cancelled".into());
        assert!(bridge.send(IpcMessage::StreamChunk(terminal)).await.is_ok());
    }

    #[tokio::test]
    async fn cancellation_fences_data_that_was_waiting_on_writer_lock() {
        let writer = Arc::new(ConnWriter::new(
            BufWriter::new(Cursor::new(Vec::new())),
            STALL,
        ));
        let held = writer.hold().await;
        let cancel = CancellationToken::new();
        let bridge = Arc::new(IpcStreamBridge::new(
            Arc::clone(&writer),
            RequestId(123),
            cancel.clone(),
        ));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let pending = tokio::spawn(async move {
            let _ = started_tx.send(());
            bridge.write_frame(b"late data", false).await
        });
        started_rx.await.expect("writer task started");
        tokio::task::yield_now().await;
        cancel.cancel();
        drop(held);
        assert!(pending.await.expect("writer task joined").is_err());
    }

    #[tokio::test]
    async fn test_stream_bridge_cancelled() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let writer = Arc::new(ConnWriter::new(BufWriter::new(cursor), STALL));
        let cancel = CancellationToken::new();
        cancel.cancel();

        let bridge = IpcStreamBridge::new(writer, RequestId(123), cancel);
        let chunk = super::super::protocol::StreamChunk::token(RequestId(123), 42);
        let result = bridge.send(IpcMessage::StreamChunk(chunk)).await;
        assert!(result.is_err());
    }
}
