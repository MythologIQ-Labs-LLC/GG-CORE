//! Streaming relay: worker token stream to IPC frames
//! (`docs/integration/V1_SCHEDULER_HOL_BLOCKING_PLAN.md`, Phase 1).
//!
//! The token stream is bounded (`StreamBound`), and every frame write is
//! bounded by the connection's `ConnWriter`. A frame write is never started
//! inside a `select!` arm and never aborted part-way: cancellation and the
//! request deadline decide only whether the *next* frame starts, so framing
//! can never be desynchronized by them.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::{HandlerError, IpcHandler, StreamSender};
use crate::engine::{StreamBound, StreamItem, StreamTerminal, TokenStream};
use crate::ipc::protocol::{InferenceRequest, IpcMessage, RequestId, StreamChunk};

/// Token channel capacity between the worker and the relay.
const STREAM_CAPACITY: usize = 32;

/// What the relay writes next.
enum Frame {
    /// A data chunk; the stream continues.
    Data(StreamChunk),
    /// The worker's own terminal.
    Terminal(StreamChunk),
    /// A terminal the relay writes on cancel, deadline or a vanished worker.
    Abort(StreamChunk),
}

impl IpcHandler {
    /// Enqueue the request into the streaming queue so the worker loop
    /// handles resource limits and telemetry, then relay its frames.
    pub(super) async fn run_streaming_inference(
        &self,
        request: InferenceRequest,
        sender: &dyn StreamSender,
        cancel: CancellationToken,
    ) -> Result<(), HandlerError> {
        let request_id = request.request_id;
        let config = request.parameters.to_config();
        let timeout = Duration::from_millis(config.timeout_ms);
        let bound = StreamBound::for_request(self.config.stream_stall, timeout);
        let (token_sender, mut stream) = TokenStream::new(STREAM_CAPACITY, bound);
        self.queue
            .enqueue_streaming_controlled(
                request.model_id,
                request.prompt,
                config,
                token_sender,
                cancel.clone(),
            )
            .await
            .map_err(|e| HandlerError::QueueFull(e.to_string()))?;
        let result = relay_stream(request_id, &mut stream, sender, &cancel, timeout).await;
        if result.is_err() {
            // The client can no longer be written to: stop generation; the
            // dropped receiver fails the worker's next send immediately.
            cancel.cancel();
        }
        result
    }
}

/// Relay frames until a terminal. One deadline for the whole request, never
/// reset per chunk.
async fn relay_stream(
    request_id: RequestId,
    stream: &mut TokenStream,
    sender: &dyn StreamSender,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<(), HandlerError> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        let frame = tokio::select! {
            biased;
            () = cancel.cancelled() => Frame::Abort(StreamChunk::error(request_id, "cancelled".into())),
            () = &mut deadline => {
                cancel.cancel();
                Frame::Abort(StreamChunk::error(request_id, "stream deadline exceeded".into()))
            }
            item = stream.next() => frame_for(request_id, item),
        };
        match frame {
            Frame::Data(chunk) => sender.send(IpcMessage::StreamChunk(chunk)).await?,
            Frame::Terminal(chunk) => return sender.send(IpcMessage::StreamChunk(chunk)).await,
            Frame::Abort(chunk) => {
                let _ = sender.send(IpcMessage::StreamChunk(chunk)).await;
                return Ok(());
            }
        }
    }
}

fn frame_for(request_id: RequestId, item: Option<StreamItem>) -> Frame {
    match item {
        Some(StreamItem::Token(token)) => Frame::Data(StreamChunk::token(request_id, token)),
        // Security-enforced streaming (B-24b): sanitized text, no raw token id.
        Some(StreamItem::Text(text)) => {
            Frame::Data(StreamChunk::token_with_text(request_id, 0, text))
        }
        Some(StreamItem::End(StreamTerminal::Complete)) => {
            Frame::Terminal(StreamChunk::complete(request_id))
        }
        Some(StreamItem::End(StreamTerminal::Rejected(m) | StreamTerminal::Error(m))) => {
            Frame::Terminal(StreamChunk::error(request_id, m))
        }
        // Producer dropped without a terminal (worker panic/abort, or a
        // stalled terminal): the client must still get a final frame.
        None => Frame::Abort(StreamChunk::error(
            request_id,
            "stream ended without a terminal".into(),
        )),
    }
}
