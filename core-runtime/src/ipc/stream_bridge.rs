//! Bridge between engine TokenStream and IPC wire protocol.
//!
//! Adapts the connection write half to send StreamChunk messages
//! as length-prefixed JSON frames.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::handler::{HandlerError, StreamSender};
use super::protocol::{encode_message, IpcMessage, RequestId};

/// Adapts an IPC connection's write half to the StreamSender trait.
///
/// Wraps the write half in Arc<Mutex<>> for shared access and supports
/// cancellation via CancellationToken.
pub struct IpcStreamBridge<W> {
    writer: Arc<Mutex<W>>,
    request_id: RequestId,
    cancel: CancellationToken,
}

impl<W> IpcStreamBridge<W> {
    /// Create a new stream bridge.
    pub fn new(writer: Arc<Mutex<W>>, request_id: RequestId, cancel: CancellationToken) -> Self {
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

impl<W: AsyncWriteExt + Unpin + Send + 'static> IpcStreamBridge<W> {
    /// Write a length-prefixed frame to the underlying writer.
    async fn write_frame(&self, data: &[u8], error_terminal: bool) -> Result<(), HandlerError> {
        let mut writer = self.writer.lock().await;
        // Re-check under the same writer lock used by cancellation ACKs:
        // a chunk that was waiting for the lock must not be emitted after
        // cancellation was acknowledged on that connection.
        if self.cancel.is_cancelled() && !error_terminal {
            return Err(HandlerError::StreamSend("cancelled".into()));
        }
        let len = data.len() as u32;
        writer
            .write_all(&len.to_le_bytes())
            .await
            .map_err(|e| HandlerError::StreamSend(e.to_string()))?;
        writer
            .write_all(data)
            .await
            .map_err(|e| HandlerError::StreamSend(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| HandlerError::StreamSend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<W: AsyncWriteExt + Unpin + Send + 'static> StreamSender for IpcStreamBridge<W> {
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
    use tokio::io::BufWriter;

    #[tokio::test]
    async fn test_stream_bridge_send() {
        // Use a Vec-backed writer that doesn't need a reader
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let writer = Arc::new(Mutex::new(BufWriter::new(cursor)));
        let cancel = CancellationToken::new();
        let bridge = IpcStreamBridge::new(writer, RequestId(123), cancel);

        let chunk = super::super::protocol::StreamChunk::token(RequestId(123), 42);
        let result = bridge.send(IpcMessage::StreamChunk(chunk)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cancelled_stream_can_send_terminal_frame() {
        let writer = Arc::new(Mutex::new(BufWriter::new(Cursor::new(Vec::new()))));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let bridge = IpcStreamBridge::new(writer, RequestId(123), cancel);
        let terminal =
            super::super::protocol::StreamChunk::error(RequestId(123), "cancelled".into());
        assert!(bridge.send(IpcMessage::StreamChunk(terminal)).await.is_ok());
    }

    #[tokio::test]
    async fn test_stream_bridge_cancelled() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let writer = Arc::new(Mutex::new(BufWriter::new(cursor)));
        let cancel = CancellationToken::new();
        cancel.cancel();

        let bridge = IpcStreamBridge::new(writer, RequestId(123), cancel);
        let chunk = super::super::protocol::StreamChunk::token(RequestId(123), 42);
        let result = bridge.send(IpcMessage::StreamChunk(chunk)).await;
        assert!(result.is_err());
    }
}
