//! External Rust consumer and V1 IPC compatibility baselines for #112.
//! This is NOT a frozen v1.0.0 public API or installed-product acceptance.

use std::path::Path;
use std::sync::Arc;

use gg_core::cli::status::RequestStats;

use gg_core::engine::{InferenceError, Model};
use gg_core::ipc::protocol::{
    decode_message, encode_message, CapabilitiesResponse, IpcMessage, ProtocolVersion, RequestId,
    CURRENT_PROTOCOL_VERSION,
};
use gg_core::ipc::protocol_typed::{TypedInferenceRequest, TypedInput, TypedTask};
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

#[test]
fn compiled_capabilities_do_not_claim_installed_qualification() {
    for entry in compiled_backend_capabilities() {
        let object = serde_json::to_value(entry).expect("serialize public capability");
        assert_eq!(object["schema_version"], json!(CAPABILITY_SCHEMA_VERSION));
        assert_eq!(object["qualified_paths"], json!([]));
        assert!(object["compiled_paths"].is_array());
    }
}

#[test]
fn pre_v1_status_quantiles_have_an_explicit_external_consumer_break() {
    // Bind the actual public Rust source type as a downstream crate does.
    // The CLI status producer currently has no measured percentile samples.
    let report = RequestStats {
        total_requests: 10,
        successful_requests: 10,
        failed_requests: 0,
        requests_per_second: 1.0,
        avg_latency_ms: 42.0,
        p50_latency_ms: None,
        p95_latency_ms: None,
        p99_latency_ms: None,
        tokens_generated: 200,
        tokens_per_second: 20.0,
    };
    let _: Option<f64> = report.p50_latency_ms;
    let _: Option<f64> = report.p95_latency_ms;
    let _: Option<f64> = report.p99_latency_ms;

    let json = serde_json::to_value(&report).expect("public status serialize");
    for key in ["p50_latency_ms", "p95_latency_ms", "p99_latency_ms"] {
        assert!(
            json[key].is_null(),
            "unknown {key} must be null, not a fabricated numeric zero"
        );
        // Older JSON consumers requiring a non-null numeric f64 cannot
        // deserialize this field. This is an intentional pre-v1 break.
        assert!(
            serde_json::from_value::<f64>(json[key].clone()).is_err(),
            "strict legacy numeric consumer unexpectedly accepted {key}"
        );
    }

    // The updated public consumer can decode null safely and does not
    // reinterpret absence as an observed zero or an invented percentile.
    let adapted: RequestStats = serde_json::from_value(json).expect("nullable consumer");
    assert!(adapted.p50_latency_ms.is_none());
    assert!(adapted.p95_latency_ms.is_none());
    assert!(adapted.p99_latency_ms.is_none());
    assert_eq!(adapted.avg_latency_ms, 42.0);

    // Older well-formed numeric JSON remains readable by the new optional
    // decoder. Read compatibility does not imply old consumers accept null.
    let legacy_numeric = serde_json::json!({
        "total_requests": 10,
        "successful_requests": 10,
        "failed_requests": 0,
        "requests_per_second": 1.0,
        "avg_latency_ms": 42.0,
        "p50_latency_ms": 20.0,
        "p95_latency_ms": 70.0,
        "p99_latency_ms": 90.0,
        "tokens_generated": 200,
        "tokens_per_second": 20.0
    });
    let decoded: RequestStats =
        serde_json::from_value(legacy_numeric).expect("legacy numeric shape");
    assert_eq!(decoded.p50_latency_ms, Some(20.0));
    assert_eq!(decoded.p95_latency_ms, Some(70.0));
    assert_eq!(decoded.p99_latency_ms, Some(90.0));
}
