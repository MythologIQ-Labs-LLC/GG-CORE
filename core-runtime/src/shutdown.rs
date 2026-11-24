//! Graceful shutdown coordination for CORE Runtime.
//!
//! Provides a state machine for clean process termination that drains
//! in-flight requests before exit.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};

/// Shutdown state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownState {
    Running,
    Draining,
    Stopped,
}

/// Result of a shutdown operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownResult {
    Complete,
    Timeout { remaining: u32 },
}

/// Coordinates graceful shutdown across runtime components.
pub struct ShutdownCoordinator {
    state: Arc<RwLock<ShutdownState>>,
    in_flight: Arc<AtomicU32>,
    notify: Arc<Notify>,
}

impl ShutdownCoordinator {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(ShutdownState::Running)),
            in_flight: Arc::new(AtomicU32::new(0)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Get current shutdown state.
    pub async fn state(&self) -> ShutdownState {
        *self.state.read().await
    }

    /// Check if accepting new requests.
    pub fn is_accepting(&self) -> bool {
        // Use try_read to avoid blocking
        self.state
            .try_read()
            .map(|s| *s == ShutdownState::Running)
            .unwrap_or(false)
    }

    /// Track an in-flight request. Returns None if shutting down.
    pub fn track(&self) -> Option<ShutdownGuard> {
        // Keep the admission read lock until the counter is charged. Checking
        // is_accepting() separately allowed initiate() to switch to Draining
        // (and observe zero work) between the check and fetch_add().
        let state = self.state.try_read().ok()?;
        if *state != ShutdownState::Running {
            return None;
        }
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        drop(state);
        Some(ShutdownGuard {
            counter: self.in_flight.clone(),
            notify: self.notify.clone(),
        })
    }

    /// Current in-flight request count.
    pub fn in_flight_count(&self) -> u32 {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Initiate shutdown: stop accepting, wait for drain.
    pub async fn initiate(&self, timeout: Duration) -> ShutdownResult {
        // Transition to draining
        {
            let mut state = self.state.write().await;
            *state = ShutdownState::Draining;
        }

        let result = self.wait_for_drain(timeout).await;

        // Transition to stopped
        {
            let mut state = self.state.write().await;
            *state = ShutdownState::Stopped;
        }

        result
    }

    async fn wait_for_drain(&self, timeout: Duration) -> ShutdownResult {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            // Register for notifications BEFORE reading the count. Otherwise
            // a guard may drop between reading a positive count and the
            // select! installing its waiter, forcing a spurious full timeout.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let count = self.in_flight_count();
            if count == 0 {
                return ShutdownResult::Complete;
            }

            let remaining_time = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining_time.is_zero() {
                return ShutdownResult::Timeout { remaining: count };
            }

            tokio::select! {
                _ = &mut notified => continue,
                _ = tokio::time::sleep(remaining_time) => {
                    let final_count = self.in_flight_count();
                    if final_count == 0 {
                        return ShutdownResult::Complete;
                    }
                    return ShutdownResult::Timeout { remaining: final_count };
                }
            }
        }
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for in-flight request tracking.
pub struct ShutdownGuard {
    counter: Arc<AtomicU32>,
    notify: Arc<Notify>,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

#[cfg(test)]
#[path = "shutdown_tests.rs"]
mod tests;
