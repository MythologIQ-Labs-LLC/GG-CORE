//! Token streaming output for incremental responses.

use tokio::sync::mpsc;

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
    /// Create a new token stream with sender/receiver pair.
    pub fn new(buffer_size: usize) -> (TokenStreamSender, Self) {
        let (sender, receiver) = mpsc::channel(buffer_size);
        (TokenStreamSender { sender }, Self { receiver })
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
pub struct TokenStreamSender {
    sender: mpsc::Sender<StreamItem>,
}

impl TokenStreamSender {
    /// Send a generated token.
    pub async fn token(&self, token: u32) -> Result<(), StreamSendError> {
        self.sender
            .send(StreamItem::Token(token))
            .await
            .map_err(|_| StreamSendError)
    }

    /// Send a sanitized text chunk (security-enforced streaming path, B-24b).
    pub async fn text(&self, text: String) -> Result<(), StreamSendError> {
        self.sender
            .send(StreamItem::Text(text))
            .await
            .map_err(|_| StreamSendError)
    }

    /// Send the terminal frame, ending the stream.
    pub async fn end(&self, terminal: StreamTerminal) -> Result<(), StreamSendError> {
        self.sender
            .send(StreamItem::End(terminal))
            .await
            .map_err(|_| StreamSendError)
    }
}

#[derive(Debug)]
pub struct StreamSendError;

impl std::fmt::Display for StreamSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream closed")
    }
}

impl std::error::Error for StreamSendError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collect_returns_tokens_and_complete_terminal() {
        let (tx, rx) = TokenStream::new(4);
        tx.token(7).await.unwrap();
        tx.token(8).await.unwrap();
        tx.end(StreamTerminal::Complete).await.unwrap();
        let (tokens, terminal) = rx.collect().await;
        assert_eq!(tokens, vec![7, 8]);
        assert_eq!(terminal, StreamTerminal::Complete);
    }

    #[tokio::test]
    async fn error_terminal_is_distinct_from_completion() {
        let (tx, rx) = TokenStream::new(4);
        tx.token(1).await.unwrap();
        tx.end(StreamTerminal::Error("boom".into())).await.unwrap();
        let (tokens, terminal) = rx.collect().await;
        assert_eq!(tokens, vec![1]);
        assert!(matches!(terminal, StreamTerminal::Error(_)));
        assert_ne!(terminal, StreamTerminal::Complete);
    }

    #[tokio::test]
    async fn dropped_sender_reports_error_not_clean() {
        let (tx, rx) = TokenStream::new(4);
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
