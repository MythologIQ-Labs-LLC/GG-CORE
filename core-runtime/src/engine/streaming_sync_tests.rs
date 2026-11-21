//! Model-free proof that native token streaming works with no Tokio runtime
//! and rejects dead/stalled consumers within the configured time bound.

use std::time::{Duration, Instant};

use super::*;
use crate::engine::{StreamBound, TokenStream};

#[test]
fn native_sender_stalls_in_bounded_time_without_a_runtime() {
    let bound = StreamBound {
        stall: Duration::from_millis(80),
        deadline: None,
    };
    let (sender, _receiver) = TokenStream::new(1, bound);
    sender.token_blocking(1).expect("first frame fits");
    let start = Instant::now();
    assert_eq!(sender.token_blocking(2), Err(StreamSendError::Stalled));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "native sender was not bounded by its stall deadline"
    );
    assert_eq!(sender.failure(), Some(StreamSendError::Stalled));
    assert_eq!(
        sender.end_blocking(StreamTerminal::Complete),
        Err(StreamSendError::Stalled),
        "a truncated stream must not be reported complete"
    );
}

#[test]
fn native_sender_observes_closed_receiver_without_a_runtime() {
    let (sender, receiver) = TokenStream::new(1, StreamBound::default());
    drop(receiver);
    assert_eq!(
        sender.token_blocking(1),
        Err(StreamSendError::Closed)
    );
    assert_eq!(sender.failure(), Some(StreamSendError::Closed));
}

#[test]
fn native_terminal_is_sent_if_capacity_exists_even_after_deadline() {
    let bound = StreamBound {
        stall: Duration::from_millis(100),
        deadline: Some(Instant::now() - Duration::from_millis(1)),
    };
    let (sender, receiver) = TokenStream::new(1, bound);
    sender
        .end_blocking(StreamTerminal::Complete)
        .expect("free slot permits terminal after deadline");
    let (tokens, terminal) = futures::executor::block_on(receiver.collect());
    assert!(tokens.is_empty());
    assert_eq!(terminal, StreamTerminal::Complete);
}

#[test]
fn native_sender_never_delivers_a_late_token_past_deadline() {
    let (sender, receiver) = TokenStream::new(
        1,
        StreamBound {
            stall: Duration::from_secs(2),
            deadline: Some(Instant::now() + Duration::from_millis(40)),
        },
    );
    sender.token_blocking(1).unwrap();
    assert_eq!(
        sender.token_blocking(2),
        Err(StreamSendError::Deadline),
        "full channel must stop at the absolute request deadline"
    );
    assert_eq!(sender.failure(), Some(StreamSendError::Deadline));
    drop(receiver);
}

#[test]
fn native_sender_unblocks_on_reader_drop_without_a_reactor() {
    let (sender, reader) = TokenStream::new(
        1,
        StreamBound {
            stall: Duration::from_secs(2),
            deadline: None,
        },
    );
    sender.token_blocking(1).unwrap();
    let producer = std::thread::spawn(move || sender.token_blocking(2));
    std::thread::sleep(Duration::from_millis(20));
    drop(reader);
    assert_eq!(producer.join().expect("producer thread"), Err(StreamSendError::Closed));
}
