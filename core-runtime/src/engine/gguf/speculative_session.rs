//! Persistent KV-cache session for speculative decoding (B-21f).
//!
//! The GGUF backend's `verify_tokens`/`generate_from_tokens` build a fresh
//! `LlamaContext` and re-decode the entire context every call — O(n)/step waste
//! that makes the wired speculative path net-slower (→ auto-disables). This
//! session keeps ONE context alive: the prompt is decoded once, each step decodes
//! only the committed delta, and speculative draft positions are rolled back after
//! verify via `clear_kv_cache_seq`, so the KV always holds exactly the committed
//! prefix.
//!
//! The obstacle is a borrow, not the API: `create_context(&self)` returns a
//! `LlamaContext` that borrows the model, so a persistent context is a
//! self-referential struct. `self_cell` owns `Arc<LlamaBackendInner>` and, tied to
//! it, the `LlamaContext` that borrows it.

use std::sync::Arc;

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use super::backend::{decode_range, LlamaBackendInner};
use crate::engine::speculative_types::VerifyResult;
use crate::engine::InferenceError;

/// Dependent side of the self-cell: the live context plus a greedy (argmax)
/// sampler. Greedy is required so verification is a deterministic oracle — a
/// draft token is accepted iff it equals the target's argmax at that position.
struct SessionCtx<'a> {
    ctx: LlamaContext<'a>,
    sampler: LlamaSampler,
}

self_cell::self_cell!(
    struct SessionCell {
        owner: Arc<LlamaBackendInner>,
        #[covariant]
        dependent: SessionCtx,
    }
);

/// A persistent-KV speculative decode session over one GGUF backend.
pub struct GgufSpeculativeSession {
    cell: SessionCell,
    /// Number of context tokens currently in the KV cache (positions `0..kv_len`).
    kv_len: usize,
}

// SAFETY: the session is only ever accessed serially — the verifier guards it
// behind a `Mutex` and the executor drives one decode step at a time. llama.cpp
// contexts are safe to use from a single thread; `Send` (moving the exclusive
// owner between threads) is therefore sound, mirroring `LlamaBackendInner`.
unsafe impl Send for GgufSpeculativeSession {}

impl GgufSpeculativeSession {
    /// Build a session and decode `prompt` once as the initial KV prefix.
    pub fn new(inner: Arc<LlamaBackendInner>, prompt: &[u32]) -> Result<Self, InferenceError> {
        let cell = SessionCell::try_new(inner, |o| {
            Ok::<_, InferenceError>(SessionCtx {
                ctx: o.create_context()?,
                sampler: LlamaSampler::greedy(),
            })
        })?;
        let mut session = Self { cell, kv_len: 0 };
        session.decode_at(prompt, 0, false)?;
        session.kv_len = prompt.len();
        Ok(session)
    }

    /// Decode `tokens` at `start_pos`. `logits_all` computes logits for every token
    /// (verification); otherwise only the last (prediction).
    fn decode_at(
        &mut self,
        tokens: &[u32],
        start_pos: usize,
        logits_all: bool,
    ) -> Result<(), InferenceError> {
        let toks: Vec<LlamaToken> = tokens.iter().map(|&t| LlamaToken(t as i32)).collect();
        self.cell.with_dependent_mut(|_o, d| {
            decode_range(&mut d.ctx, &toks, start_pos as i32, logits_all)
        })
    }

    /// Bring the KV to exactly `context` and guarantee the logit at index `-1`
    /// predicts the next position: reuse the shared prefix, then re-decode the last
    /// committed token so its logits are always fresh (llama.cpp keeps only the last
    /// batch's logits, and a prior verify's rollback leaves stale ones). The bulk
    /// prompt is decoded once and reused; only one token is re-decoded per step.
    fn seat(&mut self, context: &[u32]) -> Result<(), InferenceError> {
        let p = context.len();
        debug_assert!(p >= 1, "context must be non-empty");
        // Reuse [0, p-1): trim if the KV overran (prior draft), extend if behind.
        if self.kv_len > p - 1 {
            self.rollback_to(p - 1)?;
        } else if self.kv_len < p - 1 {
            self.decode_at(&context[self.kv_len..p - 1], self.kv_len, false)?;
        }
        self.kv_len = p - 1;
        // Re-decode the last committed token so its logit (predicting position p) is fresh.
        self.decode_at(&context[p - 1..p], p - 1, false)?;
        self.kv_len = p;
        Ok(())
    }

    /// Verify `draft` against the target at `context`. Greedily checks each draft
    /// token against the target's argmax, then rolls the draft positions back out of
    /// the KV so only the committed prefix remains.
    pub fn verify(
        &mut self,
        context: &[u32],
        draft: &[u32],
    ) -> Result<VerifyResult, InferenceError> {
        if draft.is_empty() {
            return Ok(VerifyResult::accept_all(0));
        }
        self.seat(context)?;
        let base = context.len();
        let toks: Vec<LlamaToken> = draft.iter().map(|&t| LlamaToken(t as i32)).collect();
        let outcome = self.cell.with_dependent_mut(|_o, d| {
            // draft[0] is predicted by the freshly-seated tail logit (index -1),
            // captured before the draft batch overwrites the logits.
            let pred0 = d.sampler.sample(&d.ctx, -1);
            decode_range(&mut d.ctx, &toks, base as i32, true)?;
            if pred0.0 as u32 != draft[0] {
                return Ok(VerifyResult::diverge_at(0, pred0.0 as u32));
            }
            for (i, &dtok) in draft.iter().enumerate().skip(1) {
                // draft[i] is predicted by draft[i-1]'s logit: draft-batch index i-1.
                let predicted = d.sampler.sample(&d.ctx, (i - 1) as i32);
                if predicted.0 as u32 != dtok {
                    return Ok(VerifyResult::diverge_at(i, predicted.0 as u32));
                }
            }
            Ok::<_, InferenceError>(VerifyResult::accept_all(draft.len()))
        })?;
        // The draft batch was always decoded, so this removes exactly the draft positions.
        self.rollback_to(base)?;
        self.kv_len = base;
        Ok(outcome)
    }

    /// Sample one token at `context` (does not commit it — the executor passes it
    /// back in `context` next step, so the KV stays the single source of truth).
    pub fn generate_one(&mut self, context: &[u32]) -> Result<u32, InferenceError> {
        self.seat(context)?;
        let tok = self
            .cell
            .with_dependent_mut(|_o, d| d.sampler.sample(&d.ctx, -1));
        Ok(tok.0 as u32)
    }

    /// Remove KV positions `[keep, ∞)` for sequence 0.
    fn rollback_to(&mut self, keep: usize) -> Result<(), InferenceError> {
        self.cell
            .with_dependent_mut(|_o, d| d.ctx.clear_kv_cache_seq(Some(0), Some(keep as u32), None))
            .map_err(|e| InferenceError::ModelError(format!("kv rollback: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "speculative_session_tests.rs"]
mod speculative_session_tests;
