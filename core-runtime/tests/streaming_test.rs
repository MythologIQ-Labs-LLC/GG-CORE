//! Tests for streaming response functionality.

use gg_core::engine::InferenceParams;
use gg_core::ipc::{
    decode_message, encode_message, InferenceRequest, IpcMessage, RequestId, StreamChunk,
};

// =============================================================================
// Phase 1: Protocol Extension Tests
// =============================================================================

#[test]
fn test_inference_params_stream_defaults_to_false() {
    let params = InferenceParams::default();
    assert!(!params.stream, "stream should default to false");
}

#[test]
fn test_inference_params_stream_serde_default() {
    // Deserialize JSON without stream field - should default to false
    let json = r#"{"max_tokens":100,"temperature":0.5,"top_p":0.9,"top_k":40}"#;
    let params: InferenceParams = serde_json::from_str(json).unwrap();
    assert!(!params.stream);
}

#[test]
fn test_inference_params_stream_serde_explicit() {
    // Deserialize JSON with stream field
    let json = r#"{"max_tokens":100,"temperature":0.5,"top_p":0.9,"top_k":40,"stream":true}"#;
    let params: InferenceParams = serde_json::from_str(json).unwrap();
    assert!(params.stream);
}

#[test]
fn test_stream_chunk_token_constructor() {
    let request_id = RequestId(42);
    let chunk = StreamChunk::token(request_id, 12345);

    assert_eq!(chunk.request_id, request_id);
    assert_eq!(chunk.token, 12345);
    assert!(!chunk.is_final);
    assert!(chunk.error.is_none());
}

#[test]
fn test_stream_chunk_final_token_constructor() {
    let request_id = RequestId(99);
    let chunk = StreamChunk::final_token(request_id, 54321);

    assert_eq!(chunk.request_id, request_id);
    assert_eq!(chunk.token, 54321);
    assert!(chunk.is_final);
    assert!(chunk.error.is_none());
}

#[test]
fn test_stream_chunk_error_constructor() {
    let request_id = RequestId(1);
    let chunk = StreamChunk::error(request_id, "test error".into());

    assert_eq!(chunk.request_id, request_id);
    assert_eq!(chunk.token, 0);
    assert!(chunk.is_final);
    assert_eq!(chunk.error, Some("test error".into()));
}

#[test]
fn test_stream_chunk_roundtrip() {
    let request_id = RequestId(123);
    let chunk = StreamChunk::token(request_id, 999);
    let message = IpcMessage::StreamChunk(chunk.clone());

    let encoded = encode_message(&message).expect("encode should succeed");
    let decoded = decode_message(&encoded).expect("decode should succeed");

    match decoded {
        IpcMessage::StreamChunk(decoded_chunk) => {
            assert_eq!(decoded_chunk.request_id, request_id);
            assert_eq!(decoded_chunk.token, 999);
            assert!(!decoded_chunk.is_final);
            assert!(decoded_chunk.error.is_none());
        }
        _ => panic!("Expected StreamChunk message"),
    }
}

#[test]
fn test_stream_chunk_final_roundtrip() {
    let request_id = RequestId(456);
    let chunk = StreamChunk::final_token(request_id, 888);
    let message = IpcMessage::StreamChunk(chunk);

    let encoded = encode_message(&message).expect("encode should succeed");
    let decoded = decode_message(&encoded).expect("decode should succeed");

    match decoded {
        IpcMessage::StreamChunk(decoded_chunk) => {
            assert_eq!(decoded_chunk.request_id, request_id);
            assert_eq!(decoded_chunk.token, 888);
            assert!(decoded_chunk.is_final);
        }
        _ => panic!("Expected StreamChunk message"),
    }
}

#[test]
fn test_stream_chunk_error_roundtrip() {
    let request_id = RequestId(789);
    let chunk = StreamChunk::error(request_id, "connection lost".into());
    let message = IpcMessage::StreamChunk(chunk);

    let encoded = encode_message(&message).expect("encode should succeed");
    let decoded = decode_message(&encoded).expect("decode should succeed");

    match decoded {
        IpcMessage::StreamChunk(decoded_chunk) => {
            assert_eq!(decoded_chunk.request_id, request_id);
            assert!(decoded_chunk.is_final);
            assert_eq!(decoded_chunk.error, Some("connection lost".into()));
        }
        _ => panic!("Expected StreamChunk message"),
    }
}

#[test]
fn test_inference_request_with_stream_flag() {
    let params = InferenceParams {
        stream: true,
        ..Default::default()
    };

    let request = InferenceRequest {
        request_id: RequestId(100),
        model_id: "test-model".into(),
        prompt: "test prompt for streaming".into(),
        parameters: params,
    };

    let message = IpcMessage::InferenceRequest(request);
    let encoded = encode_message(&message).expect("encode should succeed");
    let decoded = decode_message(&encoded).expect("decode should succeed");

    match decoded {
        IpcMessage::InferenceRequest(req) => {
            assert!(req.parameters.stream);
        }
        _ => panic!("Expected InferenceRequest message"),
    }
}
