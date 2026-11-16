//! Pure resource parsing tests. No process environment mutation.

use std::ffi::OsStr;

use super::{parse_decimal, ResourceConfigError};

const KEY: &str = "GG_CORE_MAX_CONTEXT";
const RANGE: &str = "decimal integer in 1..=1000000";

fn parse(raw: Option<&OsStr>) -> Result<usize, ResourceConfigError> {
    parse_decimal(KEY, raw, 4096usize, 1, 1_000_000, RANGE)
}

#[test]
fn missing_uses_the_existing_default() {
    assert_eq!(parse(None), Ok(4096));
}

#[test]
fn valid_explicit_values_are_not_modified() {
    for (raw, value) in [("1", 1), ("4096", 4096), ("1000000", 1_000_000)] {
        assert_eq!(parse(Some(OsStr::new(raw))), Ok(value));
    }
}

#[test]
fn bad_decimal_forms_fail_closed() {
    for raw in ["", " ", " 10", "10 ", "+10", "-10", "1_000", "0x10", "1.0", "abc"] {
        assert_eq!(
            parse(Some(OsStr::new(raw))),
            Err(ResourceConfigError::Invalid { var: KEY, expected: RANGE }),
            "bad value {raw:?}"
        );
    }
}

#[test]
fn explicit_out_of_range_never_clamps() {
    for raw in ["0", "1000001"] {
        assert_eq!(
            parse(Some(OsStr::new(raw))),
            Err(ResourceConfigError::OutOfRange { var: KEY, expected: RANGE })
        );
    }
}

#[test]
fn unsigned_width_overflow_is_refused() {
    let result = parse_decimal("GG_CORE_N_CTX", Some(OsStr::new("4294967296")),
                               2048u32, 128u32, u32::MAX, "decimal u32 >= 128");
    assert!(matches!(result, Err(ResourceConfigError::Invalid { .. })));
}

#[test]
fn allowed_zero_sentinel_stays_zero() {
    const THREADS: &str = "GG_CORE_N_THREADS";
    assert_eq!(
        parse_decimal(THREADS, Some(OsStr::new("0")), 0u32, 0, u32::MAX,
                      "decimal u32 (0 = auto)"),
        Ok(0)
    );
}

#[test]
fn invalid_utf8_is_not_treated_as_absent() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let raw = OsStr::from_bytes(&[0xff]);
        assert!(matches!(parse(Some(raw)), Err(ResourceConfigError::Invalid { .. })));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let raw = std::ffi::OsString::from_wide(&[0xd800]);
        assert!(matches!(parse(Some(&raw)), Err(ResourceConfigError::Invalid { .. })));
    }
}
