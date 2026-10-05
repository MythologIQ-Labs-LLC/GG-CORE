//! Integration tests for the model lifecycle IPC surface (B-41, issue #106).
//!
//! Drives `IpcHandler::process` directly (no sockets, cross-platform):
//! authentication gating, path-traversal rejection, fail-loud load errors,
//! unload semantics, and model-gated readiness.

use std::sync::Arc;

use gg_core::engine::{
    GenerationResult, InferenceCapability, InferenceConfig, InferenceError, InferenceInput,
    InferenceOutput, Model,
};
use gg_core::health::HealthConfig;
use gg_core::ipc::protocol::{decode_message, encode_message};
use gg_core::ipc::{
    HealthCheckType, IpcMessage, ModelLoadRequest, ModelLoadResponse, ModelUnloadRequest,
    ModelUnloadResponse, SessionToken,
};
use gg_core::{Runtime, RuntimeConfig};

struct MockModel {
    id: String,
}

#[async_trait::async_trait]
impl Model for MockModel {
    fn model_id(&self) -> &str {
        &self.id
    }
    fn capabilities(&self) -> &[InferenceCapability] {
        &[InferenceCapability::TextGeneration]
    }
    fn memory_usage(&self) -> usize {
        1024
    }
    async fn infer(
        &self,
        _input: &InferenceInput,
        _config: &InferenceConfig,
    ) -> Result<InferenceOutput, InferenceError> {
        Ok(InferenceOutput::Generation(GenerationResult {
            text: "mock".into(),
            tokens_generated: 1,
            finish_reason: gg_core::engine::FinishReason::MaxTokens,
        }))
    }
    async fn unload(&mut self) -> Result<(), InferenceError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn test_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        auth_token: "test-token".into(),
        ..Default::default()
    })
}

async fn send(
    runtime: &Runtime,
    message: &IpcMessage,
    session: Option<&SessionToken>,
) -> (IpcMessage, Option<SessionToken>) {
    let bytes = encode_message(message).unwrap();
    let (response_bytes, new_session) = runtime
        .ipc_handler
        .process(&bytes, session)
        .await
        .expect("handler should produce a response");
    (decode_message(&response_bytes).unwrap(), new_session)
}

async fn authenticate(runtime: &Runtime) -> SessionToken {
    let handshake = IpcMessage::Handshake {
        token: "test-token".into(),
        protocol_version: None,
    };
    let (reply, session) = send(runtime, &handshake, None).await;
    assert!(matches!(reply, IpcMessage::HandshakeAck { .. }));
    session.expect("handshake must bind a session")
}

#[tokio::test]
async fn load_without_session_is_rejected() {
    let runtime = test_runtime();
    let request = IpcMessage::ModelLoadRequest(ModelLoadRequest {
        path: "models/anything.gguf".into(),
        model_id: None,
    });
    let bytes = encode_message(&request).unwrap();
    let result = runtime.ipc_handler.process(&bytes, None).await;
    assert!(result.is_err(), "unauthenticated load must be refused");
}

#[tokio::test]
async fn unload_without_session_is_rejected() {
    let runtime = test_runtime();
    let request = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: "any".into(),
    });
    let bytes = encode_message(&request).unwrap();
    assert!(runtime.ipc_handler.process(&bytes, None).await.is_err());
}

#[tokio::test]
async fn traversal_path_is_rejected_and_connection_survives() {
    let runtime = test_runtime();
    let session = authenticate(&runtime).await;
    let request = IpcMessage::ModelLoadRequest(ModelLoadRequest {
        path: "../../etc/passwd".into(),
        model_id: None,
    });
    let (reply, _) = send(&runtime, &request, Some(&session)).await;
    match reply {
        IpcMessage::ModelLoadResponse(r) => {
            assert!(!r.success);
            let err = r.error.unwrap();
            assert!(err.contains("path rejected"), "got: {err}");
        }
        other => panic!("unexpected reply: {other:?}"),
    }
    // Same session keeps working: the failure was a response, not a hangup.
    let (reply, _) = send(&runtime, &IpcMessage::ModelsRequest, Some(&session)).await;
    assert!(matches!(reply, IpcMessage::ModelsResponse(_)));
}

