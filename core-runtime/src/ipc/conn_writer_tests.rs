//! `ConnWriter`: one framed writer per connection, bounded per frame, and
//! poisoned by the first stalled or failed write (HOL plan V1/V5).

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::AsyncWrite;

use super::*;

const STALL: Duration = Duration::from_millis(150);

/// Accepts `budget` bytes, then never accepts another (a client that
/// stopped reading with a full pipe).
#[derive(Clone)]
struct Gate {
    written: Arc<Mutex<Vec<u8>>>,
    budget: usize,
}

impl Gate {
    fn new(budget: usize) -> Self {
        Self {
            written: Arc::new(Mutex::new(Vec::new())),
            budget,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.written.lock().unwrap().clone()
    }
}

impl AsyncWrite for Gate {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut written = self.written.lock().unwrap();
        let room = self.budget.saturating_sub(written.len());
        if room == 0 {
            return Poll::Pending;
        }
        let n = room.min(buf.len());
        written.extend_from_slice(&buf[..n]);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn healthy_write_is_one_length_prefixed_frame() {
    let gate = Gate::new(usize::MAX);
    let writer = ConnWriter::new(gate.clone(), STALL);
    writer.write_frame(b"abc").await.expect("write");
    assert_eq!(gate.bytes(), [3, 0, 0, 0, b'a', b'b', b'c']);
    assert!(!writer.is_poisoned());
}

#[tokio::test]
async fn stalled_write_returns_within_stall_and_poisons() {
    let writer = ConnWriter::new(Gate::new(0), STALL);
    let start = Instant::now();
    assert_eq!(writer.write_frame(b"data").await, Err(WriteError::Stalled));
    assert!(start.elapsed() < STALL * 5);
    assert!(writer.is_poisoned());
    assert!(
        writer.closed().is_cancelled(),
        "the connection loop is told to close"
    );
}

#[tokio::test]
async fn nothing_is_written_after_a_partial_frame() {
    // The 4-byte length prefix goes out, then the client stops reading.
    let gate = Gate::new(4);
    let writer = ConnWriter::new(gate.clone(), STALL);
    assert_eq!(
        writer.write_frame(b"payload").await,
        Err(WriteError::Stalled)
    );
    let after_stall = gate.bytes();
    assert_eq!(after_stall.len(), 4);

    let start = Instant::now();
    // Cancel ACK, error terminal and non-streaming response alike.
    for frame in [
        &b"cancel_response"[..],
        b"error terminal",
        b"inference_response",
    ] {
        assert_eq!(writer.write_frame(frame).await, Err(WriteError::Poisoned));
        assert_eq!(
            writer.write_frame_if(frame, || true).await,
            Err(WriteError::Poisoned)
        );
    }
    assert!(start.elapsed() < STALL, "a poisoned writer refuses at once");
    assert_eq!(gate.bytes(), after_stall, "zero bytes after poisoning");
}

#[tokio::test]
async fn admit_is_checked_under_the_lock_before_any_byte() {
    let gate = Gate::new(usize::MAX);
    let writer = ConnWriter::new(gate.clone(), STALL);
    assert_eq!(
        writer.write_frame_if(b"late", || false).await,
        Err(WriteError::Refused)
    );
    assert!(gate.bytes().is_empty());
    assert!(
        !writer.is_poisoned(),
        "a refused frame is not a broken connection"
    );
}

#[tokio::test]
async fn a_stalled_write_releases_the_lock_within_stall() {
    let writer = Arc::new(ConnWriter::new(Gate::new(0), Duration::from_millis(300)));
    let stalled = {
        let writer = Arc::clone(&writer);
        tokio::spawn(async move { writer.write_frame(b"stuck").await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let start = Instant::now();
    let second = tokio::time::timeout(Duration::from_secs(2), writer.write_frame(b"next"))
        .await
        .expect("the lock must be released by the stall bound");
    assert_eq!(second, Err(WriteError::Poisoned));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(stalled.await.expect("joined"), Err(WriteError::Stalled));
}
