//! Test support for the release-wait tests: framing helpers, and a model
//! that holds its request until the request is stopped, through the
//! cancellable path the engine really uses.

use std::sync::atomic::{AtomicU64, Ordering};
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

/// Frame deadline floor: the historical fixed bound.
pub(super) const MIN_FRAME_DEADLINE: Duration = Duration::from_secs(120);
/// Frame deadline ceiling: a hung daemon must still fail the test.
#[cfg_attr(not(feature = "gguf"), allow(dead_code))]
pub(super) const MAX_FRAME_DEADLINE: Duration = Duration::from_secs(900);
/// A long real-model stream costs about this many short probes on the same
/// host under the same load; the margin keeps ordering assertions honest
/// while a contended host (CPU burners, a busy VM host) only slows the
/// probe and the stream alike.
#[cfg_attr(not(feature = "gguf"), allow(dead_code))]
const PROBE_MULTIPLE: u32 = 40;

static FRAME_DEADLINE_MS: AtomicU64 = AtomicU64::new(MIN_FRAME_DEADLINE.as_millis() as u64);

/// The current frame deadline: the floor, or a calibrated value for this
/// process (see [`calibrate_frame_deadline`]).
pub(super) fn frame_deadline() -> Duration {
    Duration::from_millis(FRAME_DEADLINE_MS.load(Ordering::Acquire))
}

/// Scale the frame deadline from the measured wall time of a short
/// real-model request on this host right now: `PROBE_MULTIPLE` probes,
/// clamped to `[MIN_FRAME_DEADLINE, MAX_FRAME_DEADLINE]`. The real-model
/// tests assert ordering (who is answered first), never absolute speed, so
/// a slow or contended host must not turn an ordering test into a timeout
/// while a hung daemon still fails within the ceiling. Never lowers the
/// deadline below the floor. Only the real-model (gguf) tests calibrate.
#[cfg_attr(not(feature = "gguf"), allow(dead_code))]
pub(super) fn calibrate_frame_deadline(probe: Duration) -> Duration {
    let scaled = probe.saturating_mul(PROBE_MULTIPLE);
    let chosen = scaled.clamp(MIN_FRAME_DEADLINE, MAX_FRAME_DEADLINE);
    FRAME_DEADLINE_MS.fetch_max(chosen.as_millis() as u64, Ordering::AcqRel);
    let effective = frame_deadline();
    eprintln!(
        "real-model frame deadline: probe {probe:?} x{PROBE_MULTIPLE} -> {effective:?} \
         (floor {MIN_FRAME_DEADLINE:?}, ceiling {MAX_FRAME_DEADLINE:?})"
    );
    effective
}

/// Next frame, within the (possibly calibrated) frame deadline.
pub(super) async fn receive(client: &mut DuplexStream) -> IpcMessage {
    let frame = read_frame(client, 1 << 20);
    let bytes = tokio::time::timeout(frame_deadline(), frame)
        .await
        .expect("frame deadline")
        .expect("frame");
    decode_message(&bytes).expect("decode")
}
