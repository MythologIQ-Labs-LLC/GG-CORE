//! Property-style tests for token encoding roundtrip correctness.

use gg_core::ipc::{get_encoder, ProtocolVersion, TokenEncoder, V1Encoder, V2Encoder};

#[test]
fn v1_roundtrip_empty() {
    let encoder = V1Encoder;
    let tokens: Vec<u32> = vec![];
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn v1_roundtrip_single_token() {
    let encoder = V1Encoder;
    let tokens = vec![42];
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn v1_roundtrip_small_sequence() {
    let encoder = V1Encoder;
    let tokens: Vec<u32> = (1..=100).collect();
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn v1_roundtrip_large_sequence() {
    let encoder = V1Encoder;
    let tokens: Vec<u32> = (0..4000).collect();
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn v1_roundtrip_boundary_values() {
    let encoder = V1Encoder;
    // Test boundary values: 0, 127, 128, 16383, 16384, max u32
    let tokens = vec![0, 127, 128, 16383, 16384, u32::MAX];
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn v1_roundtrip_repeated_values() {
    let encoder = V1Encoder;
    let tokens = vec![42; 1000]; // 1000 repetitions of 42
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn get_encoder_v1_returns_functional_encoder() {
    let encoder = get_encoder(ProtocolVersion::V1);
    let tokens = vec![1, 2, 3, 4, 5];
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn get_encoder_v2_returns_binary_encoder() {
    let encoder = get_encoder(ProtocolVersion::V2);
    let tokens = vec![1, 2, 3, 4, 5];
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
    // V2 should produce binary format (4 bytes header + 4 bytes per token)
    assert_eq!(encoded.len(), 4 + 5 * 4);
}

#[test]
fn v1_decode_invalid_json_returns_error() {
    let encoder = V1Encoder;
    let result = encoder.decode(b"not valid json");
    assert!(result.is_err());
}

#[test]
fn v1_decode_wrong_type_returns_error() {
    let encoder = V1Encoder;
    // Valid JSON but not an array of u32
    let result = encoder.decode(b"\"hello\"");
    assert!(result.is_err());
}

#[test]
fn v1_encoding_is_deterministic() {
    let encoder = V1Encoder;
    let tokens = vec![100, 200, 300];
    let encoded1 = encoder.encode(&tokens);
    let encoded2 = encoder.encode(&tokens);
    assert_eq!(encoded1, encoded2);
}

// V2 Encoder Tests

#[test]
fn v2_encode_empty() {
    let encoder = V2Encoder;
    let encoded = encoder.encode(&[]);
    // Empty array: just count (4 bytes) = 0
    assert_eq!(encoded.len(), 4);
    assert_eq!(&encoded[..4], &[0, 0, 0, 0]);
}

#[test]
fn v2_encode_single() {
    let encoder = V2Encoder;
    let encoded = encoder.encode(&[42]);
    // Single token: 4 bytes count + 4 bytes token = 8 bytes
    assert_eq!(encoded.len(), 8);
    // Count = 1 (little endian)
    assert_eq!(&encoded[..4], &[1, 0, 0, 0]);
    // Token = 42 (little endian)
    assert_eq!(&encoded[4..8], &[42, 0, 0, 0]);
}

#[test]
fn v2_roundtrip() {
    let encoder = V2Encoder;
    let tokens = vec![1, 2, 3, 100, 1000, 65535, u32::MAX];
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
}

#[test]
fn v2_decode_truncated() {
    let encoder = V2Encoder;
    // Less than 4 bytes (no count)
    let result = encoder.decode(&[1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn v2_decode_length_mismatch() {
    let encoder = V2Encoder;
    // Count says 2 tokens (8 bytes), but only 4 bytes of data
    let mut bytes = vec![2, 0, 0, 0]; // count = 2
    bytes.extend_from_slice(&[42, 0, 0, 0]); // only 1 token
    let result = encoder.decode(&bytes);
    assert!(result.is_err());
}

#[test]
fn v2_vs_v1_size_comparison() {
    let v1 = V1Encoder;
    let v2 = V2Encoder;

    // For typical token sequences, V2 should be smaller
    let tokens: Vec<u32> = (1000..1100).collect(); // 100 tokens with 4-digit values
    let v1_encoded = v1.encode(&tokens);
    let v2_encoded = v2.encode(&tokens);

    // V2: 4 + 100*4 = 404 bytes
    // V1: "[1000,1001,...,1099]" = roughly 5 chars per token = ~500 bytes
    assert!(v2_encoded.len() < v1_encoded.len(),
        "V2 ({} bytes) should be smaller than V1 ({} bytes)",
        v2_encoded.len(), v1_encoded.len());
}

#[test]
fn v2_roundtrip_large_sequence() {
    let encoder = V2Encoder;
    let tokens: Vec<u32> = (0..4000).collect();
    let encoded = encoder.encode(&tokens);
    let decoded = encoder.decode(&encoded).unwrap();
    assert_eq!(tokens, decoded);
    // Verify size: 4 + 4000*4 = 16004 bytes
    assert_eq!(encoded.len(), 4 + 4000 * 4);
}

#[test]
fn v2_encoding_is_deterministic() {
    let encoder = V2Encoder;
    let tokens = vec![100, 200, 300];
    let encoded1 = encoder.encode(&tokens);
    let encoded2 = encoder.encode(&tokens);
    assert_eq!(encoded1, encoded2);
}
