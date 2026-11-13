//! Prompt prefill and bounded decode ranges (moved out of `backend.rs`).
//!
//! Plain prompt prefill always follows the deterministic
//! [`prefill_schedule`](super::prefill_schedule), whether or not the caller
//! can stop the request, so the decode split, and with it greedy output,
//! never depends on load or on cancellation support (N7 successor, #184).
//! Speculative and verify ranges keep one batch-capped split
//! ([`decode_range`]); their logits contract is separate.

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::token::LlamaToken;

use super::backend::decode;
use super::prefill_schedule::{self, MAX_PREFILL_CHUNK};
use crate::engine::InferenceError;

/// Optional cooperative stop check, polled before every chunk.
pub(super) type StopCheck<'a> = Option<&'a (dyn Fn() -> bool + Send + Sync)>;

pub(super) fn stop_requested(should_stop: StopCheck<'_>) -> bool {
    should_stop.is_some_and(|f| f())
}

/// What the deterministic schedule depends on, besides position and cap.
#[derive(Debug, Clone, Copy)]
pub(super) struct PrefillShape {
    /// Resolved threads of the context.
    pub threads: u32,
    /// Model tensor bytes (`LlamaModel::size`).
    pub model_bytes: u64,
}

/// Prefill a prompt on the deterministic schedule, checking `should_stop`
/// before every chunk. Returns `Ok(false)` when stopped before the prompt
/// was fully decoded; the context is then incomplete and must be discarded.
pub(super) fn decode_prompt(
    ctx: &mut LlamaContext<'_>,
    tokens: &[LlamaToken],
    shape: PrefillShape,
    should_stop: StopCheck<'_>,
) -> Result<bool, InferenceError> {
    if tokens.is_empty() {
        return Ok(true);
    }
    let cap = decode_batch_cap(ctx, 0, tokens.len(), false, Some(MAX_PREFILL_CHUNK))?;
    let size =
        |pos, left| prefill_schedule::chunk(pos, left, shape.threads, shape.model_bytes, cap);
    decode_chunked(ctx, tokens, 0, false, &size, should_stop)
}

/// Decode a token range in batches bounded by the live llama.cpp context.
/// The last overall token receives logits for sampling. For all-position
/// speculative logits, too many tokens fail cleanly instead of returning
/// stale logits from a prior batch or triggering a native assert.
pub(super) fn decode_range(
    ctx: &mut LlamaContext<'_>,
    tokens: &[LlamaToken],
    start_pos: i32,
    logits_all: bool,
) -> Result<(), InferenceError> {
    if tokens.is_empty() {
        return Ok(());
    }
    let begin = usize::try_from(start_pos)
        .map_err(|_| InferenceError::InputValidation("negative decode position".into()))?;
    let cap = decode_batch_cap(ctx, begin, tokens.len(), logits_all, None)?;
    decode_chunked(ctx, tokens, begin, logits_all, &|_, _| cap, None).map(|_| ())
}

/// The chunk loop: `size(position, remaining)` gives each chunk's length
/// (clamped to what remains), and `should_stop` runs before every chunk.
fn decode_chunked(
    ctx: &mut LlamaContext<'_>,
    tokens: &[LlamaToken],
    begin: usize,
    logits_all: bool,
    size: &dyn Fn(usize, usize) -> usize,
    should_stop: StopCheck<'_>,
) -> Result<bool, InferenceError> {
    let mut offset = 0;
    while offset < tokens.len() {
        if stop_requested(should_stop) {
            return Ok(false);
        }
        let left = tokens.len() - offset;
        let len = size(begin + offset, left).clamp(1, left);
        #[cfg(test)]
        recorder::record(len);
        let last = offset + len == tokens.len();
        let chunk = &tokens[offset..offset + len];
        decode_chunk(ctx, chunk, begin + offset, logits_all, last)?;
        offset += len;
    }
    Ok(true)
}

/// Validate a decode range against the live context and return the largest
/// chunk it may use: both logical and physical decode capacity (the default
/// llama context can report a larger logical n_batch than physical n_ubatch).
fn decode_batch_cap(
    ctx: &LlamaContext<'_>,
    begin: usize,
    len: usize,
    logits_all: bool,
    chunk_cap: Option<usize>,
) -> Result<usize, InferenceError> {
    let end = begin
        .checked_add(len)
        .ok_or_else(|| InferenceError::InputValidation("decode position overflow".into()))?;
    if end > ctx.n_ctx() as usize {
        return Err(InferenceError::InputValidation(format!(
            "decode would exceed the {}-token context window",
            ctx.n_ctx()
        )));
    }
    let native_cap = ctx.n_batch().min(ctx.n_ubatch()) as usize;
    let batch_cap = chunk_cap.map_or(native_cap, |cap| cap.min(native_cap));
    if batch_cap == 0 {
        return Err(InferenceError::ModelError(
            "llama.cpp returned a zero-sized decode batch".into(),
        ));
    }
    if logits_all && len > batch_cap {
        return Err(InferenceError::InputValidation(
            "all-position logits require a single decode batch".into(),
        ));
    }
    Ok(batch_cap)
}

/// Decode one chunk at absolute position `pos0`. Only the final token of the
/// whole range (`last`) gets logits unless `logits_all` is set.
fn decode_chunk(
    ctx: &mut LlamaContext<'_>,
    chunk: &[LlamaToken],
    pos0: usize,
    logits_all: bool,
    last: bool,
) -> Result<(), InferenceError> {
    let mut batch = LlamaBatch::new(chunk.len(), 1);
    for (i, &tok) in chunk.iter().enumerate() {
        let pos = i32::try_from(pos0 + i)
            .map_err(|_| InferenceError::InputValidation("decode position exceeds i32".into()))?;
        let logits = logits_all || (last && i + 1 == chunk.len());
        batch
            .add(tok, pos, &[0], logits)
            .map_err(|e| InferenceError::ModelError(format!("batch: {e}")))?;
    }
    decode(ctx, &mut batch)
}

/// The split the schedule must produce for an `n`-token prompt on `backend`
/// (test oracle for the recorder).
#[cfg(test)]
pub(super) fn planned_split(backend: &super::backend::LlamaBackendInner, n: usize) -> Vec<usize> {
    let ctx = backend.create_context().expect("context");
    let cap = MAX_PREFILL_CHUNK.min(ctx.n_batch().min(ctx.n_ubatch()) as usize);
    let shape = backend.prefill_shape();
    let (mut pos, mut out) = (0, Vec::new());
    while pos < n {
        let len = prefill_schedule::chunk(pos, n - pos, shape.threads, shape.model_bytes, cap);
        out.push(len);
        pos += len;
    }
    out
}

#[cfg(test)]
#[path = "prefill_split_tests.rs"]
mod split_tests;

/// Test-only record of the chunk lengths `decode_chunked` actually decodes
/// on this thread, observed on the production call path (audit residual 1).
#[cfg(test)]
pub(super) mod recorder {
    use std::cell::RefCell;

    thread_local! {
        static CHUNKS: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
    }

    pub(super) fn record(len: usize) {
        CHUNKS.with(|c| {
            if let Some(v) = c.borrow_mut().as_mut() {
                v.push(len);
            }
        });
    }

    /// Run `f` and return its result with every chunk length it decoded.
    pub(in crate::engine::gguf) fn recording<T>(f: impl FnOnce() -> T) -> (T, Vec<usize>) {
        CHUNKS.with(|c| *c.borrow_mut() = Some(Vec::new()));
        let out = f();
        let chunks = CHUNKS.with(|c| c.borrow_mut().take()).unwrap_or_default();
        (out, chunks)
    }
}
