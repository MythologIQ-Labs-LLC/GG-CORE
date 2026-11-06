//! Wire-format tests for typed (non-generative) inference messages.

use super::*;
use crate::ipc::protocol::{decode_message, encode_message, IpcMessage};

fn decode(json: &str) -> Result<IpcMessage, String> {
    decode_message(json.as_bytes()).map_err(|e| e.to_string())
}

#[test]
fn single_text_embedding_request_round_trips() {
    let json = r#"{"type":"typed_inference_request","request_id":7,"model_id":"emb",
        "task":"embedding","input":{"text":"hello"},"timeout_ms":500}"#;
    let IpcMessage::TypedInferenceRequest(req) = decode(json).unwrap() else {
        panic!("wrong message type");
    };
    assert_eq!(req.task, TypedTask::Embedding);
    assert_eq!(req.input, TypedInput::Single("hello".into()));
    assert_eq!(req.timeout_ms, Some(500));
    let again = encode_message(&IpcMessage::TypedInferenceRequest(req)).unwrap();
    assert!(String::from_utf8(again)
        .unwrap()
        .contains(r#""input":{"text":"hello"}"#));
}

#[test]
fn batch_classification_shape_decodes() {
    let json = r#"{"type":"typed_inference_request","request_id":8,"model_id":"c",
        "task":"text_classification","input":{"texts":["a","b"]}}"#;
    let IpcMessage::TypedInferenceRequest(req) = decode(json).unwrap() else {
        panic!("wrong message type");
    };
    assert_eq!(req.input, TypedInput::Batch(vec!["a".into(), "b".into()]));
    assert_eq!(
        req.task.capability(),
        InferenceCapability::TextClassification
    );
}

#[test]
fn unknown_task_fails_closed() {
    let json = r#"{"type":"typed_inference_request","request_id":1,"model_id":"m",
        "task":"text_generation","input":{"text":"x"}}"#;
    assert!(decode(json).is_err());
}

#[test]
fn ambiguous_or_missing_input_fails_closed() {
    for input in [
        r#"{"text":"a","texts":["b"]}"#,
        "{}",
        r#"{"text":"a","extra":1}"#,
    ] {
        let json = format!(
            r#"{{"type":"typed_inference_request","request_id":1,"model_id":"m","task":"embedding","input":{input}}}"#
        );
        assert!(decode(&json).is_err(), "must reject input {input}");
    }
}

#[test]
fn unknown_request_field_fails_closed() {
    let json = r#"{"type":"typed_inference_request","request_id":1,"model_id":"m",
        "task":"embedding","input":{"text":"a"},"backend":"onnx-direct"}"#;
    assert!(decode(json).is_err());
}

#[test]
fn results_carry_explicit_kind_tags() {
    let emb = TypedInferenceResponse::success(
        RequestId(3),
        TypedResult::Embedding(Embedding {
            dimensions: 2,
            vector: vec![0.5, -0.25],
        }),
    );
    let json = String::from_utf8(encode_message(&IpcMessage::TypedInferenceResponse(emb)).unwrap())
        .unwrap();
    assert!(json.contains(r#""kind":"embedding""#), "{json}");
    let unknown = r#"{"type":"typed_inference_response","request_id":3,
        "result":{"kind":"generation","text":"x"}}"#;
    assert!(decode(unknown).is_err());
}

#[test]
fn error_response_has_code_and_no_result() {
    let r = TypedInferenceResponse::error(
        RequestId(4),
        "requested task is not supported by selected model".into(),
        InferenceErrorCode::UnsupportedTask,
    );
    let json =
        String::from_utf8(encode_message(&IpcMessage::TypedInferenceResponse(r)).unwrap()).unwrap();
    assert!(
        json.contains(r#""error_code":"unsupported_task""#),
        "{json}"
    );
    assert!(!json.contains("\"result\""), "{json}");
}

#[test]
fn engine_generation_output_is_not_coerced() {
    let gen = InferenceOutput::Generation(crate::engine::GenerationResult {
        text: "x".into(),
        tokens_generated: 1,
        finish_reason: crate::engine::FinishReason::Stop,
    });
    assert!(TypedResult::from_engine(gen).is_err());
}

#[test]
fn handshake_ack_without_features_still_decodes() {
    let old = r#"{"type":"handshake_ack","session_id":"s","protocol_version":"V1"}"#;
    assert!(matches!(
        decode(old).unwrap(),
        IpcMessage::HandshakeAck { features, .. } if features.is_empty()
    ));
}
