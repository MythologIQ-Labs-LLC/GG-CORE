//! Bounded CLI IPC frame exchange, shared by request/response and streaming.
//!
//! A peer that sends only part of a frame must never pin the CLI forever.
//! Each outbound frame and inbound frame has one complete operation deadline.
//! Streaming also has a total deadline, even when the peer keeps sending valid frames.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use super::{CliError, CliIpcClient};

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

impl CliIpcClient {
    pub(super) async fn write_request_frame<S>(
        &self,
        stream: &mut S,
        request: &[u8],
    ) -> Result<(), CliError>
    where
        S: AsyncWriteExt + Unpin,
    {
        let len = u32::try_from(request.len())
            .map_err(|_| CliError::Protocol("Request too large".into()))?;
        timeout(self.timeout_duration, async {
            stream.write_all(&len.to_le_bytes()).await?;
            stream.write_all(request).await?;
            stream.flush().await
        })
        .await
        .map_err(|_| CliError::Timeout)??;
        Ok(())
    }

    pub(super) async fn read_response_frame<S>(&self, stream: &mut S) -> Result<Vec<u8>, CliError>
    where
        S: AsyncReadExt + Unpin,
    {
        timeout(self.timeout_duration, async {
            let mut header = [0u8; 4];
            stream.read_exact(&mut header).await?;
            let len = u32::from_le_bytes(header) as usize;
            if len > MAX_RESPONSE_BYTES {
                return Err(CliError::Protocol("Response too large".into()));
            }
            let mut payload = vec![0u8; len];
            stream.read_exact(&mut payload).await?;
            Ok(payload)
        })
        .await
        .map_err(|_| CliError::Timeout)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn short_timeout() -> CliIpcClient {
        CliIpcClient::new("test".into()).with_timeout(std::time::Duration::from_millis(30))
    }

    #[tokio::test]
    async fn partial_response_header_times_out() {
        let (mut client, mut peer) = duplex(64);
        peer.write_all(&[4, 0]).await.unwrap();
        let result = short_timeout().read_response_frame(&mut client).await;
        assert!(matches!(result, Err(CliError::Timeout)), "{result:?}");
    }

    #[tokio::test]
    async fn advertised_body_without_remaining_bytes_times_out() {
        let (mut client, mut peer) = duplex(64);
        peer.write_all(&8u32.to_le_bytes()).await.unwrap();
        peer.write_all(b"x").await.unwrap();
        let result = short_timeout().read_response_frame(&mut client).await;
        assert!(matches!(result, Err(CliError::Timeout)), "{result:?}");
    }

    #[tokio::test]
    async fn peer_closed_mid_body_is_io_error_not_timeout() {
        let (mut client, mut peer) = duplex(64);
        peer.write_all(&4u32.to_le_bytes()).await.unwrap();
        peer.write_all(b"x").await.unwrap();
        drop(peer);
        let result = short_timeout().read_response_frame(&mut client).await;
        assert!(matches!(result, Err(CliError::Io(_))), "{result:?}");
    }

    #[tokio::test]
    async fn oversized_advertised_response_fails_before_allocation() {
        let (mut client, mut peer) = duplex(64);
        peer.write_all(&((MAX_RESPONSE_BYTES + 1) as u32).to_le_bytes())
            .await
            .unwrap();
        let result = short_timeout().read_response_frame(&mut client).await;
        assert!(matches!(result, Err(CliError::Protocol(_))), "{result:?}");
    }

    #[tokio::test]
    async fn valid_frame_round_trips_unchanged() {
        let (mut client, mut peer) = duplex(64);
        peer.write_all(&4u32.to_le_bytes()).await.unwrap();
        peer.write_all(b"pong").await.unwrap();
        let response = short_timeout()
            .read_response_frame(&mut client)
            .await
            .unwrap();
        assert_eq!(response, b"pong");
    }

    #[tokio::test]
    async fn stalled_request_write_times_out() {
        let (mut client, _nonreading_peer) = duplex(8);
        let result = short_timeout()
            .write_request_frame(&mut client, &[7; 4096])
            .await;
        assert!(matches!(result, Err(CliError::Timeout)), "{result:?}");
    }

    #[tokio::test]
    async fn request_header_and_body_have_correct_wire_shape() {
        let (mut client, mut peer) = duplex(64);
        short_timeout()
            .write_request_frame(&mut client, b"ping")
            .await
            .unwrap();
        let mut frame = [0u8; 8];
        peer.read_exact(&mut frame).await.unwrap();
        assert_eq!(frame, [4, 0, 0, 0, b'p', b'i', b'n', b'g']);
    }

    #[tokio::test]
    async fn streaming_exchange_times_out_on_partial_chunk() {
        let (mut client, mut peer) = duplex(64);
        let task = tokio::spawn(async move {
            let mut request = [0u8; 8];
            peer.read_exact(&mut request).await.unwrap();
            peer.write_all(&8u32.to_le_bytes()).await.unwrap();
            peer.write_all(b"x").await.unwrap();
            std::future::pending::<()>().await;
        });
        let result = short_timeout()
            .stream_exchange(&mut client, b"ping", crate::ipc::protocol::RequestId(1))
            .await;
        assert!(matches!(result, Err(CliError::Timeout)), "{result:?}");
        task.abort();
    }

    #[tokio::test]
    async fn mismatched_stream_chunk_is_refused_before_output() {
        use crate::ipc::protocol::{encode_message, IpcMessage, RequestId, StreamChunk};
        let (mut client, mut peer) = duplex(128);
        let payload = encode_message(&IpcMessage::StreamChunk(
            StreamChunk::final_token_with_text(RequestId(19), 3, "foreign output".into()),
        ))
        .unwrap();
        let task = tokio::spawn(async move {
            let mut request = [0u8; 8];
            peer.read_exact(&mut request).await.unwrap();
            peer.write_all(&(payload.len() as u32).to_le_bytes())
                .await
                .unwrap();
            peer.write_all(&payload).await.unwrap();
        });
        let result = short_timeout()
            .stream_exchange(&mut client, b"ping", RequestId(20))
            .await;
        assert!(matches!(result, Err(CliError::Protocol(msg)) if msg.contains("ID mismatch")));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn matching_empty_final_chunk_completes_stream() {
        use crate::ipc::protocol::{encode_message, IpcMessage, RequestId, StreamChunk};
        let (mut client, mut peer) = duplex(128);
        let payload = encode_message(&IpcMessage::StreamChunk(StreamChunk::complete(RequestId(
            20,
        ))))
        .unwrap();
        let task = tokio::spawn(async move {
            let mut request = [0u8; 8];
            peer.read_exact(&mut request).await.unwrap();
            peer.write_all(&(payload.len() as u32).to_le_bytes())
                .await
                .unwrap();
            peer.write_all(&payload).await.unwrap();
        });
        let output = short_timeout()
            .stream_exchange(&mut client, b"ping", RequestId(20))
            .await
            .unwrap();
        assert!(output.is_empty());
        task.await.unwrap();
    }

    #[tokio::test]
    async fn recurring_valid_chunks_cannot_extend_stream_deadline() {
        use crate::ipc::protocol::{encode_message, IpcMessage, RequestId, StreamChunk};
        let (mut client, mut peer) = duplex(256);
        let frame = encode_message(&IpcMessage::StreamChunk(
            StreamChunk::token(RequestId(1), 42),
        ))
        .unwrap();
        let server = tokio::spawn(async move {
            let mut request = [0u8; 8];
            peer.read_exact(&mut request).await.unwrap();
            loop {
                if peer.write_all(&(frame.len() as u32).to_le_bytes()).await.is_err() {
                    break;
                }
                if peer.write_all(&frame).await.is_err() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });
        let bounded = CliIpcClient::new("test".into())
            .with_timeout(std::time::Duration::from_millis(80));
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(400),
            bounded.stream_exchange(&mut client, b"ping", RequestId(1)),
        )
        .await
        .expect("continuous frames must not keep the CLI alive");
        assert!(matches!(result, Err(CliError::Timeout)), "{result:?}");
        server.abort();
    }

    #[tokio::test]
    async fn full_exchange_times_out_on_stalled_response_body() {
        let (mut client, mut peer) = duplex(64);
        let task = tokio::spawn(async move {
            let mut request = [0u8; 8];
            peer.read_exact(&mut request).await.unwrap();
            peer.write_all(&8u32.to_le_bytes()).await.unwrap();
            peer.write_all(b"x").await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });
        let result = short_timeout().exchange_data(&mut client, b"ping").await;
        assert!(matches!(result, Err(CliError::Timeout)), "{result:?}");
        task.abort();
    }
}
