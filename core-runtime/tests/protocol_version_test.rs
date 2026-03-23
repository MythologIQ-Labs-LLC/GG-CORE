//! Tests for protocol version negotiation.

use gg_core::ipc::{
    decode_message, encode_message, IpcMessage, ProtocolVersion,
};

#[test]
fn handshake_v1_default_when_not_specified() {
    // Legacy client sends handshake without protocol_version
    let legacy_json = r#"{"type":"handshake","token":"secret123"}"#;
    let message = decode_message(legacy_json.as_bytes()).unwrap();

    match message {
        IpcMessage::Handshake { token, protocol_version } => {
            assert_eq!(token, "secret123");
            assert_eq!(protocol_version, None);
        }
        _ => panic!("Expected Handshake message"),
    }
}

#[test]
fn handshake_v1_explicit() {
    let message = IpcMessage::Handshake {
        token: "secret123".to_string(),
        protocol_version: Some(ProtocolVersion::V1),
    };

    let encoded = encode_message(&message).unwrap();
    let decoded = decode_message(&encoded).unwrap();

    match decoded {
        IpcMessage::Handshake { token, protocol_version } => {
            assert_eq!(token, "secret123");
            assert_eq!(protocol_version, Some(ProtocolVersion::V1));
        }
        _ => panic!("Expected Handshake message"),
    }
}

#[test]
fn handshake_v2_request() {
    let message = IpcMessage::Handshake {
        token: "secret123".to_string(),
        protocol_version: Some(ProtocolVersion::V2),
    };

    let encoded = encode_message(&message).unwrap();
    let decoded = decode_message(&encoded).unwrap();

    match decoded {
        IpcMessage::Handshake { token, protocol_version } => {
            assert_eq!(token, "secret123");
            assert_eq!(protocol_version, Some(ProtocolVersion::V2));
        }
        _ => panic!("Expected Handshake message"),
    }
}

#[test]
fn handshake_ack_includes_negotiated_version() {
    let message = IpcMessage::HandshakeAck {
        session_id: "session-abc".to_string(),
        protocol_version: ProtocolVersion::V1,
    };

    let encoded = encode_message(&message).unwrap();
    let decoded = decode_message(&encoded).unwrap();

    match decoded {
        IpcMessage::HandshakeAck { session_id, protocol_version } => {
            assert_eq!(session_id, "session-abc");
            assert_eq!(protocol_version, ProtocolVersion::V1);
        }
        _ => panic!("Expected HandshakeAck message"),
    }
}

#[test]
fn handshake_ack_v2_negotiated() {
    let message = IpcMessage::HandshakeAck {
        session_id: "session-xyz".to_string(),
        protocol_version: ProtocolVersion::V2,
    };

    let encoded = encode_message(&message).unwrap();
    let decoded = decode_message(&encoded).unwrap();

    match decoded {
        IpcMessage::HandshakeAck { session_id, protocol_version } => {
            assert_eq!(session_id, "session-xyz");
            assert_eq!(protocol_version, ProtocolVersion::V2);
        }
        _ => panic!("Expected HandshakeAck message"),
    }
}

#[test]
fn legacy_handshake_ack_defaults_to_v1() {
    // Legacy server sends handshake_ack without protocol_version
    let legacy_json = r#"{"type":"handshake_ack","session_id":"session-old"}"#;
    let message = decode_message(legacy_json.as_bytes()).unwrap();

    match message {
        IpcMessage::HandshakeAck { session_id, protocol_version } => {
            assert_eq!(session_id, "session-old");
            assert_eq!(protocol_version, ProtocolVersion::V1); // Default
        }
        _ => panic!("Expected HandshakeAck message"),
    }
}

#[test]
fn protocol_version_default_is_v1() {
    assert_eq!(ProtocolVersion::default(), ProtocolVersion::V1);
}
