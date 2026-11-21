//! Production GGUF stream lookup and terminal delivery must not require
//! a Tokio runtime after caller cancellation or process runtime teardown.

use super::*;
use crate::engine::{StreamBound, StreamTerminal, TokenStream};

#[test]
fn missing_model_native_stream_sends_terminal_without_runtime() {
    let engine = InferenceEngine::new(4096);
    let (sender, receiver) = TokenStream::new(2, StreamBound::default());
    let result = engine.run_stream_sync(
        "missing-model",
        "hello",
        &InferenceConfig::default(),
        sender,
        None,
        None,
    );
    assert!(matches!(result, Err(InferenceError::ModelNotLoaded(_))));
    let (_, terminal) = futures::executor::block_on(receiver.collect());
    assert!(
        matches!(terminal, StreamTerminal::Error(message) if message.contains("missing-model")),
        "a missing model must terminate its stream explicitly"
    );
}
