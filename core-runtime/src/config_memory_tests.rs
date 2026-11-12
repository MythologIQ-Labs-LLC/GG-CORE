//! Table tests for the strict memory-limit resolver (#182). No env access.

use super::*;
use std::ffi::OsString;

const GIB: usize = 1 << 30;
const MIB: usize = 1 << 20;
const HOST: usize = 8 * GIB;

fn os(s: &str) -> OsString {
    OsString::from(s)
}

fn run(total: Option<&str>, per_call: Option<&str>) -> Result<MemoryLimits, MemoryConfigError> {
    let t = total.map(os);
    let p = per_call.map(os);
    resolve(t.as_deref(), p.as_deref(), HOST)
}

fn limits(total: usize, fallback: usize, ceiling: Option<usize>) -> MemoryLimits {
    MemoryLimits {
        total,
        per_call_fallback: fallback,
        per_call_ceiling: ceiling,
    }
}

#[test]
fn absent_absent_uses_host_default() {
    assert_eq!(run(None, None), Ok(limits(HOST, GIB, None)));
}

#[test]
fn explicit_total_below_fallback_is_exact_issue_182() {
    let got = run(Some("268435456"), None).unwrap();
    assert_eq!(got, limits(268_435_456, 268_435_456, None));
}

#[test]
fn explicit_total_above_fallback_keeps_one_gib_fallback() {
    assert_eq!(
        run(Some("2147483648"), None),
        Ok(limits(2 * GIB, GIB, None))
    );
}

#[test]
fn both_consistent_are_exact() {
    assert_eq!(
        run(Some("2147483648"), Some("134217728")),
        Ok(limits(2 * GIB, 128 * MIB, Some(128 * MIB)))
    );
}

#[test]
fn per_call_equal_to_total_is_admitted() {
    assert_eq!(
        run(Some("2147483648"), Some("2147483648")),
        Ok(limits(2 * GIB, 2 * GIB, Some(2 * GIB)))
    );
}

#[test]
fn per_call_above_explicit_total_is_refused() {
    let err = run(Some("268435456"), Some("268435457")).unwrap_err();
    assert_eq!(
        err,
        MemoryConfigError::PerCallExceedsTotal {
            per_call: 268_435_457,
            total: 268_435_456,
            total_explicit: true
        }
    );
    assert!(err.to_string().contains(TOTAL_VAR), "{err}");
}

#[test]
fn per_call_only_within_host_default_is_ceiling() {
    assert_eq!(
        run(None, Some("536870912")),
        Ok(limits(HOST, 512 * MIB, Some(512 * MIB)))
    );
}

#[test]
fn per_call_only_above_host_default_is_refused() {
    let raw = (HOST + 1).to_string();
    let err = run(None, Some(&raw)).unwrap_err();
    assert!(matches!(
        err,
        MemoryConfigError::PerCallExceedsTotal {
            total_explicit: false,
            ..
        }
    ));
    assert!(
        err.to_string().contains("set GG_CORE_MAX_TOTAL_MEMORY"),
        "{err}"
    );
}

#[test]
fn minimum_is_inclusive() {
    let min = MIN_EXPLICIT_MEMORY.to_string();
    assert_eq!(
        run(Some(&min), None),
        Ok(limits(MIN_EXPLICIT_MEMORY, MIN_EXPLICIT_MEMORY, None))
    );
    let below = (MIN_EXPLICIT_MEMORY - 1).to_string();
    assert!(matches!(
        run(Some(&below), None),
        Err(MemoryConfigError::BelowMinimum { .. })
    ));
}

#[test]
fn zero_is_refused_never_unlimited() {
    for (t, p) in [(Some("0"), None), (None, Some("0"))] {
        assert!(
            matches!(
                run(t, p),
                Err(MemoryConfigError::BelowMinimum { value: 0, .. })
            ),
            "{t:?} {p:?}"
        );
    }
}

#[test]
fn malformed_values_are_refused_for_both_variables() {
    let overflow = format!("{}0", usize::MAX);
    let bad = [
        "",
        "abc",
        "+5242880",
        "-1",
        " 5242880",
        "5242880 ",
        "1e9",
        "1GiB",
        "5_242_880",
        &overflow,
    ];
    for raw in bad {
        let total = run(Some(raw), None).unwrap_err();
        assert!(
            matches!(total, MemoryConfigError::Invalid { var: TOTAL_VAR, .. }),
            "total {raw:?}: {total:?}"
        );
        let per_call = run(None, Some(raw)).unwrap_err();
        assert!(
            matches!(
                per_call,
                MemoryConfigError::Invalid {
                    var: PER_CALL_VAR,
                    ..
                }
            ),
            "per-call {raw:?}: {per_call:?}"
        );
    }
}

#[test]
fn invalid_total_is_refused_even_with_valid_per_call() {
    assert!(run(Some("abc"), Some("134217728")).is_err());
}

#[test]
fn usize_max_is_accepted_exactly() {
    let raw = usize::MAX.to_string();
    assert_eq!(run(Some(&raw), None), Ok(limits(usize::MAX, GIB, None)));
}

#[cfg(unix)]
#[test]
fn non_utf8_is_refused() {
    use std::os::unix::ffi::OsStrExt;
    let raw = OsStr::from_bytes(&[0x35, 0xff]);
    let err = resolve(Some(raw), None, HOST).unwrap_err();
    assert!(matches!(err, MemoryConfigError::Invalid { .. }));
}

#[cfg(windows)]
#[test]
fn non_utf8_is_refused() {
    use std::os::windows::ffi::OsStringExt;
    let raw = OsString::from_wide(&[0x0035, 0xD800]);
    let err = resolve(Some(&raw), None, HOST).unwrap_err();
    assert!(matches!(err, MemoryConfigError::Invalid { .. }));
}

#[test]
fn resource_limits_mirror_resolved_values() {
    let rl = limits(268_435_456, 268_435_456, None).to_resource_limits(3);
    assert_eq!(rl.max_total_memory, 268_435_456);
    assert_eq!(rl.max_memory_per_call, 268_435_456);
    assert_eq!(rl.max_concurrent, 3);
}
