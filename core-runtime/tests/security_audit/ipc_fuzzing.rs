//! IPC Protocol Fuzzing Tests
//!
//! Tests for malformed messages, boundary conditions, and protocol violations.

use gg_core::engine::InferenceParams;
use gg_core::ipc::protocol::{
    decode_message, decode_message_binary, encode_message, encode_message_binary, HealthCheckType,
    InferenceRequest, IpcMessage, ProtocolError, RequestId,
};

/// Reject message exceeding size limit.
#[test]
fn reject_oversized_message() {
    let large_data = vec![0u8; 16 * 1024 * 1024 + 1];
    let result = decode_message(&large_data);
    assert!(matches!(result, Err(ProtocolError::MessageTooLarge { .. })));
}

/// Reject oversized binary message.
#[test]
fn reject_oversized_binary_message() {
    let large_data = vec![0u8; 16 * 1024 * 1024 + 1];
    let result = decode_message_binary(&large_data);
    assert!(matches!(result, Err(ProtocolError::MessageTooLarge { .. })));
}

/// Empty message rejected.
#[test]
fn reject_empty_message() {
    let empty: &[u8] = &[];
    let result = decode_message(empty);
    assert!(result.is_err());
}

/// Invalid JSON rejected.
#[test]
fn reject_invalid_json() {
    let invalid = b"{ not valid json }}}";
    let result = decode_message(invalid);
    assert!(result.is_err());
}

/// Missing required type field rejected.
#[test]
fn reject_missing_type_field() {
    let no_type = br#"{ "foo": "bar" }"#;
    let result = decode_message(no_type);
    assert!(result.is_err());
}

/// Unknown message type rejected.
#[test]
fn reject_unknown_message_type() {
    let unknown = br#"{ "type": "unknown_attack" }"#;
    let result = decode_message(unknown);
    assert!(result.is_err());
}

/// Null bytes in JSON handled safely.
#[test]
fn handle_null_bytes_in_json() {
    let null_bytes = b"{ \"type\": \"error\"\x00, \"code\": 1 }";
    let result = decode_message(null_bytes);
    assert!(result.is_err());
}

/// UTF-8 BOM handled correctly.
#[test]
fn handle_utf8_bom() {
    let with_bom = b"\xef\xbb\xbf{\"type\":\"health_check\",\"check_type\":\"Liveness\"}";
    let result = decode_message(with_bom);
    let _ = result;
}

/// Deeply nested JSON handled safely.
#[test]
fn handle_deeply_nested_json() {
    let mut nested = String::from("{\"type\":\"error\",\"code\":1,\"message\":\"test\",\"data\":");
    for _ in 0..100 {
        nested.push_str("{\"nested\":");
    }
    nested.push_str("null");
    for _ in 0..100 {
        nested.push('}');
    }
    nested.push('}');
    let result = decode_message(nested.as_bytes());
    let _ = result;
}

/// Inference request requires model_id.
#[test]
fn inference_request_requires_model_id() {
    let request = InferenceRequest {
        request_id: RequestId(1),
        model_id: String::new(),
        prompt: "test prompt".to_string(),
        parameters: InferenceParams::default(),
    };
    let result = request.validate();
    assert!(result.is_err());
}

/// Inference request requires non-empty prompt.
#[test]
fn inference_request_requires_prompt() {
    let request = InferenceRequest {
        request_id: RequestId(1),
        model_id: "test-model".to_string(),
        prompt: String::new(),
        parameters: InferenceParams::default(),
    };
    let result = request.validate();
    assert!(result.is_err());
}

/// Valid inference request passes validation.
#[test]
fn valid_inference_request_passes() {
    let request = InferenceRequest {
        request_id: RequestId(1),
        model_id: "test-model".to_string(),
        prompt: "Hello, world!".to_string(),
        parameters: InferenceParams::default(),
    };
    let result = request.validate();
    assert!(result.is_ok());
}

/// Message roundtrip JSON encoding.
#[test]
fn message_roundtrip_json() {
    let msg = IpcMessage::HealthCheck {
        check_type: HealthCheckType::Liveness,
    };
    let encoded = encode_message(&msg).unwrap();
    let decoded = decode_message(&encoded).unwrap();
    assert!(matches!(
        decoded,
        IpcMessage::HealthCheck {
            check_type: HealthCheckType::Liveness
        }
    ));
}

