//! Adaptive speculative decode executor (B-21c).
//!
//! Promotes the `run_step` compose loop (draft → confidence → plan → verify →
//! commit) into a real outer decode loop that owns the context, honors `max_tokens`
//! and EOS, records telemetry, and — critically — **never commits a rejected draft
//! suffix**: only `accepted_count` draft tokens plus an optional correction are
//! committed, mirroring the verified v2 `speculative_step`. Every dead end (empty
//! draft, fallback plan, zero accepts) degrades to a single target token, so the
//! loop always makes progress and never emits speculative garbage.
//!
//! Model-side interfaces (`BlockDraftModel`/`TargetVerifier`) are trait objects
//! (backend-swappable); the policy side (`HeuristicConfidenceEstimator`/
//! `AdaptiveVerificationScheduler`) is concrete — v1 has no learned confidence heads.

use std::time::Instant;

use super::heuristic::{AdaptiveVerificationScheduler, HeuristicConfidenceEstimator};
use super::telemetry::SpeculativeTelemetry;
use super::{BlockDraftModel, ConfidenceEstimator, TargetVerifier, VerificationScheduler};
use crate::engine::InferenceError;

/// Composes the adaptive-speculative pieces into a production decode loop.
pub struct AdaptiveSpeculativeExecutor<'a> {
    drafter: &'a dyn BlockDraftModel,
    verifier: &'a dyn TargetVerifier,
    estimator: &'a HeuristicConfidenceEstimator,
    scheduler: &'a AdaptiveVerificationScheduler,
    telemetry: &'a SpeculativeTelemetry,
    max_draft: usize,
}

impl<'a> AdaptiveSpeculativeExecutor<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        drafter: &'a dyn BlockDraftModel,
        verifier: &'a dyn TargetVerifier,
        estimator: &'a HeuristicConfidenceEstimator,
        scheduler: &'a AdaptiveVerificationScheduler,
        telemetry: &'a SpeculativeTelemetry,
        max_draft: usize,
    ) -> Self {
        Self {
            drafter,
            verifier,
            estimator,
            scheduler,
            telemetry,
            max_draft,
        }
    }

    /// Generate up to `max_tokens` tokens for `prompt_tokens`, returning the
    /// generated tokens (excluding the prompt). Stops at `max_tokens` or EOS.
    pub async fn run(
        &self,
        prompt_tokens: &[u32],
        max_tokens: usize,
    ) -> Result<Vec<u32>, InferenceError> {
        let mut context = prompt_tokens.to_vec();
        let mut generated: Vec<u32> = Vec::new();
        let eos = self.verifier.eos_token();

        while generated.len() < max_tokens {
            let step = self.step(&context).await?;
            if step.is_empty() {
                break; // no progress possible
            }
            for tok in step {
                context.push(tok);
                generated.push(tok);
                if Some(tok) == eos || generated.len() >= max_tokens {
                    return Ok(generated);
                }
            }
        }
        Ok(generated)
    }

    /// One speculative step. Returns the tokens committed this step (never a
    /// rejected suffix); falls back to a single target token on any dead end.
    async fn step(&self, context: &[u32]) -> Result<Vec<u32>, InferenceError> {
        let draft_start = Instant::now();
        let block = self.drafter.draft(context, self.max_draft).await?;
        let draft_us = draft_start.elapsed().as_micros() as u64;
        if block.is_empty() {
            return Ok(vec![self.verifier.generate_one(context).await?]);
        }

        let profile = self.estimator.estimate(context, &block);
        let plan = self.scheduler.plan(&block, &profile);
        if plan.is_fallback() {
            return Ok(vec![self.verifier.generate_one(context).await?]);
        }

        let verify_start = Instant::now();
        let result = self.verifier.verify(context, &block, &plan).await?;
        let verify_us = verify_start.elapsed().as_micros() as u64;

        let accepted = result.accepted_count;
        let draft_len = block.tokens.len();
        self.scheduler.record_result(accepted, plan.window);
        self.estimator
            .record_acceptance(accepted as f32 / draft_len.max(1) as f32);
        self.telemetry
            .record_step(draft_len as u32, accepted as u32, draft_us, verify_us);
        // B-21h: also emit the Prometheus counters so the CLI `status` (an IPC
        // client) can surface live speculative stats via the metrics channel.
        crate::telemetry::record_speculative_cycle(accepted, draft_len.saturating_sub(accepted));

        let committed = result.into_tokens(&block.tokens);
        if committed.is_empty() {
            // Nothing accepted and no correction: single target token, never stall.
            return Ok(vec![self.verifier.generate_one(context).await?]);
        }
        Ok(committed)
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod executor_tests;
