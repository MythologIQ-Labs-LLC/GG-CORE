//! Request/response handling for IPC connections.

use std::sync::Arc;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::auth::{AuthError, SessionAuth, SessionToken};
use super::health_handler::HealthHandler;
use super::protocol::{
    decode_message, encode_message, InferenceErrorCode, InferenceRequest, InferenceResponse,
    IpcMessage, ModelInfo, ModelsListResponse, ProtocolError, ProtocolVersion, StreamChunk,
    WarmupResponse,
};
use crate::engine::InferenceEngine;
use crate::health::HealthChecker;
use crate::models::ModelRegistry;
use crate::scheduler::Priority;
use crate::scheduler::RequestQueue;
use crate::shutdown::ShutdownCoordinator;
use crate::telemetry::MetricsStore;

#[derive(Error, Debug)]
pub enum HandlerError {
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

    #[error("Not authenticated")]
    NotAuthenticated,

    #[error("Queue error: {0}")]
    QueueFull(String),

    #[error("Server is shutting down")]
    ShuttingDown,

    #[error("Stream send error: {0}")]
    StreamSend(String),
}

/// Configuration for IPC handler.
#[derive(Debug, Clone)]
pub struct IpcHandlerConfig {
    pub require_auth: bool,
    /// Longest a stream send or frame write may wait for a client that is
    /// not reading (`GG_CORE_STREAM_STALL_MS`).
    pub stream_stall: std::time::Duration,
}

impl Default for IpcHandlerConfig {
    fn default() -> Self {
        Self {
            require_auth: true,
            stream_stall: crate::engine::DEFAULT_STREAM_STALL,
        }
    }
}

/// Trait for sending streaming responses over IPC.
#[async_trait::async_trait]
pub trait StreamSender: Send + Sync {
    /// Send a message to the stream. Returns error if stream is closed.
    async fn send(&self, message: IpcMessage) -> Result<(), HandlerError>;
}

/// Format SystemTime as ISO 8601 string for IPC responses.
fn format_system_time(time: std::time::SystemTime) -> String {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let datetime = chrono::DateTime::from_timestamp(secs as i64, 0);
            datetime
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_else(|| format!("{}s", secs))
        })
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Handles IPC message processing with authentication.
pub struct IpcHandler {
    /// Session authentication manager (public for FFI access)
    pub auth: Arc<SessionAuth>,
    queue: Arc<RequestQueue>,
    config: IpcHandlerConfig,
    shutdown: Arc<ShutdownCoordinator>,
    health_handler: HealthHandler,
    metrics_store: Arc<MetricsStore>,
    model_registry: Arc<ModelRegistry>,
    /// Model lifecycle coordinator for authenticated load/unload (B-41, #106).
    pub(super) model_lifecycle: Arc<crate::models::ModelLifecycle>,
    /// Path-validating loader bound to the runtime's base path (B-41, #106).
    pub(super) model_loader: Arc<crate::models::ModelLoader>,
    /// Streaming path (gguf feature) and typed capability listing.
    inference_engine: Arc<InferenceEngine>,
}

