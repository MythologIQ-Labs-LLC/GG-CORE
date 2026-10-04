//! Authentication Attack Tests
//!
//! Tests for authentication bypass attempts, token manipulation, and session attacks.

use gg_core::ipc::{AuthError, SessionAuth};
use std::collections::HashSet;
use std::time::Duration;

/// Wrong token is rejected.
#[tokio::test]
async fn reject_wrong_token() {
    let auth = SessionAuth::new("correct-token", Duration::from_secs(3600));
    let result = auth.authenticate("wrong-token").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
}

/// Empty token is rejected.
#[tokio::test]
async fn reject_empty_token() {
    let auth = SessionAuth::new("correct-token", Duration::from_secs(3600));
    let result = auth.authenticate("").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
}

/// Correct token is accepted.
#[tokio::test]
async fn accept_correct_token() {
    let auth = SessionAuth::new("correct-token", Duration::from_secs(3600));
    let result = auth.authenticate("correct-token").await;
    assert!(result.is_ok());
}

/// Session tokens are unique.
#[tokio::test]
async fn session_tokens_unique() {
    let auth = SessionAuth::new("token", Duration::from_secs(3600));
    let mut sessions = HashSet::new();
    for _ in 0..100 {
        let session = auth.authenticate("token").await.unwrap();
        sessions.insert(session.as_str().to_string());
    }
    assert_eq!(sessions.len(), 100, "All session tokens should be unique");
}

/// Session tokens are 64 hex characters.
#[tokio::test]
async fn session_token_format() {
    let auth = SessionAuth::new("token", Duration::from_secs(3600));
    let session = auth.authenticate("token").await.unwrap();
    assert_eq!(session.as_str().len(), 64);
    assert!(session.as_str().chars().all(|c| c.is_ascii_hexdigit()));
}

/// Valid session is accepted.
#[tokio::test]
async fn accept_valid_session() {
    let auth = SessionAuth::new("token", Duration::from_secs(3600));
    let session = auth.authenticate("token").await.unwrap();
    let result = auth.validate(&session).await;
    assert!(result.is_ok());
}

/// Expired session is rejected.
#[tokio::test]
async fn reject_expired_session() {
    let auth = SessionAuth::new("token", Duration::from_millis(1));
    let session = auth.authenticate("token").await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let result = auth.validate(&session).await;
    assert!(matches!(result, Err(AuthError::SessionExpired)));
}

/// Cleanup removes expired sessions.
#[tokio::test]
async fn cleanup_removes_expired() {
    let auth = SessionAuth::new("token", Duration::from_millis(1));
    let session = auth.authenticate("token").await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    auth.cleanup().await;
    let result = auth.validate(&session).await;
    assert!(matches!(result, Err(AuthError::SessionNotFound)));
}

/// Rate limiting after failed attempts.
#[tokio::test]
async fn rate_limiting_after_failures() {
    let auth = SessionAuth::new("correct-token", Duration::from_secs(3600));
    for _ in 0..5 {
        let _ = auth.authenticate("wrong-token").await;
    }
    let result = auth.authenticate("correct-token").await;
    assert!(matches!(result, Err(AuthError::RateLimited)));
}

/// Rate limit resets after successful auth.
#[tokio::test]
async fn rate_limit_reset_on_success() {
    let auth = SessionAuth::new("correct-token", Duration::from_secs(3600));
    for _ in 0..3 {
        let _ = auth.authenticate("wrong-token").await;
    }
    let result = auth.authenticate("correct-token").await;
    assert!(result.is_ok());
    let result = auth.authenticate("correct-token").await;
    assert!(result.is_ok());
}

/// Multiple sessions can coexist.
#[tokio::test]
async fn multiple_sessions() {
    let auth = SessionAuth::new("token", Duration::from_secs(3600));
    let session1 = auth.authenticate("token").await.unwrap();
    let session2 = auth.authenticate("token").await.unwrap();
    assert_ne!(session1, session2);
    assert!(auth.validate(&session1).await.is_ok());
    assert!(auth.validate(&session2).await.is_ok());
}

/// Connection tracking works.
#[tokio::test]
async fn connection_tracking() {
    let auth = SessionAuth::new("token", Duration::from_secs(3600));
    let session = auth.authenticate("token").await.unwrap();
    let count1 = auth.track_connection(&session).await.unwrap();
    assert_eq!(count1, 1);
    let count2 = auth.track_connection(&session).await.unwrap();
    assert_eq!(count2, 2);
    auth.release_connection(&session).await;
    let current = auth.connection_count(&session).await.unwrap();
    assert_eq!(current, 1);
}

/// AuthError display messages.
#[test]
fn auth_error_display() {
    assert!(AuthError::InvalidToken.to_string().contains("Invalid"));
    assert!(AuthError::SessionNotFound.to_string().contains("not found"));
    assert!(AuthError::SessionExpired.to_string().contains("expired"));
    assert!(AuthError::NotAuthenticated.to_string().contains("required"));
    assert!(AuthError::RateLimited.to_string().contains("try again"));
}

/// Token with special characters.
#[tokio::test]
async fn token_with_special_chars() {
    let special_token = "token-with-special-chars-!@#$%^&*()";
    let auth = SessionAuth::new(special_token, Duration::from_secs(3600));
    let result = auth.authenticate(special_token).await;
    assert!(result.is_ok());
}

/// Token with unicode characters.
#[tokio::test]
async fn token_with_unicode() {
    let unicode_token = "中文密码";
    let auth = SessionAuth::new(unicode_token, Duration::from_secs(3600));
    let result = auth.authenticate(unicode_token).await;
    assert!(result.is_ok());
}

/// Very long token.
#[tokio::test]
async fn very_long_token() {
    let long_token = "x".repeat(10000);
    let auth = SessionAuth::new(&long_token, Duration::from_secs(3600));
    let result = auth.authenticate(&long_token).await;
    assert!(result.is_ok());
}

/// Prefix attack - token that is prefix of correct token.
#[tokio::test]
async fn prefix_attack() {
    let auth = SessionAuth::new("secret-token-12345", Duration::from_secs(3600));
    let result = auth.authenticate("secret-token").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
}

/// Suffix attack - token that is suffix of correct token.
#[tokio::test]
async fn suffix_attack() {
    let auth = SessionAuth::new("secret-token-12345", Duration::from_secs(3600));
    let result = auth.authenticate("token-12345").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
}

/// Case sensitivity - tokens are case-sensitive.
#[tokio::test]
async fn case_sensitivity() {
    let auth = SessionAuth::new("SecretToken", Duration::from_secs(3600));
    let result = auth.authenticate("secrettoken").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
    let result = auth.authenticate("SECRETTOKEN").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
    let result = auth.authenticate("SecretToken").await;
    assert!(result.is_ok());
}

/// Whitespace differences in token.
#[tokio::test]
async fn whitespace_differences() {
    let auth = SessionAuth::new("secret token", Duration::from_secs(3600));
    let result = auth.authenticate("secret  token").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
    let result = auth.authenticate(" secret token").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
    let result = auth.authenticate("secret token ").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
    let result = auth.authenticate("secret token").await;
    assert!(result.is_ok());
}

/// Null byte in token.
#[tokio::test]
async fn null_byte_in_token() {
    let token_with_null = "secret\x00token";
    let auth = SessionAuth::new(token_with_null, Duration::from_secs(3600));
    let result = auth.authenticate(token_with_null).await;
    assert!(result.is_ok());
    let result = auth.authenticate("secret").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));
}