#[tokio::test]
async fn load_failure_leaves_no_registry_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    std::fs::write(dir.path().join("models/fake.gguf"), b"not a real model").unwrap();

    let runtime = Runtime::new(RuntimeConfig {
        auth_token: "test-token".into(),
        base_path: dir.path().to_path_buf(),
        ..Default::default()
    });
    let session = authenticate(&runtime).await;
    let request = IpcMessage::ModelLoadRequest(ModelLoadRequest {
        path: "models/fake.gguf".into(),
        model_id: Some("fake".into()),
    });
    // Default build: no gguf backend compiled -> fail-loud Backend error.
    // With gguf: the invalid file fails backend parsing. Either way: loud,
    // structured, and nothing registered.
    let (reply, _) = send(&runtime, &request, Some(&session)).await;
    match reply {
        IpcMessage::ModelLoadResponse(r) => {
            assert!(!r.success);
            assert!(r.error.is_some());
        }
        other => panic!("unexpected reply: {other:?}"),
    }
    assert_eq!(runtime.model_registry.count().await, 0, "no partial state");
}

#[tokio::test]
async fn unload_of_absent_model_fails_loud() {
    let runtime = test_runtime();
    let session = authenticate(&runtime).await;
    let request = IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
        model_id: "ghost".into(),
    });
    let (reply, _) = send(&runtime, &request, Some(&session)).await;
    match reply {
        IpcMessage::ModelUnloadResponse(r) => {
            assert!(!r.success);
            assert_eq!(r.model_id, "ghost");
            assert!(r.error.unwrap().contains("not loaded"));
        }
        other => panic!("unexpected reply: {other:?}"),
    }
}

#[tokio::test]
async fn readiness_requires_a_loaded_model_when_configured() {
    let runtime = Runtime::new(RuntimeConfig {
        auth_token: "test-token".into(),
        health: HealthConfig {
            require_model_loaded: true,
            ..Default::default()
        },
        ..Default::default()
    });

    let readiness = IpcMessage::HealthCheck {
        check_type: HealthCheckType::Readiness,
    };
    let (reply, _) = send(&runtime, &readiness, None).await;
    match reply {
        IpcMessage::HealthResponse(r) => assert!(!r.ok, "empty runtime must not be ready"),
        other => panic!("unexpected reply: {other:?}"),
    }

    // Register a model through the canonical lifecycle; readiness flips.
    let metadata = gg_core::models::ModelMetadata {
        name: "mock".into(),
        size_bytes: 1,
    };
    runtime
        .model_lifecycle
        .load(
            "mock".into(),
            metadata,
            Arc::new(MockModel { id: "mock".into() }),
        )
        .await
        .unwrap();

    let (reply, _) = send(&runtime, &readiness, None).await;
    match reply {
        IpcMessage::HealthResponse(r) => assert!(r.ok, "loaded runtime must be ready"),
        other => panic!("unexpected reply: {other:?}"),
    }
}

#[test]
fn protocol_round_trips_for_model_lifecycle_messages() {
    let messages = [
        IpcMessage::ModelLoadRequest(ModelLoadRequest {
            path: "models/a.gguf".into(),
            model_id: Some("a".into()),
        }),
        IpcMessage::ModelLoadResponse(ModelLoadResponse::success("a".into(), 7)),
        IpcMessage::ModelLoadResponse(ModelLoadResponse::error("nope")),
        IpcMessage::ModelUnloadRequest(ModelUnloadRequest {
            model_id: "a".into(),
        }),
        IpcMessage::ModelUnloadResponse(ModelUnloadResponse::success("a".into())),
        IpcMessage::ModelUnloadResponse(ModelUnloadResponse::error("a".into(), "nope")),
    ];
    for message in &messages {
        let bytes = encode_message(message).unwrap();
        let decoded = decode_message(&bytes).unwrap();
        // IpcMessage has no PartialEq; compare the serialized forms.
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(message).unwrap()
        );
    }
}
