//! External Rust consumer and V1 IPC compatibility baselines for #112.
//! This is NOT a frozen v1.0.0 public API or installed-product acceptance.

use std::path::Path;
use std::sync::Arc;

use gg_core::engine::{InferenceError, Model};
use gg_core::ipc::protocol::{
    decode_message, encode_message, CapabilitiesResponse, InferenceErrorCode, IpcMessage,
    ProtocolVersion, RequestId, CURRENT_PROTOCOL_VERSION,
};
use gg_core::ipc::protocol_typed::{
    LabelScore, TypedInferenceRequest, TypedInferenceResponse, TypedInput, TypedResult, TypedTask,
};
use gg_core::models::backend_capabilities::{
    compiled_backend_capabilities, CAPABILITY_SCHEMA_VERSION,
};
use gg_core::models::{load_model_dispatch, load_model_dispatch_for_workers};
use gg_core::{Runtime, RuntimeConfig};
use serde_json::{json, Value};

type Dispatch = fn(&Path, &str) -> Result<Arc<dyn Model>, InferenceError>;
type WorkerDispatch = fn(&Path, &str, usize) -> Result<Arc<dyn Model>, InferenceError>;

#[test]
fn embedded_entry_points_bind_as_an_external_crate() {
    let _create: fn(RuntimeConfig) -> Runtime = Runtime::new;
    let _old: Dispatch = load_model_dispatch;
    let _workers: WorkerDispatch = load_model_dispatch_for_workers;
    let config = RuntimeConfig {
        auth_token: "test-token".into(),
        ..RuntimeConfig::default()
    };
    assert_eq!(config.auth_token, "test-token");
}

#[test]
fn old_v1_handshake_defaults_still_decode() {
    assert_eq!(CURRENT_PROTOCOL_VERSION, ProtocolVersion::V1);
    let legacy = br#"{"type":"handshake","token":"test-token"}"#;
    match decode_message(legacy).expect("decode old handshake") {
        IpcMessage::Handshake {
            token,
            protocol_version,
        } => {
            assert_eq!(token, "test-token");
            assert_eq!(protocol_version, None);
        }
        other => panic!("unexpected frame: {other:?}"),
    }
    let ack = br#"{"type":"handshake_ack","session_id":"legacy"}"#;
    match decode_message(ack).expect("decode old handshake acknowledgement") {
        IpcMessage::HandshakeAck {
            session_id,
            protocol_version,
            features,
        } => {
            assert_eq!(session_id, "legacy");
            assert_eq!(protocol_version, ProtocolVersion::V1);
            assert!(features.is_empty());
        }
        other => panic!("unexpected frame: {other:?}"),
    }
}

