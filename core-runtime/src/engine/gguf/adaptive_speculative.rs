//! Adaptive-speculative GGUF adapter (B-21c).
//!
//! Implements the adaptive-speculative model-side traits over a `GgufGenerator`,
//! so the [`AdaptiveSpeculativeExecutor`] can drive a GGUF draft/target pair via
//! the block-level `BlockDraftModel`/`TargetVerifier`.
//!
//! Degradation: the GGUF backend surfaces no per-token draft log-probs, so
//! [`DraftBlock::from_tokens`] fills them with `-inf` and the heuristic estimator
//! leans on temperature/repetition/history signal (see ADR-007 §Implementation
//! Status). A real speedup additionally needs KV-cache reuse across steps (B-21f).
//!
//! [`AdaptiveSpeculativeExecutor`]: crate::engine::adaptive_speculative::executor::AdaptiveSpeculativeExecutor

use std::sync::Mutex;

use async_trait::async_trait;

use super::speculative_session::GgufSpeculativeSession;
use super::GgufGenerator;
use crate::engine::adaptive_speculative::{
    BlockDraftModel, DraftBlock, TargetVerifier, VerificationPlan, VerificationResult,
};
use crate::engine::InferenceError;

/// Draft-model adapter: generates a block of candidate tokens from a GGUF model.
/// Borrows the generator (obtained via `Model::as_any` downcast at the call site).
pub struct GgufBlockDraftModel<'a> {
    generator: &'a GgufGenerator,
}

impl<'a> GgufBlockDraftModel<'a> {
    pub fn new(generator: &'a GgufGenerator) -> Self {
        Self { generator }
    }
}

#[async_trait]
impl BlockDraftModel for GgufBlockDraftModel<'_> {
    async fn draft(&self, context: &[u32], max: usize) -> Result<DraftBlock, InferenceError> {
        let tokens = self.generator.generate_tokens(context, max).await?;
        // No per-token log-probs from the backend -> confidence runs degraded (B-21f).
        Ok(DraftBlock::from_tokens(tokens))
    }
}

/// Target-verifier adapter: greedily verifies draft tokens against a GGUF model,
/// reusing one persistent KV session across steps (B-21f). The session is created
/// lazily on the first call from the incoming prompt context.
pub struct GgufTargetVerifier<'a> {
    generator: &'a GgufGenerator,
    session: Mutex<Option<GgufSpeculativeSession>>,
}

impl<'a> GgufTargetVerifier<'a> {
    pub fn new(generator: &'a GgufGenerator) -> Self {
        Self {
            generator,
            session: Mutex::new(None),
        }
    }

    /// Get-or-create the persistent session (keyed to the initial prompt context)
    /// and run `f` against it. The lock is held only for the synchronous decode —
    /// never across an `.await` — so the future stays `Send`.
    fn with_session<R>(
        &self,
        context: &[u32],
        f: impl FnOnce(&mut GgufSpeculativeSession) -> Result<R, InferenceError>,
    ) -> Result<R, InferenceError> {
        let mut guard = self
            .session
            .lock()
            .map_err(|_| InferenceError::ModelError("session lock poisoned".into()))?;
        if guard.is_none() {
            let inner = self
                .generator
                .backend_arc()
                .ok_or_else(|| InferenceError::ModelError("no model loaded".into()))?;
            *guard = Some(GgufSpeculativeSession::new(inner, context)?);
        }
        f(guard.as_mut().expect("session initialized above"))
    }
}

#[async_trait]
impl TargetVerifier for GgufTargetVerifier<'_> {
    async fn verify(
        &self,
        context: &[u32],
        draft: &DraftBlock,
        _plan: &VerificationPlan,
    ) -> Result<VerificationResult, InferenceError> {
        // Greedy first-divergence check over the whole draft; the plan window is
        // advisory. KV reuse: only the committed delta + draft are decoded, and the
        // draft positions are rolled back after the check.
        let vr = self.with_session(context, |s| s.verify(context, &draft.tokens))?;
        Ok(match vr.correction_token {
            Some(correction) => VerificationResult::reject_at(vr.accepted_count, correction),
            None => VerificationResult::accept_all(vr.accepted_count),
        })
    }

    async fn generate_one(&self, context: &[u32]) -> Result<u32, InferenceError> {
        self.with_session(context, |s| s.generate_one(context))
    }

    fn eos_token(&self) -> Option<u32> {
        self.generator.eos_token_id()
    }
}
