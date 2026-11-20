//! Verify ordinary inference replies belong to the request that was sent.
//! The daemon has a session, but request IDs are still an IPC correlation
//! boundary. Never return another request's output or error as this one's.

use super::CliError;
use crate::ipc::protocol::{decode_message, IpcMessage, RequestId};

pub(super) fn decode_inference_reply(
    bytes: &[u8],
    expected_id: RequestId,
) -> Result<String, CliError> {
    match decode_message(bytes).map_err(|e| CliError::Protocol(e.to_string()))? {
        IpcMessage::InferenceResponse(resp) if resp.request_id != expected_id => Err(
            CliError::Protocol("Inference response request ID mismatch".into()),
        ),
        IpcMessage::InferenceResponse(resp) => match resp.error {
            Some(err) => Err(CliError::Protocol(err)),
            None => Ok(resp.output),
        },
        IpcMessage::Error { message, .. } => Err(CliError::Protocol(message)),
        _ => Err(CliError::Protocol("Unexpected response type".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::{encode_message, InferenceResponse};

    fn encode(response: IpcMessage) -> Vec<u8> {
        encode_message(&response).unwrap()
    }

    #[test]
    fn matching_success_returns_output() {
        let reply = encode(IpcMessage::InferenceResponse(InferenceResponse::success(
            RequestId(7),
            "correct".into(),
            1,
            true,
        )));
        assert_eq!(
            decode_inference_reply(&reply, RequestId(7)).unwrap(),
            "correct"
        );
    }

    #[test]
    fn mismatched_success_never_returns_output() {
        let reply = encode(IpcMessage::InferenceResponse(InferenceResponse::success(
            RequestId(8),
            "wrong user output".into(),
            1,
            true,
        )));
        let err = decode_inference_reply(&reply, RequestId(7)).unwrap_err();
        assert!(matches!(err, CliError::Protocol(msg) if msg.contains("ID mismatch")));
    }

    #[test]
    fn matching_error_remains_failure() {
        let reply = encode(IpcMessage::InferenceResponse(InferenceResponse::error(
            RequestId(7),
            "model unavailable".into(),
        )));
        let err = decode_inference_reply(&reply, RequestId(7)).unwrap_err();
        assert!(matches!(err, CliError::Protocol(msg) if msg == "model unavailable"));
    }

    #[test]
    fn mismatched_error_is_not_attributed_to_current_request() {
        let reply = encode(IpcMessage::InferenceResponse(InferenceResponse::error(
            RequestId(8),
            "different request failed".into(),
        )));
        let err = decode_inference_reply(&reply, RequestId(7)).unwrap_err();
        assert!(matches!(err, CliError::Protocol(msg) if msg.contains("ID mismatch")));
    }

    #[test]
    fn protocol_error_remains_failure() {
        let reply = encode(IpcMessage::Error {
            code: 401,
            message: "refused".into(),
        });
        assert!(decode_inference_reply(&reply, RequestId(7)).is_err());
    }

    #[test]
    fn unexpected_frame_is_refused() {
        let reply = encode(IpcMessage::CapabilitiesRequest);
        assert!(decode_inference_reply(&reply, RequestId(7)).is_err());
    }

    #[test]
    fn malformed_response_is_refused() {
        assert!(decode_inference_reply(b"{bad", RequestId(7)).is_err());
    }
}
