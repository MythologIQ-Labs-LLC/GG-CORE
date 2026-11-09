//! Resident accounting, checked reservation and concurrency regressions.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use super::try_reserve;
use crate::memory::{ResourceLimits, ResourceLimitsConfig};

fn limits(total: usize) -> ResourceLimits {
    limits_with(100, total, 4)
}

fn limits_with(per_call: usize, total: usize, concurrent: usize) -> ResourceLimits {
    ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: per_call,
        max_total_memory: total,
        max_concurrent: concurrent,
    })
}

#[test]
fn resident_bytes_count_toward_the_total() {
    let l = limits(1_000);
    let _weights = l.reserve_resident(900).unwrap();
    assert_eq!(l.current_memory(), 900);
    assert!(
        l.try_acquire(100).is_ok(),
        "900 resident + 100 working fits"
    );
    assert!(
        l.try_acquire(101).is_err(),
        "per-call cap still applies to working bytes"
    );
}

#[test]
fn working_reservation_fails_when_resident_fills_the_total() {
    let l = limits(1_000);
    let _weights = l.reserve_resident(950).unwrap();
    assert!(l.try_acquire(100).is_err(), "950 + 100 exceeds the total");
    assert_eq!(l.current_memory(), 950, "refusal must not leak");
}

#[test]
fn resident_reservation_larger_than_per_call_is_allowed() {
    let l = limits(10_000);
    assert!(
        l.reserve_resident(5_000).is_ok(),
        "weights are not per-call memory"
    );
}

#[test]
fn resident_reservation_beyond_total_is_refused_and_not_leaked() {
    let l = limits(1_000);
    assert!(l.reserve_resident(1_001).is_err());
    assert_eq!(l.current_memory(), 0, "failed reservation must not leak");
}

#[test]
fn reservation_exactly_at_the_total_is_admitted() {
    let l = limits(1_000);
    let _a = l.reserve_resident(1_000).expect("== total fits");
    assert!(l.reserve_resident(1).is_err());
    assert!(l.reserve_resident(0).is_ok(), "zero bytes always fits");
}

#[test]
fn dropping_the_guard_releases_resident_bytes() {
    let l = limits(1_000);
    let guard = l.reserve_resident(800).unwrap();
    assert!(l.reserve_resident(300).is_err());
    drop(guard);
    assert_eq!(l.current_memory(), 0);
    assert!(l.reserve_resident(300).is_ok());
}

#[test]
fn checked_reserve_never_wraps_usize() {
    let counter = AtomicUsize::new(usize::MAX - 10);
    assert_eq!(
        try_reserve(&counter, 11, usize::MAX),
        Err(usize::MAX),
        "overflow is refused, never wrapped"
    );
    assert_eq!(counter.load(Ordering::SeqCst), usize::MAX - 10);
    assert!(try_reserve(&counter, 10, usize::MAX).is_ok());
}

#[test]
fn huge_resident_request_is_refused_without_wrapping_the_counter() {
    let l = limits(usize::MAX);
    let _a = l.reserve_resident(usize::MAX - 5).unwrap();
    assert!(l.reserve_resident(usize::MAX).is_err());
    assert_eq!(l.current_memory(), usize::MAX - 5);
}

#[test]
fn huge_working_request_with_unbounded_per_call_is_refused_without_wrapping() {
    // Operator sets GG_CORE_MAX_MEMORY_PER_CALL very high: per-call check
    // passes, so only the checked total protects the counter.
    let l = limits_with(usize::MAX, usize::MAX, 4);
    let _w = l.reserve_resident(usize::MAX - 5).unwrap();
    assert!(l.try_acquire(usize::MAX).is_err());
    assert_eq!(l.current_memory(), usize::MAX - 5);
    assert_eq!(l.current_concurrent(), 0);
}

#[test]
fn resize_grows_within_budget_and_refuses_beyond_it_unchanged() {
    let l = limits(1_000);
    let mut g = l.reserve_resident(400).unwrap();
    g.resize(700).expect("grow within budget");
    assert_eq!((g.bytes(), l.current_memory()), (700, 700));
    let _other = l.reserve_resident(200).unwrap();
    assert!(
        g.resize(900).is_err(),
        "700 -> 900 with 200 elsewhere exceeds"
    );
    assert_eq!((g.bytes(), l.current_memory()), (700, 900), "unchanged");
    g.resize(100).expect("shrink always succeeds");
    assert_eq!(l.current_memory(), 300);
    drop(g);
    assert_eq!(l.current_memory(), 200);
}

#[test]
fn concurrent_reservations_never_exceed_the_total() {
    // 16 threads race for 600-byte reservations under a 1000-byte budget:
    // exactly one may win at a time, and the counter never exceeds 1000.
    let l = limits(1_000);
    let peak = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(16));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let (l, peak, barrier) = (l.clone(), Arc::clone(&peak), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                let g = l.reserve_resident(600).ok();
                peak.fetch_max(l.current_memory(), Ordering::SeqCst);
                g.is_some()
            })
        })
        .collect();
    let winners = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|w| *w)
        .count();
    assert!(winners >= 1, "at least one reservation succeeds");
    assert!(peak.load(Ordering::SeqCst) <= 1_000, "total never exceeded");
    assert_eq!(l.current_memory(), 0, "all guards released");
}

#[test]
fn concurrent_working_reservations_respect_total_and_slots() {
    let l = limits_with(300, 1_000, 3);
    let barrier = Arc::new(Barrier::new(12));
    let handles: Vec<_> = (0..12)
        .map(|_| {
            let (l, barrier) = (l.clone(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                let g = l.try_acquire(300);
                let seen = (l.current_memory(), l.current_concurrent());
                std::thread::sleep(std::time::Duration::from_millis(20));
                (g.is_ok(), seen)
            })
        })
        .collect();
    for h in handles {
        let (_, (mem, slots)) = h.join().unwrap();
        assert!(mem <= 1_000 && slots <= 3, "mem={mem} slots={slots}");
    }
    assert_eq!((l.current_memory(), l.current_concurrent()), (0, 0));
}
