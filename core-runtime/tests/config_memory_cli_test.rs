//! #182: process-isolated checks that explicit memory limits are never
//! relaxed by the shipped `gg-core-cli` binary (config show/validate, serve).
//!
//! Each child starts from a cleared environment so a developer's or CI's
//! `GG_CORE_*` values cannot leak in (audit O5).

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_gg-core-cli");
const KEEP: &[&str] = &["PATH", "SystemRoot", "SYSTEMROOT", "TEMP", "TMP", "HOME"];

fn command(args: &[&str], vars: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(args).env_clear();
    for key in KEEP {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd.envs(vars.iter().copied());
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Run to completion; a child still running after 60 s is killed and fails
/// the test (an invalid `serve` must never reach its listener).
fn run(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut child = command(args, vars).spawn().expect("spawn gg-core-cli");
    let start = Instant::now();
    while child.try_wait().expect("wait").is_none() {
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{args:?} {vars:?} did not exit: it kept running");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().expect("output")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const T: &str = "GG_CORE_MAX_TOTAL_MEMORY";
const P: &str = "GG_CORE_MAX_MEMORY_PER_CALL";

fn invalid_settings() -> Vec<Vec<(&'static str, &'static str)>> {
    vec![
        vec![(T, "268435456"), (P, "536870912")],
        vec![(T, "0")],
        vec![(P, "0")],
        vec![(T, "abc")],
        vec![(P, "not-a-number")],
        vec![(T, "18446744073709551616")],
        vec![(P, "10")],
    ]
}

#[test]
fn show_reports_explicit_total_below_one_gib_exactly() {
    let out = run(&["config", "show"], &[(T, "268435456")]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        stdout
            .lines()
            .any(|l| l == "GG_CORE_MAX_TOTAL_MEMORY=268435456"),
        "{stdout}"
    );
    assert!(
        stdout.contains("fallback reservation 268435456"),
        "{stdout}"
    );
}

#[test]
fn validate_accepts_consistent_limits() {
    let out = run(
        &["config", "validate"],
        &[(T, "2147483648"), (P, "134217728")],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
}

#[test]
fn show_and_validate_refuse_invalid_limits() {
    for vars in invalid_settings() {
        for sub in ["show", "validate"] {
            let out = run(&["config", sub], &vars);
            let stderr = text(&out.stderr);
            assert_eq!(out.status.code(), Some(1), "{sub} {vars:?}: {stderr}");
            assert!(stderr.contains("ERROR:"), "{sub} {vars:?}: {stderr}");
            assert!(
                !text(&out.stdout).contains("GG_CORE_MAX_TOTAL_MEMORY="),
                "{sub} {vars:?}"
            );
        }
    }
}

#[test]
fn serve_refuses_invalid_limits_before_preload_sandbox_or_listen() {
    for vars in invalid_settings() {
        let out = run(&["serve"], &vars);
        let stderr = text(&out.stderr);
        assert!(!out.status.success(), "{vars:?}: {stderr}");
        assert!(
            stderr.contains("serve: invalid configuration"),
            "{vars:?}: {stderr}"
        );
        for later in ["Sandbox", "Startup preload", "Auth", "CORE_AUTH_TOKEN"] {
            assert!(
                !stderr.contains(later),
                "{vars:?} reached {later}: {stderr}"
            );
        }
    }
}
