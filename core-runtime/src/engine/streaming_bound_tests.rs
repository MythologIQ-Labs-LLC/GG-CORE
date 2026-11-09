//! Bounded token-stream sends: a non-reading consumer can hold a producer
//! for at most the stall bound, never past the request deadline.

use std::time::{Duration, Instant};

use super::*;
use crate::engine::{StreamTerminal, TokenStream};

const STALL: Duration = Duration::from_millis(150);
const GENEROUS: Duration = Duration::from_secs(2);

fn bound(stall: Duration) -> StreamBound {
    StreamBound {
        stall,
        deadline: None,
    }
}

#[tokio::test]
async fn end_on_a_full_channel_returns_within_stall() {
    let (tx, _rx) = TokenStream::new(1, bound(STALL));
    tx.token(1).await.expect("first slot");
    let start = Instant::now();
    let result = tokio::time::timeout(GENEROUS, tx.end(StreamTerminal::Complete))
        .await
        .expect("bounded end must not hang");
    assert_eq!(result, Err(StreamSendError::Stalled));
    assert!(start.elapsed() < GENEROUS);
}

#[tokio::test]
async fn a_stall_closes_the_sender_for_good() {
    let (tx, mut rx) = TokenStream::new(1, bound(STALL));
    tx.token(1).await.expect("first slot");
    assert_eq!(tx.token(2).await, Err(StreamSendError::Stalled));
    assert!(tx.is_stalled());
    // Capacity is free again, but the stream is already lost.
    let _ = rx.next().await;
    let start = Instant::now();
    assert_eq!(tx.token(3).await, Err(StreamSendError::Stalled));
    assert!(start.elapsed() < STALL, "a closed sender must fail at once");
}

#[tokio::test]
async fn a_stalled_stream_never_reads_as_complete() {
    let (tx, rx) = TokenStream::new(1, bound(STALL));
    tx.token(1).await.expect("first slot");
    assert!(tx.end(StreamTerminal::Complete).await.is_err());
    drop(tx);
    let (_tokens, terminal) = rx.collect().await;
    assert!(matches!(terminal, StreamTerminal::Error(_)));
}

#[tokio::test]
async fn no_send_waits_past_the_deadline() {
    let deadline = Instant::now() + Duration::from_millis(120);
    let b = StreamBound {
        stall: Duration::from_secs(30),
        deadline: Some(deadline),
    };
    let (tx, _rx) = TokenStream::new(1, b);
    tx.token(1).await.expect("first slot");
    let result = tokio::time::timeout(GENEROUS, tx.token(2))
        .await
        .expect("deadline must bound the send");
    assert_eq!(result, Err(StreamSendError::Deadline));
    assert!(!tx.is_stalled(), "a deadline is not a consumer stall");
}

#[tokio::test]
async fn terminal_after_the_deadline_is_delivered_when_capacity_is_free() {
    let b = StreamBound {
        stall: DEFAULT_STREAM_STALL,
        deadline: Some(Instant::now()),
    };
    let (tx, rx) = TokenStream::new(2, b);
    tx.end(StreamTerminal::Error("deadline".into()))
        .await
        .expect("free slot takes the terminal");
    drop(tx);
    let (_tokens, terminal) = rx.collect().await;
    assert_eq!(terminal, StreamTerminal::Error("deadline".into()));
}

#[test]
fn for_request_clamps_stall_into_the_documented_range() {
    let tiny = StreamBound::for_request(Duration::from_millis(1), Duration::from_secs(30));
    assert_eq!(tiny.stall, MIN_STREAM_STALL);
    let long = StreamBound::for_request(Duration::from_secs(60), Duration::from_secs(2));
    assert_eq!(
        long.stall,
        Duration::from_secs(2),
        "never longer than timeout"
    );
    assert!(long.deadline.is_some());
}