impl IpcHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        auth: Arc<SessionAuth>,
        queue: Arc<RequestQueue>,
        config: IpcHandlerConfig,
        shutdown: Arc<ShutdownCoordinator>,
        health: Arc<HealthChecker>,
        model_registry: Arc<ModelRegistry>,
        metrics_store: Arc<MetricsStore>,
        inference_engine: Arc<InferenceEngine>,
        model_lifecycle: Arc<crate::models::ModelLifecycle>,
        model_loader: Arc<crate::models::ModelLoader>,
    ) -> Self {
        let health_handler = HealthHandler::new(
            health,
            Arc::clone(&shutdown),
            Arc::clone(&model_registry),
            Arc::clone(&queue),
        );
        Self {
            auth,
            queue,
            config,
            shutdown,
            health_handler,
            metrics_store,
            model_registry,
            model_lifecycle,
            model_loader,
            inference_engine,
        }
    }

    /// Process incoming message bytes and return response bytes.
    pub async fn process(
        &self,
        bytes: &[u8],
        session: Option<&SessionToken>,
    ) -> Result<(Vec<u8>, Option<SessionToken>), HandlerError> {
        let message = decode_message(bytes)?;
        let (response, new_session) = self.handle_message(message, session).await?;
        let response_bytes = encode_message(&response)?;
        Ok((response_bytes, new_session))
    }

    async fn handle_message(
        &self,
        message: IpcMessage,
        session: Option<&SessionToken>,
    ) -> Result<(IpcMessage, Option<SessionToken>), HandlerError> {
        match message {
            IpcMessage::Handshake {
                token,
                protocol_version,
            } => {
                let session_token = self.auth.authenticate(&token).await?;
                // Negotiate protocol version with client
                let negotiated_version = ProtocolVersion::negotiate(protocol_version);
                let response = IpcMessage::HandshakeAck {
                    session_id: session_token.as_str().to_string(),
                    protocol_version: negotiated_version,
                    features: vec![super::protocol_typed::TYPED_INFERENCE_FEATURE.to_string()],
                };
                Ok((response, Some(session_token)))
            }

            IpcMessage::InferenceRequest(request) => {
                self.require_auth(session).await?;
                let response = self.handle_inference(request).await;
                Ok((IpcMessage::InferenceResponse(response), None))
            }

            IpcMessage::TypedInferenceRequest(request) => {
                // Connection servers route this beside the reader (cancellable);
                // direct `process` callers get the same authenticated path.
                self.require_auth(session).await?;
                let cancel = tokio_util::sync::CancellationToken::new();
                let response = self.handle_typed_controlled(request, cancel).await;
                Ok((IpcMessage::TypedInferenceResponse(response), None))
            }

            IpcMessage::HealthCheck { check_type } => {
                // NO AUTH REQUIRED for health checks (orchestrator pattern)
                let response = self.health_handler.handle(check_type).await;
                Ok((IpcMessage::HealthResponse(response), None))
            }

            IpcMessage::MetricsRequest => {
                // NO AUTH REQUIRED for metrics (orchestrator pattern, same as health)
                let snapshot = self.metrics_store.snapshot();
                Ok((IpcMessage::MetricsResponse(snapshot), None))
            }

            IpcMessage::ModelsRequest => {
                // NO AUTH REQUIRED for model listing (orchestrator pattern, same as health/metrics).
                // Task capabilities (#171) are disclosed to authenticated sessions only.
                let authenticated = self.require_auth(session).await.is_ok();
                let response = self.handle_models_request(authenticated).await;
                Ok((IpcMessage::ModelsResponse(response), None))
            }

            IpcMessage::CancelRequest { request_id } => {
                // AUTH REQUIRED for cancellation (session-scoped operation)
                self.require_auth(session).await?;
                let cancelled = self.queue.cancel(request_id.0).await;
                Ok((
                    IpcMessage::CancelResponse {
                        request_id,
                        cancelled,
                    },
                    None,
                ))
            }

            IpcMessage::WarmupRequest(request) => {
                // Warmup runs real inference and consumes compute; it must be
                // authorized like every other inference request.
                self.require_auth(session).await?;
                let response = self.handle_warmup(request.model_id, request.tokens).await;
                Ok((IpcMessage::WarmupResponse(response), None))
            }

            IpcMessage::ModelLoadRequest(request) => {
                // AUTH REQUIRED: load mutates runtime state (B-41, #106)
                self.require_auth(session).await?;
                let response = self.handle_model_load(request).await;
                Ok((IpcMessage::ModelLoadResponse(response), None))
            }

            IpcMessage::ModelUnloadRequest(request) => {
                // AUTH REQUIRED: unload mutates runtime state (B-41, #106)
                self.require_auth(session).await?;
                let response = self.handle_model_unload(request).await;
                Ok((IpcMessage::ModelUnloadResponse(response), None))
            }

            _ => {
                let error = IpcMessage::Error {
                    code: 400,
                    message: "Unexpected message type".into(),
                };
                Ok((error, None))
            }
        }
    }

    pub(super) async fn require_auth(
        &self,
        session: Option<&SessionToken>,
    ) -> Result<(), HandlerError> {
        if !self.config.require_auth {
            return Ok(());
        }

        let token = session.ok_or(HandlerError::NotAuthenticated)?;
        self.auth.validate(token).await?;
        Ok(())
    }

    /// Non-streaming inference without a connection-scoped cancel owner; the
    /// request's `timeout_ms` deadline still applies.
    async fn handle_inference(&self, request: InferenceRequest) -> InferenceResponse {
        self.handle_inference_controlled(request, CancellationToken::new())
            .await
    }

    async fn handle_warmup(&self, model_id: String, _tokens: usize) -> WarmupResponse {
        let start = std::time::Instant::now();

        // Real warmup: 1-token inference through the queue
        let warmup_params = crate::engine::InferenceParams {
            max_tokens: 1,
            ..Default::default()
        };

        let enqueue_result = self
            .queue
            .enqueue_with_response(
                model_id.clone(),
                "Hello".to_string(),
                warmup_params,
                Priority::Low,
            )
            .await;

        let rx = match enqueue_result {
            Ok((_id, rx)) => rx,
            Err(e) => {
                let elapsed = start.elapsed().as_millis() as u64;
                return WarmupResponse::error(model_id, e.to_string(), elapsed);
            }
        };

        // Await actual inference result
        let elapsed_ms = match rx.await {
            Ok(Ok(_)) => start.elapsed().as_millis() as u64,
            Ok(Err(e)) => {
                let elapsed = start.elapsed().as_millis() as u64;
                return WarmupResponse::error(model_id, e, elapsed);
            }
            Err(_) => {
                let elapsed = start.elapsed().as_millis() as u64;
                return WarmupResponse::error(model_id, "worker unavailable".into(), elapsed);
            }
        };

        WarmupResponse::success(model_id, elapsed_ms)
    }

    async fn handle_models_request(&self, authenticated: bool) -> ModelsListResponse {
        let mut models = self.model_registry.list_models().await;
        self.append_releasing(&mut models);
        let total_memory_bytes = models.iter().map(|m| m.memory_bytes).sum();

        let mut capabilities = std::collections::HashMap::new();
        for m in models.iter().filter(|_| authenticated) {
            if let Some(id) = self.model_lifecycle.get_model_id(m.handle_id).await {
                let caps = self.inference_engine.model_capabilities(&id).await;
                capabilities.insert(m.handle_id, caps);
            }
        }
        let model_infos: Vec<ModelInfo> = models
            .into_iter()
            .map(|m| {
                let avg_latency_ms = if m.request_count > 0 {
                    m.total_latency_ms / m.request_count as f64
                } else {
                    0.0
                };
                ModelInfo {
                    handle_id: m.handle_id,
                    name: m.name,
                    format: m.format,
                    size_bytes: m.size_bytes,
                    memory_bytes: m.memory_bytes,
                    state: m.state.as_str().to_string(),
                    request_count: m.request_count,
                    avg_latency_ms,
                    loaded_at: format_system_time(m.loaded_at),
                    capabilities: capabilities
                        .remove(&m.handle_id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|c| super::protocol_typed::capability_name(c).to_string())
                        .collect(),
                }
            })
            .collect();

        ModelsListResponse {
            models: model_infos,
            total_memory_bytes,
        }
    }

    /// Process streaming inference request. Sends token chunks via sender.
    ///
    /// Creates a token stream channel, spawns inference on a blocking task,
    /// and relays tokens to the client until completion or cancellation.
    #[allow(unused_variables)]
    pub async fn process_streaming(
        &self,
        request: InferenceRequest,
        session: &SessionToken,
        sender: &dyn StreamSender,
        cancel: CancellationToken,
    ) -> Result<(), HandlerError> {
        self.auth.validate(session).await?;
        let _guard = self.shutdown.track().ok_or(HandlerError::ShuttingDown)?;

        if let Err(e) = request.validate() {
            let chunk = StreamChunk::error(request.request_id, e.to_string());
            sender.send(IpcMessage::StreamChunk(chunk)).await?;
            return Ok(());
        }

        if let Err(e) = request.parameters.validate() {
            let chunk = StreamChunk::error(request.request_id, e.to_string());
            sender.send(IpcMessage::StreamChunk(chunk)).await?;
            return Ok(());
        }

        // Streaming requires gguf feature
        #[cfg(not(feature = "gguf"))]
        {
            let chunk = StreamChunk::error(
                request.request_id,
                "Streaming requires GGUF feature. Rebuild with --features gguf.".into(),
            );
            sender.send(IpcMessage::StreamChunk(chunk)).await?;
            Ok(())
        }

        #[cfg(feature = "gguf")]
        {
            self.run_streaming_inference(request, sender, cancel).await
        }
    }
}

