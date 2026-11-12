//! Tests for daemon sandbox boot policy (B-41 follow-on; SECURITY.md gap 1).

use super::*;
use std::sync::Mutex;

// GG_CORE_SANDBOX is process-global; serialize env-touching tests
// (same pattern as the other env locks in this crate).
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn unset_env_uses_platform_default() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(SANDBOX_ENV, "lenient");
    let err = sandbox_mode_from_env().unwrap_err();
    assert!(err.contains("lenient"));
    std::env::remove_var(SANDBOX_ENV);
}

#[test]
fn daemon_config_never_sets_a_cumulative_cpu_budget() {
    // A non-zero budget becomes JOB_OBJECT_LIMIT_JOB_TIME on Windows and
    // terminates the daemon mid-inference; memory is the daemon's gate.
    let rc = RuntimeConfig::default();
    for mode in [SandboxMode::Enforce, SandboxMode::Seccomp] {
        let cfg = daemon_sandbox_config(mode, &rc);
        assert_eq!(cfg.max_cpu_time_ms, 0, "{mode:?}");
        assert_eq!(cfg.max_memory_bytes, rc.resource_limits.max_total_memory);
        assert_eq!(cfg.cgroup_enabled, mode == SandboxMode::Enforce);
        assert!(!cfg.gpu_enabled);
        assert!(cfg.enabled);
    }
}

#[test]
fn off_mode_reports_without_applying() {
    let msg = apply_daemon_sandbox(SandboxMode::Off, &RuntimeConfig::default()).unwrap();
    assert!(msg.contains("OFF"));
}

/// #182: the Job Object / cgroup cap is the explicit total, never raised to
/// the 1 GiB per-call fallback.
#[test]
fn sandbox_memory_cap_equals_explicit_total_below_fallback() {
    let total = std::ffi::OsString::from("268435456");
    let mem = gg_core::config::memory::resolve(Some(&total), None, 8 << 30).unwrap();
    let rc = RuntimeConfig {
        resource_limits: mem.to_resource_limits(2),
        ..Default::default()
    };
    for mode in [SandboxMode::Enforce, SandboxMode::Seccomp] {
        assert_eq!(
            daemon_sandbox_config(mode, &rc).max_memory_bytes,
            268_435_456
        );
    }
}
