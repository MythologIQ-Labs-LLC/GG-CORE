//! Daemon authentication boot policy (RC acceptance: fail secure on a
//! missing credential).
//!
//! The IPC handshake accepts exactly the configured `CORE_AUTH_TOKEN`. With
//! an empty token, an empty-token handshake succeeds, which turns every
//! local process that can reach the socket into an authenticated client.
//! `serve` therefore refuses to start without a token unless the operator
//! explicitly opts into the development-only no-auth mode, which is logged
//! as a critical security event and printed on every start.

use gg_core::telemetry::{log_security_event, SecurityEvent};

/// Explicit opt-in for running the daemon without an auth token.
pub const INSECURE_NO_AUTH_ENV: &str = "GG_CORE_ALLOW_INSECURE_NO_AUTH";

/// Minimum token length below which startup warns (not refuses).
pub const RECOMMENDED_TOKEN_BYTES: usize = 16;

/// Resolved daemon authentication policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthPolicy {
    /// A non-empty shared token is required on every session handshake.
    Token,
    /// Development-only: empty token accepted (explicit opt-in).
    InsecureNoAuth,
}

/// Decide the auth policy from the configured token and the opt-in env var.
/// `Err` must abort startup.
pub fn auth_policy(token: &str) -> Result<AuthPolicy, String> {
    if !token.is_empty() {
        return Ok(AuthPolicy::Token);
    }
    match std::env::var(INSECURE_NO_AUTH_ENV) {
        Ok(v) if v.trim() == "1" => Ok(AuthPolicy::InsecureNoAuth),
        _ => Err(format!(
            "CORE_AUTH_TOKEN is empty. The daemon refuses to serve without a shared \
             token because an empty token authenticates every local client. Set \
             CORE_AUTH_TOKEN to a secret of at least {RECOMMENDED_TOKEN_BYTES} bytes, \
             or set {INSECURE_NO_AUTH_ENV}=1 for a development-only, local-trust \
             deployment (never in production)."
        )),
    }
}

/// Human-readable status line for the resolved policy; also emits the
/// security log entry for the insecure mode.
pub fn describe(policy: AuthPolicy, token: &str) -> String {
    match policy {
        AuthPolicy::Token if token.len() < RECOMMENDED_TOKEN_BYTES => format!(
            "Auth: shared token (WARNING: {} bytes; {RECOMMENDED_TOKEN_BYTES}+ recommended)",
            token.len()
        ),
        AuthPolicy::Token => "Auth: shared token required on every session".to_string(),
        AuthPolicy::InsecureNoAuth => {
            log_security_event(
                SecurityEvent::InsecureConfiguration,
                "daemon started WITHOUT authentication (development-only opt-in)",
                &[("env", INSECURE_NO_AUTH_ENV)],
            );
            format!(
                "Auth: *** DISABLED *** ({INSECURE_NO_AUTH_ENV}=1): every local client that \
                 can reach the socket is trusted. Development only."
            )
        }
    }
}

#[cfg(test)]
#[path = "auth_boot_tests.rs"]
mod tests;
