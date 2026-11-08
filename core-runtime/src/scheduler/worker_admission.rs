//! Per-request working-memory admission for every worker path (generation,
//! streaming, typed). Sidecar of `worker.rs` (Section 4 Razor).

use crate::engine::InferenceEngine;
use crate::memory::{ResourceGuard, ResourceLimits};

type GuardResult = Result<Option<ResourceGuard>, String>;

pub(in crate::scheduler) async fn acquire_guard(
    engine: &InferenceEngine,
    limits: Option<&ResourceLimits>,
    model_id: &str,
) -> GuardResult {
    let Some(limits) = limits else {
        return Ok(None);
    };
    // Reserve this request's working memory (KV cache + compute) before the
    // backend allocates it: the model's metadata-derived estimate when known,
    // else the configured fallback. Resident weights were reserved once at
    // load (ModelLifecycle) and are never charged again per call.
    let bytes = match engine.working_memory_estimate(model_id).await {
        Some(estimate) => estimate,
        None => limits.max_memory_per_call(),
    };
    limits.try_acquire_working(bytes).map(Some).map_err(|e| {
        format!("{e} (request working memory; GG_CORE_MAX_TOTAL_MEMORY bounds it, GG_CORE_MAX_MEMORY_PER_CALL caps it when set)")
    })
}

#[cfg(test)]
#[path = "worker_admission_tests.rs"]
mod tests;
