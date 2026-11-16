//! Backend stop observation for the #184 stop trace (S3).
//!
//! Every native decode goes through [`timed_decode`], which records the
//! duration and token count of the most recent decode on this thread. The
//! cooperative stop checks call [`setup_stop`] (request entry) or
//! [`stopped`] (between prefill chunks / generated tokens); when a stop is
//! seen they emit `backend stop observed` with what the last native call
//! cost. One request runs on one blocking thread from entry to return, and
//! the record is reset on every request entry (audit A1), so a previous
//! request's decode is never reported. Integers and fixed strings only.

use std::cell::Cell;
use std::time::Instant;

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_batch::LlamaBatch;

use super::prefill::{stop_requested, StopCheck};
use crate::engine::InferenceError;
use crate::telemetry::stop_trace::{dropped, STOP_TARGET};

/// (duration in microseconds, tokens) of the last decode on this thread.
type LastDecode = Option<(u64, usize)>;

thread_local! {
    static LAST_DECODE: Cell<LastDecode> = const { Cell::new(None) };
}

/// Where in the request a stop was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// Before the context was created.
    Setup,
    /// Between prompt-prefill chunks.
    Prefill,
    /// Before a generated token.
    Decode,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Setup => "setup",
            Phase::Prefill => "prefill",
            Phase::Decode => "decode",
        }
    }
}

/// One native decode, timed for the stop trace.
pub(super) fn timed_decode(
    ctx: &mut LlamaContext<'_>,
    batch: &mut LlamaBatch,
) -> Result<(), InferenceError> {
    let tokens = usize::try_from(batch.n_tokens()).unwrap_or(0);
    let start = Instant::now();
    let result = ctx.decode(batch);
    let micros = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
    LAST_DECODE.with(|last| last.set(Some((micros, tokens))));
    result.map_err(|e| InferenceError::ModelError(format!("decode: {e}")))
}

/// Request-entry check: always resets this thread's record, then checks.
pub(super) fn setup_stop(should_stop: StopCheck<'_>) -> bool {
    LAST_DECODE.with(|last| last.set(None));
    stopped(should_stop, Phase::Setup, 0, 0)
}

/// Cooperative checkpoint: on a stop, emit S3 and reset the record.
/// `done` is the prompt position (prefill) or tokens generated (decode);
/// `remaining` is prompt tokens still to decode (prefill only).
pub(super) fn stopped(
    should_stop: StopCheck<'_>,
    phase: Phase,
    done: usize,
    remaining: usize,
) -> bool {
    if !stop_requested(should_stop) {
        return false;
    }
    let last = LAST_DECODE.with(|last| last.replace(None));
    tracing::info!(
        target: STOP_TARGET,
        phase = phase.as_str(),
        done,
        remaining,
        last_decode_tokens = last.map(|(_, t)| t),
        last_decode_ms = last.map(|(us, _)| us as f64 / 1000.0),
        dropped = dropped(),
        "backend stop observed"
    );
    true
}

/// Test view of this thread's record.
#[cfg(test)]
pub(super) fn last_decode() -> LastDecode {
    LAST_DECODE.with(Cell::get)
}

/// Test hook: pretend a decode of `tokens` took `micros` on this thread.
#[cfg(test)]
pub(super) fn record_for_test(micros: u64, tokens: usize) {
    LAST_DECODE.with(|last| last.set(Some((micros, tokens))));
}

#[cfg(test)]
#[path = "stop_observe_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "stop_observe_real_tests.rs"]
mod real_tests;
