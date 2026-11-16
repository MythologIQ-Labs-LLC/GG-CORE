//! Process-isolated agreement tests for the strict resource parser (#194).
//!
//! Tests the binary, not a duplicated parser. Each child clears GG_CORE
//! configuration overrides inherited from the test runner. No daemon binds.

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const KEYS: &[&str] = &[
    "GG_CORE_MAX_CONTEXT",
    "GG_CORE_MAX_QUEUE_DEPTH",
    "GG_CORE_MAX_CONTEXT_TOKENS",
    "GG_CORE_MAX_MEMORY_PER_CALL",
    "GG_CORE_MAX_TOTAL_MEMORY",
    "GG_CORE_MAX_CONCURRENT",
    "GG_CORE_BATCH_MAX_REQUESTS",
    "GG_CORE_BATCH_MAX_TOKENS",
    "GG_CORE_SHUTDOWN_TIMEOUT",
    "GG_CORE_SESSION_TIMEOUT",
    "GG_CORE_N_CTX",
    "GG_CORE_N_THREADS",
    "GG_CORE_IPC_FRAME_LIMIT",
    "GG_CORE_MAX_CONNECTIONS",
    "GG_CORE_STREAM_STALL_MS",
];

fn config(sub: &str, override_value: Option<(&str, &str)>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gg-core-cli"));
    command.arg("config").arg(sub);
    for key in KEYS {
        command.env_remove(key);
    }
    if let Some((key, value)) = override_value {
        command.env(key, value);
    }
    command.output().expect("run the packaged CLI command")
}

#[test]
fn defaults_remain_valid_in_an_isolated_process() {
    for cmd in ["show", "validate"] {
        let output = config(cmd, None);
        assert!(output.status.success(), "{cmd}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn invalid_explicit_overrides_fail_show_and_validate_equally() {
    for (var, raw) in [
        ("GG_CORE_MAX_CONTEXT", "1000001"),
        ("GG_CORE_MAX_QUEUE_DEPTH", "garbage"),
        ("GG_CORE_MAX_CONTEXT_TOKENS", "0"),
        ("GG_CORE_MAX_CONCURRENT", "0"),
        ("GG_CORE_BATCH_MAX_REQUESTS", "0"),
        ("GG_CORE_BATCH_MAX_TOKENS", "-1"),
        ("GG_CORE_SHUTDOWN_TIMEOUT", "0"),
        ("GG_CORE_SESSION_TIMEOUT", "0"),
        ("GG_CORE_N_CTX", "127"),
        ("GG_CORE_N_THREADS", "abc"),
        ("GG_CORE_N_THREADS", "2147483648"),
        ("GG_CORE_IPC_FRAME_LIMIT", "4095"),
        ("GG_CORE_MAX_CONNECTIONS", "0"),
        ("GG_CORE_STREAM_STALL_MS", "99"),
    ] {
        for sub in ["show", "validate"] {
            let output = config(sub, Some((var, raw)));
            assert!(!output.status.success(), "{sub} accepted invalid {var}={raw}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(var), "{sub} lost field identity: {stderr}");
            assert!(output.stdout.is_empty(), "{sub} printed misleading config");
        }
    }
}

#[test]
fn documented_gguf_auto_thread_sentinel_is_valid() {
    let output = config("validate", Some(("GG_CORE_N_THREADS", "0")));
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn old_memory_ceiling_guard_remains_strict() {
    for var in ["GG_CORE_MAX_MEMORY_PER_CALL", "GG_CORE_MAX_TOTAL_MEMORY"] {
        let output = config("validate", Some((var, "0")));
        assert!(!output.status.success(), "{var} was silently accepted");
        assert!(String::from_utf8_lossy(&output.stderr).contains(var));
    }
}

#[test]
fn invalid_config_prevents_daemon_from_binding() {
    // Subprocess: a regression must time out and fail, never leave a server
    // running or pass because some earlier unrelated startup check failed.
    let mut command = Command::new(env!("CARGO_BIN_EXE_gg-core-cli"));
    command.arg("serve");
    for key in KEYS {
        command.env_remove(key);
    }
    command.env_remove("GG_CORE_SANDBOX");
    command.env_remove("GG_CORE_PRELOAD_MODELS");
    command.env_remove("CORE_AUTH_TOKEN");
    command.env("GG_CORE_MAX_CONCURRENT", "0");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().expect("spawn daemon refusal probe");
    let deadline = Instant::now() + Duration::from_secs(15);
    let timed_out = loop {
        if child.try_wait().expect("poll daemon process").is_some() {
            break false;
        }
        if Instant::now() >= deadline {
            child.kill().expect("terminate unexpectedly running daemon");
            break true;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let output = child.wait_with_output().expect("collect daemon refusal");
    assert!(!timed_out, "invalid configuration did not stop daemon startup");
    assert!(!output.status.success(), "daemon accepted invalid concurrency");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("serve: invalid configuration:") && stderr.contains("GG_CORE_MAX_CONCURRENT"),
        "wrong startup refusal: {stderr}"
    );
}