impl IpcHandler {
    /// Stall bound for stream sends and every frame write on a connection.
    pub fn stream_stall(&self) -> std::time::Duration {
        self.config.stream_stall
    }
}

#[path = "handler_control.rs"]
mod handler_control;

#[cfg(feature = "gguf")]
#[path = "stream_relay.rs"]
mod stream_relay;

#[path = "handler_inference.rs"]
mod inference_control;

#[path = "handler_typed.rs"]
mod typed_control;

/// Map a worker error string to a structured `InferenceErrorCode`.
///
/// The worker stringifies `engine::InferenceError` variants. We classify by
/// matching known prefixes so callers can distinguish admission rejections
/// (retriable) from execution failures (not retriable without change).
fn classify_worker_error(msg: &str) -> InferenceErrorCode {
    if msg.contains("Request cancelled") {
        InferenceErrorCode::Cancelled
    } else if msg.contains("Deadline exceeded") {
        InferenceErrorCode::DeadlineExceeded
    } else if msg.contains("Memory limit exceeded")
        || msg.contains("queue full")
        || msg.contains("Queue full")
    {
        InferenceErrorCode::AdmissionRejected
    } else if msg.contains("Model not loaded") {
        InferenceErrorCode::ModelNotLoaded
    } else if msg.contains("Context length exceeded") || msg.contains("Input validation failed") {
        // Engine estimate or the GGUF backend's exact tokenized check: the
        // prompt cannot fit the configured context. Retrying cannot succeed.
        InferenceErrorCode::InputInvalid
    } else {
        InferenceErrorCode::ExecutionFailed
    }
}

