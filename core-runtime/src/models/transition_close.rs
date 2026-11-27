//! Closing A to new admissions: the two #153 release policies (S9).
//!
//! The approved baseline unloads A at once and waits for its release; a
//! wait that expires is the documented `ReleasePending` outage. Option 2
//! (drain before unload) first fences new admissions to A, waits for the
//! leases already out to end, and only then unloads through the same
//! canonical transaction. If the leases do not end in time the fence is
//! lifted and the switch is refused with A registered and serving under its
//! original handle: B is never loaded, no weights are duplicated, and A's
//! release is never handed to the supervisor. The policy is crate-internal
//! and defaults to the baseline until the owner decides #153.

use super::{ColdSwitchRequest, LifecycleError, ModelLifecycle, SwitchOutcome, SwitchRefusal};
use crate::engine::AdmissionFence;

/// How the switch closes A while in-flight requests may still hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ReleasePolicy {
    /// Approved baseline (#153): unload A, then wait for its release. An
    /// expired wait leaves no model serving (`SwitchOutcome::ReleasePending`).
    #[default]
    UnloadThenWait,
    /// Option 2 (owner decision pending): fence new admissions, drain the
    /// leases out within the wait, then unload. An expired wait refuses the
    /// switch (`SwitchRefusal::DrainTimedOut`) and A serves on unchanged.
    DrainBeforeUnload,
}

/// Close A under the request's policy. `Err` is the switch's final outcome.
pub(super) async fn close_a(
    lifecycle: &ModelLifecycle,
    req: &ColdSwitchRequest,
) -> Result<(), SwitchOutcome> {
    let fence = match req.policy {
        ReleasePolicy::UnloadThenWait => None,
        ReleasePolicy::DrainBeforeUnload => Some(drain(lifecycle, req).await?),
    };
    // Disruption starts here: A leaves the registry; in-flight and streaming
    // requests continue to completion. The fence (if any) stays raised until
    // the unload has committed, so no lease is taken between drain and unload;
    // requests that waited on it then observe `ModelNotLoaded`.
    let unloaded = lifecycle
        .unload_expected(&req.from_id, req.expected_handle)
        .await;
    drop(fence);
    match unloaded {
        Ok(_) => Ok(()),
        Err(LifecycleError::StaleHandle(_)) => {
            Err(SwitchOutcome::Refused(SwitchRefusal::StaleHandle))
        }
        Err(_) => Err(SwitchOutcome::Refused(SwitchRefusal::NotLoaded)),
    }
}

/// Fence A and wait for its leases to end. On expiry the fence is dropped
/// here, which reopens A before the refusal is reported.
async fn drain(
    lifecycle: &ModelLifecycle,
    req: &ColdSwitchRequest,
) -> Result<AdmissionFence, SwitchOutcome> {
    let (fence, outstanding) = match lifecycle
        .fence_admissions(&req.from_id, req.expected_handle)
        .await
    {
        Ok(fenced) => fenced,
        Err(LifecycleError::StaleHandle(_)) => {
            return Err(SwitchOutcome::Refused(SwitchRefusal::StaleHandle))
        }
        Err(_) => return Err(SwitchOutcome::Refused(SwitchRefusal::NotLoaded)),
    };
    let pending = match outstanding {
        0 => 0,
        _ => lifecycle.wait_drained(&req.from_id, req.release_wait).await,
    };
    if pending > 0 {
        drop(fence);
        return Err(SwitchOutcome::Refused(SwitchRefusal::DrainTimedOut {
            pending,
        }));
    }
    Ok(fence)
}
