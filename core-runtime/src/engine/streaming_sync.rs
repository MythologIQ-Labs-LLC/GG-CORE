//! Bounded native-stream sends that do not depend on a Tokio runtime.
//! A blocking GGUF worker may survive cancellation, shutdown_timeout, or a
//! dropped async caller. It must remain bounded even after the reactor stops.

use std::sync::atomic::Ordering;

use super::{StreamItem, StreamSendError, StreamTerminal, TokenStreamSender, NO_FAILURE};
use crate::engine::streaming_bound::bounded_send_blocking;

impl TokenStreamSender {
    pub(crate) fn token_blocking(&self, token: u32) -> Result<(), StreamSendError> {
        self.send_blocking(StreamItem::Token(token))
    }

    pub(crate) fn text_blocking(&self, text: String) -> Result<(), StreamSendError> {
        self.send_blocking(StreamItem::Text(text))
    }

    pub(crate) fn end_blocking(&self, terminal: StreamTerminal) -> Result<(), StreamSendError> {
        self.send_blocking(StreamItem::End(terminal))
    }

    fn send_blocking(&self, item: StreamItem) -> Result<(), StreamSendError> {
        if self.is_stalled() {
            return Err(StreamSendError::Stalled);
        }
        let result = bounded_send_blocking(&self.sender, &self.bound, item);
        if let Err(err) = result {
            let _ = self.failure.compare_exchange(
                NO_FAILURE,
                err.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        result
    }
}

#[cfg(test)]
#[path = "streaming_sync_tests.rs"]
mod tests;