#[cfg(test)]
mod classify_tests {
    use super::*;

    #[test]
    fn context_overflow_is_input_invalid() {
        for msg in [
            "Context length exceeded: max 4096, got 5000",
            "Inference failed: Input validation failed: prompt contains 5000 tokens, \
             leaving no room in the 4096-token context",
        ] {
            assert_eq!(classify_worker_error(msg), InferenceErrorCode::InputInvalid);
        }
    }

    #[test]
    fn other_worker_errors_keep_their_codes() {
        assert_eq!(
            classify_worker_error("Queue full: 3/3 pending requests"),
            InferenceErrorCode::AdmissionRejected
        );
        assert_eq!(
            classify_worker_error("Model not loaded: qwen"),
            InferenceErrorCode::ModelNotLoaded
        );
        assert_eq!(
            classify_worker_error("Inference failed: Model error: decode: -1"),
            InferenceErrorCode::ExecutionFailed
        );
    }

    #[test]
    fn stopped_requests_get_explicit_codes() {
        let cancelled = crate::engine::inference::InferenceError::Cancelled.to_string();
        assert_eq!(
            classify_worker_error(&cancelled),
            InferenceErrorCode::Cancelled
        );
        let expired =
            crate::engine::inference::InferenceError::DeadlineExceeded { timeout_ms: 350 };
        assert_eq!(
            classify_worker_error(&expired.to_string()),
            InferenceErrorCode::DeadlineExceeded
        );
    }
}