#[test]
fn additive_capability_frames_have_stable_wire_tags() {
    let request = encode_message(&IpcMessage::CapabilitiesRequest).expect("encode request");
    assert_eq!(request.as_slice(), br#"{"type":"capabilities_request"}"#);
    assert!(matches!(
        decode_message(&request).expect("decode request"),
        IpcMessage::CapabilitiesRequest
    ));

    let snapshot = json!({"schema_version": CAPABILITY_SCHEMA_VERSION, "qualified": []});
    let message = IpcMessage::CapabilitiesResponse(CapabilitiesResponse::new(snapshot.clone()));
    let bytes = encode_message(&message).expect("encode response");
    let value: Value = serde_json::from_slice(&bytes).expect("parse response");
    assert_eq!(
        value,
        json!({"type": "capabilities_response", "snapshot": snapshot})
    );
    match decode_message(&bytes).expect("decode response") {
        IpcMessage::CapabilitiesResponse(response) => assert_eq!(response.snapshot, snapshot),
        other => panic!("unexpected frame: {other:?}"),
    }
}

#[test]
fn typed_embedding_request_retains_its_wire_shape() {
    let message = IpcMessage::TypedInferenceRequest(TypedInferenceRequest {
        request_id: RequestId(17),
        model_id: "embed-model".into(),
        task: TypedTask::Embedding,
        input: TypedInput::Single("example".into()),
        timeout_ms: None,
    });
    let bytes = encode_message(&message).expect("encode typed request");
    let value: Value = serde_json::from_slice(&bytes).expect("parse typed request");
    assert_eq!(
        value,
        json!({
            "type": "typed_inference_request",
            "request_id": 17,
            "model_id": "embed-model",
            "task": "embedding",
            "input": {"text": "example"}
        })
    );
    match decode_message(&bytes).expect("decode typed request") {
        IpcMessage::TypedInferenceRequest(request) => {
            assert_eq!(request.request_id, RequestId(17));
            assert_eq!(request.task, TypedTask::Embedding);
            assert_eq!(request.input, TypedInput::Single("example".into()));
        }
        other => panic!("unexpected frame: {other:?}"),
    }
}

// #114: Exercise the public client-facing *external crate* wire contract.
// This does not assert that an installed ONNX classifier has been qualified.
#[test]
fn typed_classification_batch_request_preserves_public_wire_shape() {
    let request = IpcMessage::TypedInferenceRequest(TypedInferenceRequest {
        request_id: RequestId(23),
        model_id: "classifier".into(),
        task: TypedTask::TextClassification,
        input: TypedInput::Batch(vec!["first".into(), "second".into()]),
        timeout_ms: Some(1_500),
    });
    let bytes = encode_message(&request).expect("encode classification request");
    let wire: Value = serde_json::from_slice(&bytes).expect("decode wire JSON");
    assert_eq!(
        wire,
        json!({
            "type": "typed_inference_request",
            "request_id": 23,
            "model_id": "classifier",
            "task": "text_classification",
            "input": {"texts": ["first", "second"]},
            "timeout_ms": 1500
        })
    );
    match decode_message(&bytes).expect("decode classification request") {
        IpcMessage::TypedInferenceRequest(decoded) => {
            assert_eq!(decoded.request_id, RequestId(23));
            assert_eq!(decoded.task, TypedTask::TextClassification);
            assert_eq!(
                decoded.input,
                TypedInput::Batch(vec!["first".into(), "second".into()])
            );
            assert_eq!(decoded.timeout_ms, Some(1_500));
        }
        other => panic!("unexpected classification frame: {other:?}"),
    }
}

#[test]
fn typed_classification_success_and_unsupported_task_refusal_remain_distinct() {
    let result = TypedResult::Classification {
        label: "positive".into(),
        confidence: 0.75,
        all_labels: vec![LabelScore {
            label: "positive".into(),
            confidence: 0.75,
        }],
    };
    let success = IpcMessage::TypedInferenceResponse(TypedInferenceResponse::success(
        RequestId(23),
        result.clone(),
    ));
    let success_bytes = encode_message(&success).expect("encode classification success");
    let success_wire: Value = serde_json::from_slice(&success_bytes).unwrap();
    assert_eq!(
        success_wire,
        json!({
            "type": "typed_inference_response",
            "request_id": 23,
            "result": {
                "kind": "classification",
                "label": "positive",
                "confidence": 0.75,
                "all_labels": [{"label": "positive", "confidence": 0.75}]
            }
        })
    );
    match decode_message(&success_bytes).expect("decode classification success") {
        IpcMessage::TypedInferenceResponse(decoded) => {
            assert_eq!(decoded.request_id, RequestId(23));
            assert_eq!(decoded.result, Some(result));
            assert!(decoded.error.is_none());
            assert!(decoded.error_code.is_none());
        }
        other => panic!("unexpected classification result: {other:?}"),
    }

    let refusal = IpcMessage::TypedInferenceResponse(TypedInferenceResponse::error(
        RequestId(24),
        "unsupported task".into(),
        InferenceErrorCode::UnsupportedTask,
    ));
    let refusal_bytes = encode_message(&refusal).expect("encode typed refusal");
    let refusal_wire: Value = serde_json::from_slice(&refusal_bytes).unwrap();
    assert_eq!(
        refusal_wire,
        json!({
            "type": "typed_inference_response",
            "request_id": 24,
            "error": "unsupported task",
            "error_code": "unsupported_task"
        })
    );
    match decode_message(&refusal_bytes).expect("decode typed refusal") {
        IpcMessage::TypedInferenceResponse(decoded) => {
            assert_eq!(decoded.request_id, RequestId(24));
            assert!(decoded.result.is_none());
            assert_eq!(
                decoded.error_code,
                Some(InferenceErrorCode::UnsupportedTask)
            );
        }
        other => panic!("unexpected typed refusal: {other:?}"),
    }
}

#[test]
fn compiled_capabilities_do_not_claim_installed_qualification() {
    for entry in compiled_backend_capabilities() {
        let object = serde_json::to_value(entry).expect("serialize public capability");
        assert_eq!(object["schema_version"], json!(CAPABILITY_SCHEMA_VERSION));
        assert_eq!(object["qualified_paths"], json!([]));
        assert!(object["compiled_paths"].is_array());
    }
}
