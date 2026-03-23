//! Tests for connection pool management.

use std::sync::Arc;
use std::time::Duration;

use gg_core::ipc::{ConnectionConfig, ConnectionPool, SessionAuth};

#[test]
fn test_acquire_within_limit() {
    let config = ConnectionConfig { max_connections: 2 };
    let pool = ConnectionPool::new(config);

    let guard1 = pool.try_acquire();
    assert!(guard1.is_some());
    assert_eq!(pool.active_count(), 1);

    let guard2 = pool.try_acquire();
    assert!(guard2.is_some());
    assert_eq!(pool.active_count(), 2);
}

#[test]
fn test_acquire_at_limit() {
    let config = ConnectionConfig { max_connections: 1 };
    let pool = ConnectionPool::new(config);

    let _guard = pool.try_acquire();
    assert_eq!(pool.active_count(), 1);

    // Should fail - at limit
    let guard2 = pool.try_acquire();
    assert!(guard2.is_none());
    assert_eq!(pool.active_count(), 1);
}

#[test]
fn test_guard_releases_on_drop() {
    let config = ConnectionConfig { max_connections: 1 };
    let pool = ConnectionPool::new(config);

    {
        let _guard = pool.try_acquire();
        assert_eq!(pool.active_count(), 1);
    }

    // Guard dropped, should release
    assert_eq!(pool.active_count(), 0);

    // Can acquire again
    let guard2 = pool.try_acquire();
    assert!(guard2.is_some());
}

#[test]
fn test_concurrent_acquire() {
    use std::thread;

    let config = ConnectionConfig { max_connections: 100 };
    let pool = Arc::new(ConnectionPool::new(config));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let pool = Arc::clone(&pool);
            thread::spawn(move || {
                let mut guards = Vec::new();
                for _ in 0..10 {
                    if let Some(guard) = pool.try_acquire() {
                        guards.push(guard);
                    }
                }
                guards.len()
            })
        })
        .collect();

    let total: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    // All 100 connections should be acquired
    assert_eq!(total, 100);
}

#[tokio::test]
async fn test_session_connection_tracking() {
    let auth = SessionAuth::new("test-token", Duration::from_secs(60));

    // Authenticate
    let token = auth.authenticate("test-token").await.unwrap();

    // Track connections
    let count1 = auth.track_connection(&token).await.unwrap();
    assert_eq!(count1, 1);

    let count2 = auth.track_connection(&token).await.unwrap();
    assert_eq!(count2, 2);

    // Release one
    auth.release_connection(&token).await;
    let count = auth.connection_count(&token).await.unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_connection_config_defaults() {
    let config = ConnectionConfig::default();
    assert_eq!(config.max_connections, 64);
}
