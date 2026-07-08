//! Span utilities and extension traits for CORE Runtime tracing.
//!
//! Provides standardized span creation and result recording.

use tracing::{info_span, Span};

/// Extension trait for adding context to spans.
pub trait SpanExt {
    /// Record the result of an operation into the span.
    fn record_result<T, E>(&self, result: &Result<T, E>)
    where
        E: std::fmt::Display;
}

impl SpanExt for Span {
    fn record_result<T, E>(&self, result: &Result<T, E>)
    where
        E: std::fmt::Display,
    {
        match result {
            Ok(_) => {
                self.record("status", "ok");
            }
            Err(e) => {
                self.record("status", "error");
                self.record("error.message", e.to_string().as_str());
            }
        }
    }
}

/// Factory for creating standardized request spans.
pub struct RequestSpan;

impl RequestSpan {
    /// Create a new request span with standard fields.
    ///
    /// Fields included:
    /// - `request_id`: Unique identifier for the request
    /// - `model_id`: Model being used for inference
    /// - `status`: To be filled in by `SpanExt::record_result`
    /// - `error.message`: To be filled in on error
    /// - `latency_ms`: To be filled in after completion
    /// - `tokens_generated`: To be filled in after generation
    #[allow(clippy::new_ret_no_self)]
    pub fn new(request_id: &str, model_id: &str) -> Span {
        info_span!(
            "inference_request",
            request_id = %request_id,
            model_id = %model_id,
            status = tracing::field::Empty,
            error.message = tracing::field::Empty,
            latency_ms = tracing::field::Empty,
            tokens_generated = tracing::field::Empty,
        )
    }
}
