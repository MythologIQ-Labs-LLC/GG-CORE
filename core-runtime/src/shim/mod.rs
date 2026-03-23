// Copyright 2024-2026 GG-CORE Contributors
// SPDX-License-Identifier: Apache-2.0

//! Request shim interface for extensible request interception.
//!
//! This module provides trait-based extension points for:
//! - Rate limiting
//! - Priority tagging
//! - Tenant context injection
//!
//! # Open Core Architecture
//!
//! The base GG-CORE runtime provides a no-op `PassthroughInterceptor`.
//! Commercial extensions (GG-CORE Nexus) can provide implementations
//! with multi-tenant features, rate limiting, and priority queuing.
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │                  GG-CORE (OSS)                  │
//! │  ┌───────────────────────────────────────────┐  │
//! │  │         RequestInterceptor trait          │  │
//! │  │  (PassthroughInterceptor default impl)    │  │
//! │  └───────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────┘
//!                        ▲
//!                        │ implements
//! ┌─────────────────────────────────────────────────┐
//! │             GG-CORE Nexus (Private)             │
//! │  ┌───────────────────────────────────────────┐  │
//! │  │    TieredInterceptor implementation       │  │
//! │  │  - ServiceTier (Bronze/Silver/Gold)       │  │
//! │  │  - RateLimiter (token bucket)             │  │
//! │  │  - TenantContext (arena allocator)        │  │
//! │  └───────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────┘
//! ```

pub mod rate_limiter;
pub mod service_tier;

use std::fmt;
use std::sync::Arc;

use crate::ipc::protocol::InferenceRequest;
use crate::scheduler::Priority;

pub use rate_limiter::RateLimiter;
pub use service_tier::ServiceTier;

/// Error returned when request interception fails.
#[derive(Debug, Clone)]
pub enum InterceptError {
    /// Request rejected due to rate limiting.
    RateLimited {
        retry_after_ms: u64,
    },
    /// Session not authorized for this tier.
    TierNotAllowed {
        requested: String,
        allowed: String,
    },
    /// Internal interceptor error.
    Internal(String),
}

impl fmt::Display for InterceptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RateLimited { retry_after_ms } => {
                write!(f, "Rate limited. Retry after {}ms", retry_after_ms)
            }
            Self::TierNotAllowed { requested, allowed } => {
                write!(f, "Tier {} not allowed. Max tier: {}", requested, allowed)
            }
            Self::Internal(msg) => write!(f, "Interceptor error: {}", msg),
        }
    }
}

impl std::error::Error for InterceptError {}

/// Result of successful interception with optional metadata.
#[derive(Debug, Clone, Default)]
pub struct InterceptResult {
    /// Adjusted priority for the request.
    pub priority: Option<Priority>,
    /// Session identifier for tracking.
    pub session_id: Option<String>,
    /// Opaque context data for downstream processing.
    pub context: Option<Vec<u8>>,
}

/// Trait for request interception before inference.
///
/// Implementors can:
/// - Reject requests (rate limiting)
/// - Adjust priority
/// - Attach session context
///
/// # Thread Safety
/// Implementations must be `Send + Sync` for use across async tasks.
pub trait RequestInterceptor: Send + Sync {
    /// Intercept a request before processing.
    ///
    /// # Arguments
    /// * `request` - The inference request to intercept
    /// * `session_token` - Optional session token for authentication
    ///
    /// # Returns
    /// * `Ok(InterceptResult)` - Request allowed with optional metadata
    /// * `Err(InterceptError)` - Request rejected
    fn intercept(
        &self,
        request: &InferenceRequest,
        session_token: Option<&str>,
    ) -> Result<InterceptResult, InterceptError>;

    /// Cleanup resources for a session (called on disconnect).
    fn cleanup_session(&self, _session_id: &str) {
        // Default: no-op
    }

    /// Periodic maintenance (called by background task).
    fn maintenance(&self) {
        // Default: no-op
    }
}

/// No-op interceptor that allows all requests.
///
/// This is the default implementation shipped with GG-CORE OSS.
/// It passes through all requests without modification.
#[derive(Debug, Clone, Default)]
pub struct PassthroughInterceptor;

impl RequestInterceptor for PassthroughInterceptor {
    fn intercept(
        &self,
        _request: &InferenceRequest,
        _session_token: Option<&str>,
    ) -> Result<InterceptResult, InterceptError> {
        Ok(InterceptResult::default())
    }
}

/// Factory for creating interceptor instances.
///
/// External crates can register custom factories via the `shim-external` feature.
pub type InterceptorFactory = Arc<dyn Fn() -> Arc<dyn RequestInterceptor> + Send + Sync>;

/// Get the default interceptor.
///
/// Returns `PassthroughInterceptor` unless an external shim is registered.
pub fn default_interceptor() -> Arc<dyn RequestInterceptor> {
    Arc::new(PassthroughInterceptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_allows_all() {
        let interceptor = PassthroughInterceptor;
        let request = InferenceRequest {
            request_id: crate::ipc::protocol::RequestId(1),
            model_id: "test".to_string(),
            prompt: "Hello".to_string(),
            parameters: Default::default(),
        };

        let result = interceptor.intercept(&request, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_intercept_result_default() {
        let result = InterceptResult::default();
        assert!(result.priority.is_none());
        assert!(result.session_id.is_none());
        assert!(result.context.is_none());
    }

    #[test]
    fn test_intercept_error_display() {
        let err = InterceptError::RateLimited { retry_after_ms: 1000 };
        assert!(err.to_string().contains("1000"));

        let err = InterceptError::TierNotAllowed {
            requested: "Gold".to_string(),
            allowed: "Silver".to_string(),
        };
        assert!(err.to_string().contains("Gold"));
        assert!(err.to_string().contains("Silver"));
    }
}
