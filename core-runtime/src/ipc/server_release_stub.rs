//! Test support for the release-wait tests: framing helpers, and a model
//! that holds its request until the request is stopped, through the
//! cancellable path the engine really uses.

use std::time::Duration;

use tokio::io::DuplexStream;

use super::read_frame;
use crate::ipc::conn_writer::write_framed;
use crate::ipc::protocol::{decode_message, encode_message, IpcMessage};

use crate::engine::{
    FinishReason, GenerationResult, InferenceCapability, InferenceConfig, InferenceError,
    InferenceInput, InferenceOutput, Model,
};

/// Holds itself until its request is stopped (overrides the cancellable path).
pub(super) struct Blocking;

#[async_trait::async_trait]
impl Model for Blocking {
    fn model_id(&self) -> &str {
        "blocking"
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        4096
    }
    async fn infer(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "ok".into(),
            tokens_generated: 1,
            finish_reason: FinishReason::Stop,
        }))
    }
    async fn infer_cancellable(
        &self,
        _: &InferenceInput,
        _: &InferenceConfig,
        is_stopped: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> Result<InferenceOutput, InferenceError> {
        while !is_stopped.is_some_and(|stopped| stopped()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(InferenceError::ModelError("stopped".into()))
    }
    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub(super) async fn send(client: &mut DuplexStream, message: &IpcMessage) {
    let data = encode_message(message).expect("encode");
    write_framed(client, &data).await.expect("send");
}

/// Next frame, with a generous deadline (real-model generation included).
pub(super) async fn receive(client: &mut DuplexStream) -> IpcMessage {
    let frame = read_frame(client, 1 << 20);
    let bytes = tokio::time::timeout(Duration::from_secs(120), frame)
        .await
        .expect("frame deadline")
        .expect("frame");
    decode_message(&bytes).expect("decode")
}
