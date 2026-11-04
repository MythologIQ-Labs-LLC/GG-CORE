//! Tests for daemon sandbox boot policy (B-41 follow-on; SECURITY.md gap 1).

use super::*;
use std::sync::Mutex;

// GG_CORE_SANDBOX is process-global; serialize env-touching tests
// (same pattern as the other env locks in this crate).
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn unset_env_uses_platform_default() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var(SANDBOX_ENV);
    let mode = sandbox_mode_from_env().unwrap();
    if cfg!(windows) {
        assert_eq!(mode, SandboxMode::Enforce);
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(mode, SandboxMode::Seccomp);
    } else {
        assert_eq!(mode, SandboxMode::Off);
    }
}

#[test]
fn explicit_modes_parse_case_insensitively() {
    let _guard = ENV_LOCK.lock().unwrap();
    for (v, expected) in [
        ("enforce", SandboxMode::Enforce),
        ("SECCOMP", SandboxMode::Seccomp),
        ("Off", SandboxMode::Off),
    ] {
        std::env::set_var(SANDBOX_ENV, v);
        assert_eq!(sandbox_mode_from_env().unwrap(), expected, "value {v}");
    }
    std::env::remove_var(SANDBOX_ENV);
}

#[test]
fn invalid_mode_is_rejected_loudly() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var(SANDBOX_ENV, "lenient");
    let err = sandbox_mode_from_env().unwrap_err();
    assert!(err.contains("lenient"));
    std::env::remove_var(SANDBOX_ENV);
}

#[test]
fn off_mode_reports_without_applying() {
    let msg = apply_daemon_sandbox(SandboxMode::Off, &RuntimeConfig::default()).unwrap();
    assert!(msg.contains("OFF"));
}
