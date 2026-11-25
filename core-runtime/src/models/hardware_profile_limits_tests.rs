//! Admission-budget evidence from the real shared resource counter (#50).
//! All tests are model-free. Snapshot estimates are never load authorization.

use super::*;
use crate::memory::ResourceLimitsConfig;

fn limits(total: usize) -> ResourceLimits {
    ResourceLimits::new(ResourceLimitsConfig {
        max_memory_per_call: total,
        max_total_memory: total,
        max_concurrent: 2,
    })
}

#[test]
fn absent_limits_report_unknown_memory_not_infinite_headroom() {
    let profile = HardwareProfile::detect_with_limits(None);
    assert_eq!(profile.unreserved_ram_bytes, None);
    assert_eq!(profile.unreserved_vram_bytes, None);
    assert_eq!(profile.cuda, TargetAvailability::Unknown);
    assert_eq!(profile.metal, TargetAvailability::Unknown);
}

#[test]
fn resident_reservation_is_counted_once_and_restored_only_on_drop() {
    let account = limits(4_096);
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(4_096)
    );
    let held = account.reserve_resident(1_024).expect("resident admitted");
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(3_072)
    );
    let working = account
        .try_acquire_working(512)
        .expect("working reservation admitted");
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(2_560)
    );
    drop(working);
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(3_072)
    );
    drop(held);
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(4_096)
    );
}

#[test]
fn exhausted_budget_is_zero_not_unknown_or_available() {
    let account = limits(2_048);
    let held = account
        .reserve_resident(2_048)
        .expect("full budget reservation");
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(0)
    );
    assert!(
        account.reserve_resident(1).is_err(),
        "a snapshot must never bypass canonical admission"
    );
    drop(held);
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&account)).unreserved_ram_bytes,
        Some(2_048)
    );
}

#[test]
fn shared_accounting_is_observed_through_a_clone_without_new_authority() {
    let account = limits(1_024);
    let held = account.reserve_resident(1_024).expect("exact budget");
    let clone = account.clone();
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&clone)).unreserved_ram_bytes,
        Some(0)
    );
    assert!(clone.try_acquire_working(1).is_err());
    drop(held);
    assert_eq!(
        HardwareProfile::detect_with_limits(Some(&clone)).unreserved_ram_bytes,
        Some(1_024)
    );
}
