//! Token bucket rate limiter with per-session isolation.
//!
//! Uses DashMap for lock-free concurrent access across sessions.
//! Each session gets an independent bucket based on its ServiceTier.

use std::time::Instant;

use dashmap::DashMap;

use super::service_tier::ServiceTier;

/// Per-session token bucket state.
#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    tier: ServiceTier,
}

impl Bucket {
    fn new(tier: ServiceTier) -> Self {
        Self {
            tokens: tier.burst_capacity() as f64,
            last_refill: Instant::now(),
            tier,
        }
    }

    /// Refill tokens based on elapsed time, then try to consume one.
    fn try_consume(&mut self) -> Result<(), u64> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        let refill = elapsed * self.tier.rate_limit_rps() as f64;
        let cap = self.tier.burst_capacity() as f64;
        self.tokens = (self.tokens + refill).min(cap);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - self.tokens;
            let wait_ms = (deficit / self.tier.rate_limit_rps() as f64) * 1000.0;
            Err(wait_ms.ceil() as u64)
        }
    }
}

/// Concurrent per-session rate limiter.
pub struct RateLimiter {
    buckets: DashMap<String, Bucket>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: DashMap::new(),
        }
    }

    /// Check if a request is allowed for the given session.
    ///
    /// Returns `Ok(())` if allowed, `Err(retry_after_ms)` if rate limited.
    pub fn check(&self, session_id: &str, tier: ServiceTier) -> Result<(), u64> {
        let mut entry = self
            .buckets
            .entry(session_id.to_string())
            .or_insert_with(|| Bucket::new(tier));
        entry.value_mut().try_consume()
    }

    /// Remove stale buckets that haven't been used recently.
    pub fn cleanup(&self, max_idle_secs: u64) {
        let cutoff = Instant::now() - std::time::Duration::from_secs(max_idle_secs);
        self.buckets.retain(|_, bucket| bucket.last_refill > cutoff);
    }

    /// Remove a specific session's bucket.
    pub fn remove_session(&self, session_id: &str) {
        self.buckets.remove(session_id);
    }

    /// Number of active sessions being tracked.
    pub fn active_sessions(&self) -> usize {
        self.buckets.len()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_within_burst() {
        let limiter = RateLimiter::new();
        for _ in 0..10 {
            assert!(limiter.check("s1", ServiceTier::Bronze).is_ok());
        }
    }

    #[test]
    fn test_rejects_over_burst() {
        let limiter = RateLimiter::new();
        // Bronze burst = 10, exhaust it
        for _ in 0..10 {
            let _ = limiter.check("s1", ServiceTier::Bronze);
        }
        // Next should be rejected
        assert!(limiter.check("s1", ServiceTier::Bronze).is_err());
    }

    #[test]
    fn test_sessions_isolated() {
        let limiter = RateLimiter::new();
        // Exhaust s1
        for _ in 0..10 {
            let _ = limiter.check("s1", ServiceTier::Bronze);
        }
        // s2 should still work
        assert!(limiter.check("s2", ServiceTier::Bronze).is_ok());
    }

    #[test]
    fn test_cleanup_removes_stale() {
        let limiter = RateLimiter::new();
        limiter.check("s1", ServiceTier::Silver).unwrap();
        assert_eq!(limiter.active_sessions(), 1);

        // Cleanup with 0 idle = remove all
        limiter.cleanup(0);
        assert_eq!(limiter.active_sessions(), 0);
    }

    #[test]
    fn test_remove_session() {
        let limiter = RateLimiter::new();
        limiter.check("s1", ServiceTier::Silver).unwrap();
        limiter.remove_session("s1");
        assert_eq!(limiter.active_sessions(), 0);
    }

    #[test]
    fn test_gold_has_higher_burst() {
        let limiter = RateLimiter::new();
        // Gold burst = 200
        for _ in 0..200 {
            assert!(limiter.check("gold", ServiceTier::Gold).is_ok());
        }
    }
}
