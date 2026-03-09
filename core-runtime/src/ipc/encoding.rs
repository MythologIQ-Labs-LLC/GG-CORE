//! Token encoding strategies for IPC protocol versioning.

use super::protocol::ProtocolError;

/// Trait for encoding/decoding token sequences.
pub trait TokenEncoder {
    /// Encode tokens to bytes.
    fn encode(&self, tokens: &[u32]) -> Vec<u8>;

    /// Decode bytes back to tokens.
    fn decode(&self, bytes: &[u8]) -> Result<Vec<u32>, ProtocolError>;
}

/// V1 Encoder: JSON serialization of token arrays.
/// This is the default encoding for backward compatibility.
#[derive(Debug, Clone, Copy, Default)]
pub struct V1Encoder;

impl TokenEncoder for V1Encoder {
    fn encode(&self, tokens: &[u32]) -> Vec<u8> {
        serde_json::to_vec(tokens).unwrap_or_default()
    }

    fn decode(&self, bytes: &[u8]) -> Result<Vec<u32>, ProtocolError> {
        serde_json::from_slice(bytes).map_err(|e| ProtocolError::InvalidFormat(e.to_string()))
    }
}

/// V2 Encoder: Packed binary format for token arrays.
/// Format: [count: u32-le][token0: u32-le][token1: u32-le]...
/// ~50% smaller than JSON for typical payloads.
#[derive(Debug, Clone, Copy, Default)]
pub struct V2Encoder;

impl TokenEncoder for V2Encoder {
    fn encode(&self, tokens: &[u32]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + tokens.len() * 4);
        buf.extend_from_slice(&(tokens.len() as u32).to_le_bytes());
        for token in tokens {
            buf.extend_from_slice(&token.to_le_bytes());
        }
        buf
    }

    fn decode(&self, bytes: &[u8]) -> Result<Vec<u32>, ProtocolError> {
        if bytes.len() < 4 {
            return Err(ProtocolError::InvalidFormat("V2: too short".into()));
        }
        let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let expected_len = 4 + count * 4;
        if bytes.len() != expected_len {
            return Err(ProtocolError::InvalidFormat(format!(
                "V2: expected {} bytes, got {}",
                expected_len,
                bytes.len()
            )));
        }
        let mut tokens = Vec::with_capacity(count);
        for i in 0..count {
            let offset = 4 + i * 4;
            let token = u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            tokens.push(token);
        }
        Ok(tokens)
    }
}

/// Get encoder for a given protocol version.
pub fn get_encoder(
    version: super::protocol::ProtocolVersion,
) -> Box<dyn TokenEncoder + Send + Sync> {
    match version {
        super::protocol::ProtocolVersion::V1 => Box::new(V1Encoder),
        super::protocol::ProtocolVersion::V2 => Box::new(V2Encoder),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_encode_empty() {
        let encoder = V1Encoder;
        let encoded = encoder.encode(&[]);
        assert_eq!(encoded, b"[]");
    }

    #[test]
    fn v1_encode_single() {
        let encoder = V1Encoder;
        let encoded = encoder.encode(&[42]);
        assert_eq!(encoded, b"[42]");
    }

    #[test]
    fn v1_roundtrip() {
        let encoder = V1Encoder;
        let tokens = vec![1, 2, 3, 100, 1000, 65535];
        let encoded = encoder.encode(&tokens);
        let decoded = encoder.decode(&encoded).unwrap();
        assert_eq!(tokens, decoded);
    }

    #[test]
    fn v1_decode_invalid() {
        let encoder = V1Encoder;
        let result = encoder.decode(b"not json");
        assert!(result.is_err());
    }
}
