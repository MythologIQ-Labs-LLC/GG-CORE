//! The single write side of an IPC connection
//! (`docs/integration/V1_SCHEDULER_HOL_BLOCKING_PLAN.md`, Phase 1).
//!
//! Every frame on a connection goes through [`ConnWriter`]: length-prefixed
//! framing, a stall bound per frame, and poisoning. A frame write that stalls
//! or fails may already have put part of a frame on the wire; every later
//! frame would then be parsed against the wrong length. So the first such
//! failure poisons the connection: nothing more is written, and
//! [`ConnWriter::closed`] fires so the connection loop closes it and cancels
//! its requests.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Why a frame was not written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    #[error("connection closed after a stalled or failed frame write")]
    Poisoned,
    #[error("frame refused")]
    Refused,
    #[error("stream write stalled")]
    Stalled,
    #[error("frame write failed: {0}")]
    Io(String),
}

/// Shared, poisonable writer for one connection.
pub struct ConnWriter<W> {
    inner: Mutex<W>,
    poisoned: AtomicBool,
    stall: Duration,
    closed: CancellationToken,
}

impl<W: AsyncWrite + Unpin> ConnWriter<W> {
    /// Wrap a connection's write half; `stall` bounds every frame write.
    pub fn new(writer: W, stall: Duration) -> Self {
        Self {
            inner: Mutex::new(writer),
            poisoned: AtomicBool::new(false),
            stall,
            closed: CancellationToken::new(),
        }
    }

    /// Fires once the connection is poisoned.
    pub fn closed(&self) -> &CancellationToken {
        &self.closed
    }

    /// True after a stalled or failed frame write.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::Acquire)
    }

    /// Write one frame.
    pub async fn write_frame(&self, data: &[u8]) -> Result<(), WriteError> {
        self.write_frame_if(data, || true).await
    }

    /// Write one frame if `admit` (evaluated under the writer lock, before
    /// any byte is written) still allows it.
    pub async fn write_frame_if(
        &self,
        data: &[u8],
        admit: impl FnOnce() -> bool,
    ) -> Result<(), WriteError> {
        let mut writer = self.inner.lock().await;
        if self.is_poisoned() {
            return Err(WriteError::Poisoned);
        }
        if !admit() {
            return Err(WriteError::Refused);
        }
        match tokio::time::timeout(self.stall, write_framed(&mut *writer, data)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                self.poison();
                Err(WriteError::Io(e.to_string()))
            }
            Err(_) => {
                self.poison();
                Err(WriteError::Stalled)
            }
        }
    }

    /// Best-effort shutdown of the write half, itself bounded by `stall`.
    pub async fn shutdown(&self) {
        let mut writer = self.inner.lock().await;
        let _ = tokio::time::timeout(self.stall, writer.shutdown()).await;
    }

    fn poison(&self) {
        self.poisoned.store(true, Ordering::Release);
        self.closed.cancel();
    }

    /// Hold the writer lock (tests of lock-ordering guarantees).
    #[cfg(test)]
    pub(crate) async fn hold(&self) -> tokio::sync::MutexGuard<'_, W> {
        self.inner.lock().await
    }
}

pub(crate) async fn write_framed<W: AsyncWrite + Unpin>(
    writer: &mut W,
    data: &[u8],
) -> std::io::Result<()> {
    let len = u32::try_from(data.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame too large"))?;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await
}

#[cfg(test)]
#[path = "conn_writer_tests.rs"]
mod tests;
