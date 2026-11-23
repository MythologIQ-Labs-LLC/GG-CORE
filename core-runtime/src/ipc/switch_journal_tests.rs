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
fn tombstone_saturation_refuses_new_mutations_and_keeps_old_ids() {
    let journal = SwitchJournal::default();
    let running = request('e', "b");
    assert!(matches!(journal.reserve(running.clone()), Admission::New));
    for n in 0..MAX_EXPIRED_IDS {
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
    last.operation_id = format!("{:064x}", MAX_EXPIRED_IDS - 1);
    assert!(matches!(journal.reserve(last), Admission::Expired));
    assert_eq!(journal.expired.lock().unwrap().len(), MAX_EXPIRED_IDS);

    let mut oldest = request('f', "b");
    oldest.operation_id = format!("{:064x}", 0);
    assert!(matches!(journal.reserve(oldest.clone()), Admission::Expired));
    let mut new_id = request('f', "b");
    new_id.operation_id = format!("{MAX_EXPIRED_IDS:064x}");
    assert!(matches!(journal.reserve(new_id), Admission::Capacity));
    assert!(matches!(journal.reserve(oldest), Admission::Expired));
    assert!(matches!(journal.reserve(running.clone()), Admission::Repeat(_)));
    assert_eq!(
        journal.status(&running.operation_id).unwrap().phase,
        ModelSwitchPhase::Running
    );
}

#[test]
fn simultaneous_ttl_expiry_cannot_evade_saturation() {
    let journal = SwitchJournal::default();
    {
        let mut expired = journal.expired.lock().unwrap();
        for n in 0..MAX_EXPIRED_IDS - 1 {
            expired.push_back(format!("{n:064x}"));
        }
    }
    let mut first = request('a', "b");
    first.operation_id = format!("{:064x}", MAX_EXPIRED_IDS);
    let mut second = request('b', "b");
    second.operation_id = format!("{:064x}", MAX_EXPIRED_IDS + 1);
    for r in [&first, &second] {
        assert!(matches!(journal.reserve(r.clone()), Admission::New));
    }
    for r in [&first, &second] {
        journal.complete(
            &r.operation_id,
            SwitchOutcome::Unknown { reason: "expired".into() },
        );
        expire(&journal, &r.operation_id);
    }
    let mut unseen = request('c', "b");
    unseen.operation_id = format!("{:064x}", MAX_EXPIRED_IDS + 2);
    assert!(matches!(journal.reserve(unseen), Admission::Capacity));
    assert!(matches!(
        journal.reserve(first),
        Admission::Expired | Admission::Capacity
    ));
    assert!(matches!(
        journal.reserve(second),
        Admission::Expired | Admission::Capacity
    ));
    assert_eq!(journal.expired.lock().unwrap().len(), MAX_EXPIRED_IDS);
}
