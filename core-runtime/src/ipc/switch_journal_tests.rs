//! Unit tests for the switch journal (bounded, read-only retries).

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

fn expire(journal: &SwitchJournal, id: &str) {
    let past = Instant::now()
        .checked_sub(COMPLETED_TTL + Duration::from_secs(1))
        .expect("host uptime exceeds the journal TTL");
    journal.lock().get_mut(id).expect("recorded").finished_at = Some(past);
}

#[test]
fn expired_operation_id_is_never_readmitted_as_a_new_switch() {
    let journal = SwitchJournal::default();
    let a = request('c', "d");
    assert!(matches!(journal.reserve(a.clone()), Admission::New));
    journal.complete(
        &a.operation_id,
        SwitchOutcome::Unknown { reason: "x".into() },
    );
    expire(&journal, &a.operation_id);
    assert_eq!(
        journal.status(&a.operation_id),
        None,
        "outcome no longer retained"
    );
    // Same payload or a different one: the expired ID cannot mutate again.
    assert!(matches!(journal.reserve(a.clone()), Admission::Expired));
    assert!(matches!(
        journal.reserve(request('c', "e")),
        Admission::Expired
    ));
    assert!(matches!(journal.reserve(request('d', "d")), Admission::New));
}

#[test]
fn expired_tombstones_are_bounded() {
    let journal = SwitchJournal::default();
    for n in 0..MAX_EXPIRED_IDS + 1 {
        let mut r = request('f', "b");
        r.operation_id = format!("{n:064x}");
        assert!(matches!(journal.reserve(r.clone()), Admission::New));
        journal.complete(
            &r.operation_id,
            SwitchOutcome::Unknown { reason: "x".into() },
        );
        expire(&journal, &r.operation_id);
    }
    let mut last = request('f', "b");
    last.operation_id = format!("{:064x}", MAX_EXPIRED_IDS);
    assert!(matches!(journal.reserve(last), Admission::Expired));
    assert!(journal.expired.lock().unwrap().len() <= MAX_EXPIRED_IDS);
}
