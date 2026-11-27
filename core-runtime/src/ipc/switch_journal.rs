//! Bounded, connection-independent status journal for cold model switches.
//!
//! A client-generated CSPRNG operation ID is the reconnect capability,
//! protected *in addition* by normal session authentication at the handler.
//! The server never lists IDs, accepts client-written status, or retries a
//! second model mutation for the same ID. This ledger is process-local and
//! deliberately does not survive daemon restart; missing state is UNKNOWN
//! to the caller, not a success or a new switch authorization. Expired
//! IDs remain blocked by tombstones. When the bounded history fills, new
//! switches are refused instead of evicting replay evidence. A restart
//! resets process-local status to UNKNOWN; clients must not infer that
//! repeating an old operation is safe.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::protocol_switch_types::{
    ModelSwitchPhase, ModelSwitchRequest, ModelSwitchStatusResponse,
};
use crate::models::transition::SwitchOutcome;

const MAX_RECORDS: usize = 64;
const COMPLETED_TTL: Duration = Duration::from_secs(600);
const MAX_DETAIL_CHARS: usize = 256;
const MAX_EXPIRED_IDS: usize = 1024;

struct Record {
    request: ModelSwitchRequest,
    status: ModelSwitchStatusResponse,
    finished_at: Option<Instant>,
}

#[derive(Default)]
pub(super) struct SwitchJournal {
    records: Mutex<HashMap<String, Record>>,
    expired: Mutex<VecDeque<String>>,
}

pub(super) enum Admission {
    New,
    Repeat(ModelSwitchStatusResponse),
    Conflict,
    Capacity,
    /// The ID finished more than the TTL ago; its outcome is gone and the
    /// ID can never start another switch.
    Expired,
}

impl SwitchJournal {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Record>> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn reserve(&self, request: ModelSwitchRequest) -> Admission {
        let mut records = self.lock();
        let mut expired = self
            .expired
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Make status lookup and replay semantics agree at the TTL boundary.
        // Active operations are never expired; expired IDs become tombstones.
        records.retain(|id, entry| {
            let live = entry
                .finished_at
                .is_none_or(|time| time.elapsed() < COMPLETED_TTL);
            if !live {
                // Never evict consumed IDs. When history is full,
                // previously unseen IDs are refused below.
                if expired.len() < MAX_EXPIRED_IDS {
                    expired.push_back(id.clone());
                }
            }
            live
        });
        if expired.contains(&request.operation_id) {
            return Admission::Expired;
        }
        if let Some(entry) = records.get(&request.operation_id) {
            return if entry.request == request {
                Admission::Repeat(entry.status.clone())
            } else {
                Admission::Conflict
            };
        }
        // Known reconnects remain read-only at saturation. Unknown IDs
        // cannot start another mutation once replay history is full.
        if expired.len() >= MAX_EXPIRED_IDS || records.len() >= MAX_RECORDS {
            return Admission::Capacity;
        }
        let status = ModelSwitchStatusResponse::new(
            request.operation_id.clone(),
            request.from_id.clone(),
            request.to_id.clone(),
        );
        records.insert(
            request.operation_id.clone(),
            Record {
                request,
                status: status.clone(),
                finished_at: None,
            },
        );
        Admission::New
    }

    pub(super) fn status(&self, operation_id: &str) -> Option<ModelSwitchStatusResponse> {
        let records = self.lock();
        let entry = records.get(operation_id)?;
        if entry
            .finished_at
            .is_some_and(|time| time.elapsed() >= COMPLETED_TTL)
        {
            return None;
        }
        Some(entry.status.clone())
    }

    pub(super) fn complete(&self, operation_id: &str, outcome: SwitchOutcome) {
        let mut records = self.lock();
        let Some(entry) = records.get_mut(operation_id) else {
            return; // an unregistered operation cannot publish a result
        };
        let (phase, to_handle, detail) = match outcome {
            SwitchOutcome::Switched { to_handle, .. } => (
                ModelSwitchPhase::LoadedUnverified,
                Some(to_handle),
                Some("registered; task-specific inference readiness is not yet verified".into()),
            ),
            SwitchOutcome::Refused(refusal) => (
                ModelSwitchPhase::Refused,
                None,
                Some(safe_detail(&format!("{refusal:?}"))),
            ),
            SwitchOutcome::ReleasePending { pending, .. } => (
                ModelSwitchPhase::ReleasePending,
                None,
                // A historical outcome, not live state: A was closed and B was
                // not loaded, so no model serves until an explicit load (#153).
                Some(format!(
                    "release wait expired with {pending} old model instance(s) still held;                      no model is serving (source unloaded, target not loaded);                      recover with an explicit model load after release"
                )),
            ),
            SwitchOutcome::RolledBack {
                cause,
                restored_handle,
            } => (
                ModelSwitchPhase::RolledBackUnverified,
                None, // B was not loaded; this is A's restored handle, not B's.
                Some(safe_detail(&format!(
                    "{cause}; source restored as handle {restored_handle} (not smoke-verified)"
                ))),
            ),
            SwitchOutcome::RollbackFailed {
                cause,
                rollback_error,
            } => (
                ModelSwitchPhase::RollbackFailed,
                None,
                Some(safe_detail(&format!("{cause}; rollback: {rollback_error}"))),
            ),
            SwitchOutcome::Unknown { reason } => {
                (ModelSwitchPhase::Unknown, None, Some(safe_detail(&reason)))
            }
        };
        entry.status.phase = phase;
        entry.status.to_handle = to_handle;
        entry.status.detail = detail;
        entry.status.ready = false;
        entry.finished_at = Some(Instant::now());
    }
}

fn safe_detail(s: &str) -> String {
    s.chars().take(MAX_DETAIL_CHARS).collect()
}

#[cfg(test)]
#[path = "switch_journal_tests.rs"]
mod tests;