/// Message roundtrip binary encoding.
#[test]
fn message_roundtrip_binary() {
    let msg = IpcMessage::HealthCheck {
        check_type: HealthCheckType::Readiness,
    };
    let encoded = encode_message_binary(&msg).unwrap();
    let decoded = decode_message_binary(&encoded).unwrap();
    assert!(matches!(
        decoded,
        IpcMessage::HealthCheck {
            check_type: HealthCheckType::Readiness
        }
    ));
}

/// Handshake message encodes correctly.
#[test]
fn handshake_message_encoding() {
    let msg = IpcMessage::Handshake {
        token: "test-token".to_string(),
        protocol_version: None,
    };
    let encoded = encode_message(&msg).unwrap();
    let decoded = decode_message(&encoded).unwrap();
    assert!(matches!(decoded, IpcMessage::Handshake { token, .. } if token == "test-token"));
}

/// Large prompt handled safely.
#[test]
fn large_prompt() {
    let large_prompt = "x".repeat(50000);
    let request = InferenceRequest {
        request_id: RequestId(1),
        model_id: "test".to_string(),
        prompt: large_prompt.clone(),
        parameters: InferenceParams::default(),
    };
    let msg = IpcMessage::InferenceRequest(request);
    let encoded = encode_message(&msg).unwrap();
    let decoded = decode_message(&encoded).unwrap();
    if let IpcMessage::InferenceRequest(req) = decoded {
        assert_eq!(req.prompt.len(), 50000);
    } else {
        panic!("Wrong message type");
    }
}

/// Error message with long string handled.
#[test]
fn error_message_long_string() {
    let long_error = "x".repeat(100000);
    let msg = IpcMessage::Error {
        code: 500,
        message: long_error.clone(),
    };
    let encoded = encode_message(&msg).unwrap();
    let decoded = decode_message(&encoded).unwrap();
    if let IpcMessage::Error { message, .. } = decoded {
        assert_eq!(message, long_error);
    } else {
        panic!("Wrong message type");
    }
}

/// Unicode in model ID handled safely.
#[test]
fn unicode_model_id() {
    let request = InferenceRequest {
        request_id: RequestId(1),
        model_id: "test-model-\u{4e2d}\u{6587}".to_string(),
        prompt: "test prompt".to_string(),
        parameters: InferenceParams::default(),
    };
    let msg = IpcMessage::InferenceRequest(request);
    let encoded = encode_message(&msg);
    assert!(encoded.is_ok());
}

/// Control characters in strings handled.
#[test]
fn control_characters_in_strings() {
    let msg = IpcMessage::Error {
        code: 400,
        message: "Error with\x00\x01\x02\x03tabs\tand\nnewlines".to_string(),
    };
    let encoded = encode_message(&msg);
    assert!(encoded.is_ok());
}

/// Protocol error display messages.
#[test]
fn protocol_error_display() {
    let err = ProtocolError::MessageTooLarge { size: 100, max: 50 };
    let msg = err.to_string();
    assert!(msg.contains("100") && msg.contains("50"));
    let err = ProtocolError::MissingField("test".to_string());
    assert!(err.to_string().contains("test"));
}

/// Negative request ID handled.
#[test]
fn negative_request_id() {
    let json = br#"{"type":"inference_request","request_id":-1,"model_id":"test","prompt":"hello","parameters":{}}"#;
    let result = decode_message(json);
    let _ = result;
}

/// Very large request ID handled.
#[test]
fn large_request_id() {
    let request = InferenceRequest {
        request_id: RequestId(u64::MAX),
        model_id: "test".to_string(),
        prompt: "test prompt".to_string(),
        parameters: InferenceParams::default(),
    };
    let msg = IpcMessage::InferenceRequest(request);
    let encoded = encode_message(&msg).unwrap();
    let decoded = decode_message(&encoded).unwrap();
    if let IpcMessage::InferenceRequest(req) = decoded {
        assert_eq!(req.request_id.0, u64::MAX);
    } else {
        panic!("Wrong message type");
    }
}

/// Zero request ID is valid.
#[test]
fn zero_request_id() {
    let request = InferenceRequest {
        request_id: RequestId(0),
        model_id: "test".to_string(),
        prompt: "test prompt".to_string(),
        parameters: InferenceParams::default(),
    };
    let msg = IpcMessage::InferenceRequest(request);
    let encoded = encode_message(&msg).unwrap();
    let decoded = decode_message(&encoded).unwrap();
    if let IpcMessage::InferenceRequest(req) = decoded {
        assert_eq!(req.request_id.0, 0);
    } else {
        panic!("Wrong message type");
    }
}
