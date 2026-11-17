//! Bounded, connection-independent status journal for cold model switches.
//!
//! A client-generated CSPRNG operation ID is the reconnect capability,
//! protected *in addition* by normal session authentication at the handler.
//! The server never lists IDs, accepts client-written status, or retries a
//! second model mutation for the same ID. This ledger is process-local and
//! deliberately does not survive daemon restart; missing state is UNKNOWN
//! to the caller, not a success or a new switch authorization.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::protocol_switch_types::{
    ModelSwitchPhase, ModelSwitchRequest, ModelSwitchStatusResponse,
};
use crate::models::transition::SwitchOutcome;

const MAX_RECORDS: usize = 64;
const COMPLETED_TTL: Duration = Duration::from_secs(600);
const MAX_DETAIL_CHARS: usize = 256;

struct Record {
    request: ModelSwitchRequest,
    status: ModelSwitchStatusResponse,
    finished_at: Option<Instant>,
}

#[derive(Default)]
pub(super) struct SwitchJournal {
    records: Mutex<HashMap<String, Record>>,
}

pub(super) enum Admission {
    New,
    Repeat(ModelSwitchStatusResponse),
    Conflict,
    Capacity,
}

impl SwitchJournal {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Record>> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn reserve(&self, request: ModelSwitchRequest) -> Admission {
        let mut records = self.lock();
        // Make status lookup and replay semantics agree at the TTL boundary.
        // Active operations are never expired.
        records.retain(|_, entry| {
            entry
                .finished_at
                .is_none_or(|time| time.elapsed() < COMPLETED_TTL)
        });
        if let Some(entry) = records.get(&request.operation_id) {
            return if entry.request == request {
                Admission::Repeat(entry.status.clone())
            } else {
                Admission::Conflict
            };
        }
        if records.len() >= MAX_RECORDS {
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
                Some(format!("{pending} old model instance(s) still hold memory")),
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
mod tests {
    use super::*;

    fn request(id: char, target: &str) -> ModelSwitchRequest {
        ModelSwitchRequest::new(
            id.to_string().repeat(64),
            "a".into(),
            1,
            format!("models/{target}.gguf"),
            target.into(),
            30000,
        )
    }

    #[test]
    fn identical_retry_is_read_only_but_conflicting_payload_refuses() {
        let journal = SwitchJournal::default();
        let a = request('a', "b");
        assert!(matches!(journal.reserve(a.clone()), Admission::New));
        assert!(matches!(journal.reserve(a.clone()), Admission::Repeat(_)));
        let changed = request('a', "different");
        assert!(matches!(journal.reserve(changed), Admission::Conflict));
        assert_eq!(
            journal.status(&a.operation_id).unwrap().phase,
            ModelSwitchPhase::Running
        );
    }

    #[test]
    fn finished_status_survives_retry_without_claiming_readiness() {
        let journal = SwitchJournal::default();
        let a = request('b', "c");
        assert!(matches!(journal.reserve(a.clone()), Admission::New));
        journal.complete(
            &a.operation_id,
            SwitchOutcome::Switched {
                from_handle: 1,
                to_handle: 2,
                release_ms: 12,
                load_ms: 25,
            },
        );
        let Admission::Repeat(status) = journal.reserve(a.clone()) else {
            panic!("same id must never create a second switch");
        };
        assert_eq!(status.phase, ModelSwitchPhase::LoadedUnverified);
        assert_eq!(status.to_handle, Some(2));
        assert!(!status.ready);
        assert_eq!(journal.status(&a.operation_id), Some(status));
    }

    #[test]
    fn records_are_bounded_without_dropping_a_running_operation() {
        let journal = SwitchJournal::default();
        for n in 0..MAX_RECORDS {
            let id = format!("{n:064x}");
            let mut r = request('f', "b");
            r.operation_id = id;
            assert!(matches!(journal.reserve(r), Admission::New));
        }
        assert!(matches!(
            journal.reserve(request('e', "b")),
            Admission::Capacity
        ));
        assert_eq!(
            journal.status(&format!("{:064x}", 0)).unwrap().phase,
            ModelSwitchPhase::Running
        );
    }
}
