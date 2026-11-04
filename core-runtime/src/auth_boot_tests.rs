//! Tests for the daemon authentication boot policy.

use super::*;
use std::sync::Mutex;

// The opt-in env var is process-global; serialize env-touching tests.
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn non_empty_token_requires_token_policy() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var(INSECURE_NO_AUTH_ENV);
    assert_eq!(
        auth_policy("s3cret-token-of-reasonable-length").unwrap(),
        AuthPolicy::Token
    );
}

#[test]
fn empty_token_without_opt_in_refuses_startup() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var(INSECURE_NO_AUTH_ENV);
    let err = auth_policy("").unwrap_err();
    assert!(err.contains("CORE_AUTH_TOKEN"), "got: {err}");
    assert!(err.contains(INSECURE_NO_AUTH_ENV), "got: {err}");
}

#[test]
fn empty_token_with_explicit_opt_in_is_insecure_mode() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var(INSECURE_NO_AUTH_ENV, "1");
    assert_eq!(auth_policy("").unwrap(), AuthPolicy::InsecureNoAuth);
    std::env::remove_var(INSECURE_NO_AUTH_ENV);
}

#[test]
fn opt_in_requires_exact_value_one() {
    let _guard = ENV_LOCK.lock().unwrap();
    for v in ["true", "yes", "0", ""] {
        std::env::set_var(INSECURE_NO_AUTH_ENV, v);
        assert!(auth_policy("").is_err(), "value {v:?} must not opt in");
    }
    std::env::remove_var(INSECURE_NO_AUTH_ENV);
}

#[test]
fn opt_in_never_overrides_a_configured_token() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var(INSECURE_NO_AUTH_ENV, "1");
    assert_eq!(auth_policy("configured-token").unwrap(), AuthPolicy::Token);
    std::env::remove_var(INSECURE_NO_AUTH_ENV);
}

#[test]
fn describe_warns_on_short_token_and_flags_insecure_mode() {
    assert!(describe(AuthPolicy::Token, "short").contains("WARNING"));
    assert!(!describe(AuthPolicy::Token, "a-token-that-is-long-enough").contains("WARNING"));
    assert!(describe(AuthPolicy::InsecureNoAuth, "").contains("DISABLED"));
}
