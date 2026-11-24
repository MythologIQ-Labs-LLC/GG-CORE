//! Offline operator deployment checks (GG-CORE #159).
//!
//! No network listener, model execution, download, or sandbox bypass.
//! A successful result verifies only live local IPC + advertised security
//! posture. It does not attest binary identity, inference or v1 release.
use std::time::Duration;

use crate::config;
use crate::ipc::protocol::{
    decode_message, encode_message, HealthCheckType, IpcMessage,
};
use crate::models::backend_capabilities::CAPABILITY_SCHEMA_VERSION;

use super::{get_socket_path, CliIpcClient};

const IPC_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
pub struct VerifyOptions {
    pub socket: String,
    pub all: bool,
}

/// Refuse unknown, duplicate or incomplete options. No host environment
/// mutation, and no attempt to interpret an IPC path as a network URL.
pub fn parse_options(args: &[String]) -> Result<VerifyOptions, &'static str> {
    let mut socket = None;
    let mut all = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" if !all => all = true,
            "--socket" if socket.is_none() => {
                i += 1;
                let path = args.get(i).ok_or("missing --socket path")?;
                if path.is_empty() || path.starts_with("--") {
                    return Err("invalid --socket path");
                }
                socket = Some(path.clone());
            }
            _ => return Err("unknown or repeated verify argument"),
        }
        i += 1;
    }
    Ok(VerifyOptions {
        socket: socket.unwrap_or_else(get_socket_path),
        all,
    })
}

/// Reject a spoofed/malformed capability response or a daemon that reports
/// insecure operating mode. An ack alone cannot prove protected IPC.
fn validate_capabilities(bytes: &[u8]) -> Result<(), &'static str> {
    let response = decode_message(bytes).map_err(|_| "invalid capabilities response")?;
    let IpcMessage::CapabilitiesResponse(response) = response else {
        return Err("unexpected capabilities response");
    };
    let snapshot = &response.snapshot;
    if snapshot.get("schema_version").and_then(|v| v.as_u64())
        != Some(u64::from(CAPABILITY_SCHEMA_VERSION))
    {
        return Err("unsupported capability schema");
    }
    let operator = snapshot.get("operator").ok_or("missing operator constraints")?;
    if operator.get("auth_required").and_then(|v| v.as_bool()) != Some(true) {
        return Err("daemon does not require authenticated IPC");
    }
    if operator.get("offline_only").and_then(|v| v.as_bool()) != Some(true)
        || operator.get("network_listener").and_then(|v| v.as_bool()) != Some(false)
    {
        return Err("daemon does not advertise offline-only execution");
    }
    Ok(())
}

/// Verify a running daemon using a fresh local authenticated IPC session.
/// The stricter --all does not infer model quality from model readiness.
async fn check_live(options: &VerifyOptions) -> Result<(), &'static str> {
    if std::env::var("CORE_AUTH_TOKEN").is_err()
        || std::env::var("CORE_AUTH_TOKEN").is_ok_and(|s| s.is_empty())
    {
        return Err("CORE_AUTH_TOKEN is required");
    }
    let client = CliIpcClient::new(options.socket.clone()).with_timeout(IPC_TIMEOUT);
    if client.check_health(HealthCheckType::Liveness).await != Ok(true) {
        return Err("daemon IPC liveness check failed");
    }
    let request = encode_message(&IpcMessage::CapabilitiesRequest)
        .map_err(|_| "capability encoding failed")?;
    let response = client.send_receive_authenticated(&request).await
        .map_err(|_| "authenticated capability request refused")?;
    validate_capabilities(&response)?;

    if options.all {
        if client.check_health(HealthCheckType::Readiness).await != Ok(true) {
            return Err("daemon is not ready");
        }
        let models = client.get_models().await
            .map_err(|_| "model list IPC failed")?;
        if !models.models.iter().any(|model| model.state == "ready") {
            return Err("no ready model is reported");
        }
        client.get_metrics().await.map_err(|_| "metrics IPC failed")?;
    }
    Ok(())
}

/// CLI return codes: 0 checked claims passed; 1 runtime refusal; 2 usage.
pub async fn run_verify(args: &[String]) -> i32 {
    let options = match parse_options(args) {
        Ok(v) => v,
        Err(code) => {
            eprintln!("VERIFY USAGE ERROR: {code}");
            return 2;
        }
    };
    if config::try_load().is_err() {
        eprintln!("VERIFY FAIL: local client configuration is invalid");
        return 1;
    }
    match check_live(&options).await {
        Ok(()) => {
            if options.all {
                println!("VERIFY PASS: config, authenticated offline IPC, liveness, readiness, ready-model listing and metrics");
            } else {
                println!("VERIFY PASS: config, authenticated offline IPC and liveness");
            }
            println!("NOT QUALIFIED: binary attestation, real-model inference, security containment, or v1 release");
            0
        }
        Err(code) => {
            eprintln!("VERIFY FAIL: {code}");
            1
        }
    }
}

#[cfg(test)]
#[path = "verify_cmd_tests.rs"]
mod tests;
