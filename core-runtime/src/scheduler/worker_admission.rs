//! Per-request working-memory admission for every worker path (generation,
//! streaming, typed). Sidecar of `worker.rs` (Section 4 Razor).

use crate::engine::InferenceEngine;
use crate::memory::{ResourceGuard, ResourceLimits};

type GuardResult = Result<Option<ResourceGuard>, String>;

/// Refusal for a model whose GGUF metadata cannot bound its working memory.
pub(in crate::scheduler) const UNBOUNDED: &str = "Memory admission refused: this model's working memory cannot be bounded from its GGUF metadata (zero, missing, malformed or unsupported shape: recurrent/hybrid, mixture-of-experts, per-layer heads); no native allocation was attempted";

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
        // The model's metadata cannot bound a context: refuse explicitly
        // rather than reserve a guess (fail closed, before any allocation).
        Some(usize::MAX) => return Err(UNBOUNDED.to_string()),
        Some(estimate) => estimate,
        None => limits.max_memory_per_call(),
    };
    limits.try_acquire_working(bytes).map(Some).map_err(|e| {
        format!("{e} (request working memory; GG_CORE_MAX_TOTAL_MEMORY bounds it, GG_CORE_MAX_MEMORY_PER_CALL caps it when set)")
    })
}

/// Execute admitted work on the blocking pool. The reservation is owned by the
/// native task, NOT by its awaiter: aborting a worker future drops its
/// JoinHandle but cannot stop running spawn_blocking work. The reservation
/// must therefore remain charged until that closure completes or unwinds.
///
/// This shared boundary is used by normal, typed and streaming workers. It
/// also prevents the three paths from silently diverging again (#219).
pub(in crate::scheduler) async fn run_blocking_admitted<T, F>(
    guard: Option<ResourceGuard>,
    run: F,
) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        run()
    })
    .await
}

/// Own a reactor for the whole native inference call. spawn_blocking tasks can
/// outlive their Tokio caller on abort/shutdown_timeout (#219). Reusing the
/// caller's Handle would leave Tokio-timer/IO-backed model futures suspended
/// forever once that runtime shuts down, retaining their admitted bytes.
/// Fresh, per-invocation runtime ownership avoids cross-request task leakage.
pub(in crate::scheduler) fn block_on_native<T>(
    future: impl std::future::Future<Output = Result<T, crate::engine::inference::InferenceError>>,
) -> Result<T, crate::engine::inference::InferenceError> {
    use crate::engine::inference::InferenceError;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            InferenceError::ExecutionFailed(format!("native inference runtime unavailable: {e}"))
        })?;
    rt.block_on(future)
}

#[cfg(test)]
#[path = "worker_admission_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "worker_shutdown_tests.rs"]
mod shutdown_tests;
