//! Wire contract for `inference_request.parameters` (docs/IPC_PROTOCOL_SCHEMA.md):
//! every field is optional and a missing field takes the documented default.
//! A consumer sending only what it cares about must not get a 400
//! "missing field" serialization error.

use gg_core::engine::InferenceParams;

#[test]
fn empty_parameters_take_documented_defaults() {
    let p: InferenceParams = serde_json::from_str("{}").expect("empty object deserializes");
    assert_eq!(p.max_tokens, 256);
    assert_eq!(p.temperature, 0.7);
    assert_eq!(p.top_p, 0.9);
    assert_eq!(p.top_k, 40);
    assert!(!p.stream);
    assert_eq!(p.timeout_ms, None);
}

#[test]
fn partial_parameters_keep_supplied_values() {
    let p: InferenceParams =
        serde_json::from_str(r#"{"max_tokens":4,"stream":false,"timeout_ms":60000}"#)
            .expect("partial object deserializes");
    assert_eq!(p.max_tokens, 4);
    assert_eq!(p.timeout_ms, Some(60_000));
    assert_eq!(p.temperature, 0.7, "unspecified fields default");
    assert!(p.validate().is_ok());
}

#[test]
fn serialization_still_writes_every_field() {
    let json = serde_json::to_string(&InferenceParams::default()).unwrap();
    for key in [
        "max_tokens",
        "temperature",
        "top_p",
        "top_k",
        "stream",
        "timeout_ms",
    ] {
        assert!(
            json.contains(&format!("\"{key}\"")),
            "{key} missing from {json}"
        );
    }
}
