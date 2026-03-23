//! TDD-Light tests for process sandboxing.

use gg_core::sandbox::{create_sandbox, SandboxConfig};

#[test]
fn sandbox_default_config_is_reasonable() {
    let config = SandboxConfig::default();

    assert!(config.max_memory_bytes >= 1024 * 1024 * 1024); // At least 1GB
    assert!(config.max_cpu_time_ms >= 1000); // At least 1 second
    assert!(config.enabled);
}

#[test]
fn sandbox_can_be_created() {
    let config = SandboxConfig::default();
    let sandbox = create_sandbox(config);

    // Sandbox should be created successfully
    assert!(!sandbox.is_active()); // Not yet applied
}

#[test]
fn sandbox_disabled_reports_success() {
    let config = SandboxConfig {
        enabled: false,
        ..Default::default()
    };
    let sandbox = create_sandbox(config);

    let result = sandbox.apply();
    assert!(result.success);
}

#[test]
fn sandbox_usage_returns_none_when_inactive() {
    let config = SandboxConfig::default();
    let sandbox = create_sandbox(config);

    let usage = sandbox.get_usage();
    assert!(usage.is_none());
}

#[test]
fn sandbox_config_custom_limits() {
    let config = SandboxConfig {
        max_memory_bytes: 512 * 1024 * 1024, // 512MB
        max_cpu_time_ms: 5000,                // 5 seconds
        enabled: true,
    };

    assert_eq!(config.max_memory_bytes, 512 * 1024 * 1024);
    assert_eq!(config.max_cpu_time_ms, 5000);
}
