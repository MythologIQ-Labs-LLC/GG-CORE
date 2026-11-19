//! AES-256-GCM fixed-output startup known-answer self-test.
//!
//! Known answer: AES-256-GCM zero key, 96-bit zero IV, 16 zero plaintext
//! bytes and no AAD, sourced from the NIST GCM test vectors (also present
//! in OpenSSL/BoringSSL vector suites). This is NOT CMVP validation.
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use super::SelfTestError;

// Expected output is ciphertext followed by the 128-bit authentication tag.
// It must never be derived from the current crypto implementation at runtime.
const EXPECTED: [u8; 32] = [
    0xce, 0xa7, 0x40, 0x3d, 0x4d, 0x60, 0x6b, 0x6e,
    0x07, 0x4e, 0xc5, 0xd3, 0xba, 0xf3, 0x9d, 0x18,
    0xd0, 0xd1, 0xc8, 0xa7, 0x99, 0x99, 0x6b, 0xf0,
    0x26, 0x5b, 0x98, 0xb5, 0xd4, 0x8a, 0xb9, 0x19,
];

pub fn aes_gcm_kat() -> Result<(), SelfTestError> {
    run_fixed_answer(&EXPECTED)
}

fn run_fixed_answer(expected: &[u8]) -> Result<(), SelfTestError> {
    let key = aes_gcm::Key::<Aes256Gcm>::from([0u8; 32]);
    let nonce = Nonce::from([0u8; 12]);
    let plaintext = [0u8; 16];
    let cipher = Aes256Gcm::new(&key);
    let actual = cipher.encrypt(&nonce, plaintext.as_ref())
        .map_err(|_| SelfTestError::AesGcmEncryptFailed)?;
    if actual.as_slice() != expected {
        return Err(SelfTestError::KatMismatch {
            expected: expected.to_vec(), actual,
        });
    }
    let decrypted = cipher.decrypt(&nonce, actual.as_slice())
        .map_err(|_| SelfTestError::AesGcmDecryptFailed)?;
    if decrypted.as_slice() != plaintext {
        return Err(SelfTestError::AesGcmDecryptFailed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_nist_vector_matches() {
        assert!(aes_gcm_kat().is_ok());
    }
    #[test]
    fn corrupted_reference_must_fail_even_if_round_trip_works() {
        let mut corrupted = EXPECTED;
        corrupted[0] ^= 1;
        assert!(matches!(run_fixed_answer(&corrupted), Err(SelfTestError::KatMismatch { .. })));
    }
}
