//! Tests for model warm-up functionality.

use gg_core::ipc::{
    decode_message, encode_message, IpcMessage, WarmupRequest, WarmupResponse,
};

#[test]
fn test_warmup_request_defaults() {
    // Deserialize with defaults
    let json = r#"{"type":"warmup_request","model_id":"test-model"}"#;
    let msg: IpcMessage = serde_json::from_str(json).unwrap();

    match msg {
        IpcMessage::WarmupRequest(req) => {
            assert_eq!(req.model_id, "test-model");
            assert_eq!(req.tokens, 1); // Default value
        }
        _ => panic!("Expected WarmupRequest"),
    }
}

#[test]
fn test_warmup_request_with_tokens() {
    let json = r#"{"type":"warmup_request","model_id":"test-model","tokens":5}"#;
    let msg: IpcMessage = serde_json::from_str(json).unwrap();

    match msg {
        IpcMessage::WarmupRequest(req) => {
            assert_eq!(req.model_id, "test-model");
            assert_eq!(req.tokens, 5);
        }
        _ => panic!("Expected WarmupRequest"),
    }
}

#[test]
fn test_warmup_request_roundtrip() {
    let request = WarmupRequest {
        model_id: "phi-3-mini".to_string(),
        tokens: 3,
    };
    let msg = IpcMessage::WarmupRequest(request);

    let bytes = encode_message(&msg).unwrap();
    let decoded = decode_message(&bytes).unwrap();

    match decoded {
        IpcMessage::WarmupRequest(req) => {
            assert_eq!(req.model_id, "phi-3-mini");
            assert_eq!(req.tokens, 3);
        }
        _ => panic!("Expected WarmupRequest"),
    }
}

#[test]
fn test_warmup_response_success() {
    let response = WarmupResponse::success("test-model".to_string(), 42);

    assert_eq!(response.model_id, "test-model");
    assert!(response.success);
    assert!(response.error.is_none());
    assert_eq!(response.elapsed_ms, 42);
}

#[test]
fn test_warmup_response_error() {
    let response = WarmupResponse::error(
        "test-model".to_string(),
        "Model not found".to_string(),
        10,
    );

    assert_eq!(response.model_id, "test-model");
    assert!(!response.success);
    assert_eq!(response.error, Some("Model not found".to_string()));
    assert_eq!(response.elapsed_ms, 10);
}

#[test]
fn test_warmup_response_roundtrip() {
    let response = WarmupResponse::success("gpt-mini".to_string(), 100);
    let msg = IpcMessage::WarmupResponse(response);

    let bytes = encode_message(&msg).unwrap();
    let decoded = decode_message(&bytes).unwrap();

    match decoded {
        IpcMessage::WarmupResponse(resp) => {
            assert_eq!(resp.model_id, "gpt-mini");
            assert!(resp.success);
            assert_eq!(resp.elapsed_ms, 100);
        }
        _ => panic!("Expected WarmupResponse"),
    }
}
