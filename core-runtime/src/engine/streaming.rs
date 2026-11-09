//! Token streaming output for incremental responses.

use std::sync::atomic::{AtomicU8, Ordering};

use tokio::sync::mpsc;

use super::streaming_bound::{bounded_send, StreamBound};

/// Reason a token stream ended.
///
/// Makes a normal completion distinguishable from a mid-stream security
/// rejection and from an engine error, so a client never mistakes a truncated
/// or aborted stream for a finished one (B-24 F2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamTerminal {
    /// Generation finished normally (end-of-generation token or token budget).
    Complete,
    /// Aborted by a security control. Reserved for egress sanitization (B-24b);
    /// the ingress-reject path also uses it.
    Rejected(String),
    /// The engine failed part-way through generation.
    Error(String),
}

/// A frame on a token stream: a generated token, a sanitized text chunk, or the
/// terminal that ends it.
///
/// `Text` is the client-facing frame for the security-enforced streaming path
/// (B-24b): the runtime detokenizes and egress-sanitizes, then emits text so raw
/// token ids never leave the runtime. `Token` remains for the internal/unsanitized
/// path and tests.
#[derive(Debug, Clone)]
pub enum StreamItem {
    Token(u32),
    Text(String),
    End(StreamTerminal),
}

/// Async stream of generated tokens, terminated by exactly one `End` frame.
pub struct TokenStream {
    receiver: mpsc::Receiver<StreamItem>,
}

impl TokenStream {
    /// Create a sender/receiver pair. Every send on the sender is bounded by
    /// `bound` (there is no unbounded stream).
    pub fn new(buffer_size: usize, bound: StreamBound) -> (TokenStreamSender, Self) {
        let (sender, receiver) = mpsc::channel(buffer_size);
        let sender = TokenStreamSender {
            sender,
            bound,
            failure: AtomicU8::new(NO_FAILURE),
        };
        (sender, Self { receiver })
    }

    /// Receive the next frame, if available. `None` once the sender is gone.
    pub async fn next(&mut self) -> Option<StreamItem> {
        self.receiver.recv().await
    }

    /// Collect all tokens and the terminal reason. A sender dropped without an
    /// explicit `End` yields `StreamTerminal::Error` — never report a dropped
    /// stream as clean.
    pub async fn collect(mut self) -> (Vec<u32>, StreamTerminal) {
        let mut tokens = Vec::new();
        while let Some(item) = self.next().await {
            match item {
                StreamItem::Token(t) => tokens.push(t),
                // `collect` is the token-oriented consumer; sanitized `Text` frames
                // (B-24b) are not tokens and are ignored here.
                StreamItem::Text(_) => {}
                StreamItem::End(terminal) => return (tokens, terminal),
            }
        }
        (
            tokens,
            StreamTerminal::Error("stream dropped before terminal".into()),
        )
    }
}

/// Sender half. Emit `token()` per generated token, then exactly one `end()`.
/// Once a send stalls past the bound the sender is closed for good; the
/// first failed send is remembered so a truncated stream is never reported
/// as complete.
pub struct TokenStreamSender {
    sender: mpsc::Sender<StreamItem>,
    bound: StreamBound,
    failure: AtomicU8,
}

const NO_FAILURE: u8 = 0;

impl TokenStreamSender {
    /// Send a generated token.
    pub async fn token(&self, token: u32) -> Result<(), StreamSendError> {
        self.send(StreamItem::Token(token)).await
    }

    /// Send a sanitized text chunk (security-enforced streaming path, B-24b).
    pub async fn text(&self, text: String) -> Result<(), StreamSendError> {
        self.send(StreamItem::Text(text)).await
    }

    /// Send the terminal frame, ending the stream.
    pub async fn end(&self, terminal: StreamTerminal) -> Result<(), StreamSendError> {
        self.send(StreamItem::End(terminal)).await
    }

    /// True once a send waited past the stall bound (consumer not reading).
    pub fn is_stalled(&self) -> bool {
        self.failure() == Some(StreamSendError::Stalled)
    }

    /// The first send failure on this stream, if any.
    pub fn failure(&self) -> Option<StreamSendError> {
        StreamSendError::from_code(self.failure.load(Ordering::Acquire))
    }

    async fn send(&self, item: StreamItem) -> Result<(), StreamSendError> {
        if self.is_stalled() {
            return Err(StreamSendError::Stalled);
        }
        let result = bounded_send(&self.sender, &self.bound, item).await;
        if let Err(e) = result {
            let _ = self.failure.compare_exchange(
                NO_FAILURE,
                e.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        result
    }
}

/// Why a stream send failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamSendError {
    /// The consumer is gone.
    Closed,
    /// The consumer did not read within the stall bound.
    Stalled,
    /// The request deadline passed while waiting for capacity.
    Deadline,
}

impl StreamSendError {
    fn code(self) -> u8 {
        match self {
            Self::Closed => 1,
            Self::Stalled => 2,
            Self::Deadline => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Closed),
            2 => Some(Self::Stalled),
            3 => Some(Self::Deadline),
            _ => None,
        }
    }
}

impl std::fmt::Display for StreamSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Closed => "stream closed",
            Self::Stalled => STREAM_STALLED,
            Self::Deadline => "stream deadline exceeded",
        };
        f.write_str(text)
    }
}

impl std::error::Error for StreamSendError {}

/// Client-visible reason for a stream ended by a non-reading consumer.
pub const STREAM_STALLED: &str = "stream consumer stalled";

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collect_returns_tokens_and_complete_terminal() {
        let (tx, rx) = TokenStream::new(4, StreamBound::default());
        tx.token(7).await.unwrap();
        tx.token(8).await.unwrap();
        tx.end(StreamTerminal::Complete).await.unwrap();
        let (tokens, terminal) = rx.collect().await;
        assert_eq!(tokens, vec![7, 8]);
        assert_eq!(terminal, StreamTerminal::Complete);
    }

    #[tokio::test]
    async fn error_terminal_is_distinct_from_completion() {
        let (tx, rx) = TokenStream::new(4, StreamBound::default());
        tx.token(1).await.unwrap();
        tx.end(StreamTerminal::Error("boom".into())).await.unwrap();
        let (tokens, terminal) = rx.collect().await;
        assert_eq!(tokens, vec![1]);
        assert!(matches!(terminal, StreamTerminal::Error(_)));
        assert_ne!(terminal, StreamTerminal::Complete);
    }

    #[tokio::test]
    async fn dropped_sender_reports_error_not_clean() {
        let (tx, rx) = TokenStream::new(4, StreamBound::default());
        tx.token(1).await.unwrap();
        drop(tx);
        let (tokens, terminal) = rx.collect().await;
        assert_eq!(tokens, vec![1]);
        assert!(
            matches!(terminal, StreamTerminal::Error(_)),
            "a dropped stream must never read as Complete"
        );
    }
}
