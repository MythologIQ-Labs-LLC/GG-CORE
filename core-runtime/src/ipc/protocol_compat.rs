//! Rust source-compatibility contract for [`IpcMessage`](super::protocol_types::IpcMessage).
//!
//! Source compatibility (#112, owner disposition 2026-11-13): the enum is
//! `#[non_exhaustive]`, so a new frame type is not a breaking change for
//! crates that depend on `gg_core`. Their matches need a wildcard arm:
//!
//! ```
//! use gg_core::ipc::protocol_types::IpcMessage;
//!
//! fn is_error(message: &IpcMessage) -> bool {
//!     match message {
//!         IpcMessage::Error { .. } => true,
//!         _ => false,
//!     }
//! }
//! # let _ = is_error;
//! ```
//!
//! An external match that names every current variant but has no wildcard
//! arm is rejected (E0004, non-exhaustive patterns):
//!
//! ```compile_fail,E0004
//! use gg_core::ipc::protocol_types::IpcMessage as M;
//!
//! fn kind(message: &M) -> u8 {
//!     match message {
//!         M::Handshake { .. } => 0,
//!         M::HandshakeAck { .. } => 1,
//!         M::InferenceRequest { .. } => 2,
//!         M::InferenceResponse { .. } => 3,
//!         M::StreamChunk { .. } => 4,
//!         M::HealthCheck { .. } => 5,
//!         M::HealthResponse { .. } => 6,
//!         M::MetricsRequest { .. } => 7,
//!         M::MetricsResponse { .. } => 8,
//!         M::PrometheusMetricsRequest { .. } => 9,
//!         M::PrometheusMetricsResponse { .. } => 10,
//!         M::SpansRequest { .. } => 11,
//!         M::SpansResponse { .. } => 12,
//!         M::CancelRequest { .. } => 13,
//!         M::CancelResponse { .. } => 14,
//!         M::WarmupRequest { .. } => 15,
//!         M::WarmupResponse { .. } => 16,
//!         M::ModelsRequest { .. } => 17,
//!         M::ModelsResponse { .. } => 18,
//!         M::ModelLoadRequest { .. } => 19,
//!         M::ModelLoadResponse { .. } => 20,
//!         M::ModelUnloadRequest { .. } => 21,
//!         M::ModelUnloadResponse { .. } => 22,
//!         M::TypedInferenceRequest { .. } => 23,
//!         M::TypedInferenceResponse { .. } => 24,
//!         M::ModelReleaseWaitRequest { .. } => 25,
//!         M::ModelReleaseWaitResponse { .. } => 26,
//!         M::Error { .. } => 27,
//!     }
//! }
//! # let _ = kind;
//! ```
