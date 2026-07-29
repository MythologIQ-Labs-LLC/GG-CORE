//! Tests for `UnixSandbox` (B-16 split from `unix.rs`; relocated verbatim).

use super::*;

#[test]
fn test_cgroups_v2_detection() {
    // This test will pass whether or not cgroups v2 is available
    // It just verifies the detection doesn't panic
    let _available = UnixSandbox::cgroups_v2_available();
}

#[test]
fn test_sandbox_disabled_by_config() {
    let config = SandboxConfig {
        enabled: false,
        ..Default::default()
    };
    let sandbox = UnixSandbox::new(config);
    let result = sandbox.apply();

    assert!(result.success);
    assert!(result.error.unwrap().contains("disabled"));
}

#[test]
#[ignore = "installs a real seccomp filter into the test process which cannot be removed; run in isolation only"]
fn test_sandbox_enabled_returns_proper_error() {
    let config = SandboxConfig {
        enabled: true,
        ..Default::default()
    };
    let sandbox = UnixSandbox::new(config);
    let result = sandbox.apply();

    // If cgroups v2 is not available or we don't have permissions,
    // this should return an error (not silently succeed)
    if !result.success {
        assert!(
            result.error.as_ref().unwrap().contains("Failed")
                || result.error.as_ref().unwrap().contains("not available")
        );
    }
    // If it succeeds, that's also valid (we have permissions)
}
