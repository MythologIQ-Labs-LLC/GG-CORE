//! Model-free argument and capability boundary negatives.
use super::*;
use crate::ipc::protocol::encode_message;
use crate::ipc::protocol_capability_types::CapabilitiesResponse;
use serde_json::json;

#[test]
fn strict_argument_parser_distinguishes_default_and_all() {
    let empty: Vec<String> = vec![];
    let default = parse_options(&empty).unwrap();
    assert!(!default.all);
    assert!(!default.socket.is_empty());
    let provided = vec![
        "--all".to_string(),
        "--socket".to_string(),
        "/private/run/gg.sock".to_string(),
    ];
    assert_eq!(
        parse_options(&provided).unwrap(),
        VerifyOptions {
            socket: "/private/run/gg.sock".into(),
            all: true,
        }
    );
}

#[test]
fn missing_duplicate_and_unknown_arguments_fail_closed() {
    for args in [
        vec!["--socket"],
        vec!["--socket", ""],
        vec!["--socket", "--all"],
        vec!["--all", "--all"],
        vec!["--socket", "/tmp/a.sock", "--socket", "/tmp/b.sock"],
        vec!["--json"],
    ] {
        assert!(
            parse_options(&args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).is_err(),
            "{args:?}"
        );
    }
}

fn snapshot(auth: bool, offline: bool, network: bool) -> serde_json::Value {
    json!({
        "schema_version": CAPABILITY_SCHEMA_VERSION,
        "operator": {
            "auth_required": auth,
            "offline_only": offline,
            "network_listener": network
        }
    })
}

fn encode(snapshot: serde_json::Value) -> Vec<u8> {
    encode_message(&IpcMessage::CapabilitiesResponse(
        CapabilitiesResponse::new(snapshot),
    ))
    .unwrap()
}

#[test]
fn valid_authenticated_offline_capabilities_are_accepted() {
    assert!(validate_capabilities(&encode(snapshot(true, true, false))).is_ok());
}

#[test]
fn unauthenticated_network_and_invalid_schema_are_refused() {
    for bad in [
        snapshot(false, true, false),
        snapshot(true, false, false),
        snapshot(true, true, true),
        json!({"schema_version": CAPABILITY_SCHEMA_VERSION}),
        json!({"schema_version": CAPABILITY_SCHEMA_VERSION, "operator": {}}),
        json!({"schema_version": 999, "operator": {"auth_required": true, "offline_only": true, "network_listener": false}}),
        json!({}),
    ] {
        assert!(validate_capabilities(&encode(bad)).is_err());
    }
}

#[test]
fn unexpected_and_invalid_response_are_refused() {
    assert!(validate_capabilities(&encode_message(&IpcMessage::CapabilitiesRequest).unwrap()).is_err());
    assert!(validate_capabilities(b"not-json").is_err());
}
