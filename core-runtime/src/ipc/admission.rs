//! Explicit, bounded rejection of connections beyond the connection cap.
//!
//! A client admitted at the transport layer (accepted socket, connected
//! named-pipe instance) but refused by `ConnectionPool` receives one
//! `503` error frame instead of a silent close, so callers can tell bounded
//! admission rejection from an unexpected transport failure. Each rejection
//! is time-bounded, reads at most one small frame, and at most
//! `MAX_INFLIGHT_REJECTIONS` run at once; beyond that the stream is simply
//! closed, so a flood of over-cap clients cannot pin tasks or memory.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Error frame sent to a client refused by the connection cap.
pub(crate) const OVER_CAPACITY_FRAME: &[u8] =
    br#"{"type":"error","code":503,"message":"Connection limit reached"}"#;

/// Upper bound on the whole rejection exchange.
const REJECT_DEADLINE: Duration = Duration::from_millis(1000);

/// Largest client frame drained before closing (handshake-sized).
const MAX_DRAIN_BYTES: usize = 4096;

/// Concurrent rejection replies; excess over-cap clients are closed silently.
const MAX_INFLIGHT_REJECTIONS: usize = 32;

static INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Reply 503 to an over-capacity client in a bounded background task.
pub(super) fn spawn_rejection<S>(stream: S)
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    if INFLIGHT.fetch_add(1, Ordering::AcqRel) >= MAX_INFLIGHT_REJECTIONS {
        INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        return; // dropping the stream closes it
    }
    tokio::spawn(async move {
        reject_over_capacity(stream).await;
        INFLIGHT.fetch_sub(1, Ordering::AcqRel);
    });
}

/// Send the 503 frame, drain at most one small client frame so the client's
/// pending write completes and it can read the rejection, then close.
async fn reject_over_capacity<S: AsyncReadExt + AsyncWriteExt + Unpin>(mut stream: S) {
    let _ = tokio::time::timeout(REJECT_DEADLINE, async {
        let len = (OVER_CAPACITY_FRAME.len() as u32).to_le_bytes();
        stream.write_all(&len).await?;
        stream.write_all(OVER_CAPACITY_FRAME).await?;
        stream.flush().await?;
        let mut buf = [0u8; MAX_DRAIN_BYTES];
        let _ = stream.read(&mut buf).await?;
        Ok::<(), std::io::Error>(())
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn over_capacity_client_reads_explicit_503_after_its_write() {
        let (server, mut client) = tokio::io::duplex(8192);
        let task = tokio::spawn(reject_over_capacity(server));
        let hs = br#"{"type":"handshake","token":"t"}"#;
        client
            .write_all(&(hs.len() as u32).to_le_bytes())
            .await
            .unwrap();
        client.write_all(hs).await.unwrap();
        let mut len = [0u8; 4];
        client.read_exact(&mut len).await.unwrap();
        let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
        client.read_exact(&mut body).await.unwrap();
        assert_eq!(body, OVER_CAPACITY_FRAME);
        task.await.unwrap();
        let mut rest = Vec::new();
        assert_eq!(
            client.read_to_end(&mut rest).await.unwrap(),
            0,
            "closed after 503"
        );
    }

    #[tokio::test]
    async fn silent_peer_cannot_pin_rejection_task() {
        let (server, _client) = tokio::io::duplex(8192);
        // Peer never writes or reads further; the deadline must end the task.
        tokio::time::timeout(REJECT_DEADLINE * 2, reject_over_capacity(server))
            .await
            .expect("rejection must be time-bounded");
    }
}
