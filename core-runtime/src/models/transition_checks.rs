//! Pre-disruption checks of a cold switch: preconditions and preflight.
//! Everything here is refused with A still serving and nothing changed.

use std::sync::Arc;

use tokio::task::spawn_blocking;

use super::{
    ColdSwitchRequest, ModelLifecycle, ModelSource, Preparer, SwitchRefusal, MAX_RELEASE_WAIT,
};

pub(super) async fn preconditions(
    lifecycle: &ModelLifecycle,
    req: &ColdSwitchRequest,
) -> Result<ModelSource, SwitchRefusal> {
    if req.release_wait > MAX_RELEASE_WAIT {
        return Err(SwitchRefusal::WaitTooLong);
    }
    if req.to_id == req.from_id {
        return Err(SwitchRefusal::SameModel);
    }
    match lifecycle.get_handle(&req.from_id).await {
        None => return Err(SwitchRefusal::NotLoaded),
        Some(handle) if handle != req.expected_handle => return Err(SwitchRefusal::StaleHandle),
        Some(_) => {}
    }
    if lifecycle.is_loaded(&req.to_id).await {
        return Err(SwitchRefusal::TargetLoaded);
    }
    lifecycle
        .source(req.expected_handle)
        .ok_or(SwitchRefusal::UnknownSource)
}

/// Validate A's recorded path and B's target without reading model bytes,
/// then estimate (re-checked at load) whether B fits once A's bytes return.
pub(super) async fn preflight(
    lifecycle: &ModelLifecycle,
    prep: &Arc<dyn Preparer>,
    req: &ColdSwitchRequest,
    source: &ModelSource,
) -> Result<(), SwitchRefusal> {
    let (p, to, from) = (
        Arc::clone(prep),
        req.to_path.clone(),
        source.relative_path.clone(),
    );
    let facts = spawn_blocking(move || p.preflight(&from).and_then(|_| p.preflight(&to)))
        .await
        .map_err(|e| SwitchRefusal::Preflight(e.to_string()))?
        .map_err(|e| SwitchRefusal::Preflight(e.to_string()))?;
    if let Some(limits) = lifecycle.resource_limits() {
        // Everything charged now (other residents, draining releases, request
        // working memory) except A's resident bytes, which the switch returns.
        let a = lifecycle.charged_bytes(&req.from_id).await;
        let others = limits.current_memory().saturating_sub(a) as u64;
        let available = (limits.max_total_memory() as u64).saturating_sub(others);
        if facts.size_bytes > available {
            return Err(SwitchRefusal::OverBudget {
                needed: facts.size_bytes,
                available,
            });
        }
    }
    Ok(())
}
