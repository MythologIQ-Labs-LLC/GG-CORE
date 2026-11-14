//! Wire shape of the release-wait pair (MODEL_RELEASE_V1_PLAN A8).

use super::*;
use crate::ipc::protocol::{decode_message, encode_message, IpcMessage, ModelUnloadRequest};

#[test]
fn request_and_response_round_trip_as_json() {
    let req = IpcMessage::ModelReleaseWaitRequest(ModelReleaseWaitRequest::new("m", 2_500));
    let json = String::from_utf8(encode_message(&req).unwrap()).unwrap();
    assert!(
        json.contains(r#""type":"model_release_wait_request""#),
        "{json}"
    );
    assert!(json.contains(r#""wait_ms":2500"#), "{json}");
    match decode_message(json.as_bytes()).unwrap() {
        IpcMessage::ModelReleaseWaitRequest(r) => {
            assert_eq!(r, ModelReleaseWaitRequest::new("m", 2_500))
        }
        other => panic!("{other:?}"),
    }
    let resp = IpcMessage::ModelReleaseWaitResponse(ModelReleaseWaitResponse::new("m", false, 2));
    let json = String::from_utf8(encode_message(&resp).unwrap()).unwrap();
    assert!(json.contains(r#""released":false"#) && json.contains(r#""pending_instances":2"#));
}

#[test]
fn wait_defaults_to_zero_and_is_clamped() {
    let raw = br#"{"type":"model_release_wait_request","model_id":"m"}"#;
    let IpcMessage::ModelReleaseWaitRequest(r) = decode_message(raw).unwrap() else {
        panic!("wrong variant");
    };
    assert_eq!(r.clamped_wait(), std::time::Duration::ZERO);
    let long = ModelReleaseWaitRequest::new("m", u64::MAX);
    assert_eq!(long.clamped_wait().as_millis() as u64, MAX_RELEASE_WAIT_MS);
}

#[test]
fn the_existing_unload_messages_are_unchanged() {
    let raw = br#"{"type":"model_unload_request","model_id":"m"}"#;
    let IpcMessage::ModelUnloadRequest(r) = decode_message(raw).unwrap() else {
        panic!("wrong variant");
    };
    assert_eq!(
        r,
        ModelUnloadRequest {
            model_id: "m".into()
        }
    );
    let resp = br#"{"type":"model_unload_response","success":true,"model_id":"m"}"#;
    assert!(
        matches!(decode_message(resp).unwrap(), IpcMessage::ModelUnloadResponse(r) if r.success)
    );
}
