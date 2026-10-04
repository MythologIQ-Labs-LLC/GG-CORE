//! Cryptographic Implementation Penetration Tests
//!
//! Tests validating cryptographic correctness and resistance to attacks.

use gg_core::security::encryption::{ModelEncryption, KEY_SIZE, NONCE_SIZE};
use std::collections::HashSet;
use std::time::Instant;

fn create_test_key() -> [u8; KEY_SIZE] {
    rand::random()
}

/// Verify semantic security - same plaintext produces different ciphertext.
#[test]
fn semantic_security_different_ciphertexts() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Same message encrypted multiple times";
    let mut ciphertexts = HashSet::new();
    for _ in 0..100 {
        let (_, ct) = encryption.encrypt(plaintext.as_slice()).unwrap();
        ciphertexts.insert(ct);
    }
    assert_eq!(ciphertexts.len(), 100, "All ciphertexts should be unique");
}

/// Verify nonces are unique and random.
#[test]
fn nonce_uniqueness() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test";
    let mut nonces = HashSet::new();
    for _ in 0..1000 {
        let (nonce, _) = encryption.encrypt(plaintext.as_slice()).unwrap();
        nonces.insert(nonce);
    }
    assert_eq!(nonces.len(), 1000, "All nonces should be unique");
}

/// Verify authentication tag detects single bit flip.
#[test]
fn authentication_single_bit_flip() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message for bit flip detection";
    let (nonce, mut ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    ciphertext[10] ^= 0x01;
    let result = encryption.decrypt(&nonce, &ciphertext);
    assert!(result.is_err(), "Single bit flip should be detected");
}

/// Verify authentication detects byte removal.
#[test]
fn authentication_byte_removal() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message";
    let (nonce, mut ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    ciphertext.pop();
    let result = encryption.decrypt(&nonce, &ciphertext);
    assert!(result.is_err(), "Byte removal should be detected");
}

/// Verify authentication detects byte insertion.
#[test]
fn authentication_byte_insertion() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message";
    let (nonce, mut ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    ciphertext.push(0);
    let result = encryption.decrypt(&nonce, &ciphertext);
    assert!(result.is_err(), "Byte insertion should be detected");
}

/// Verify nonce modification causes decryption failure.
#[test]
fn nonce_modification_fails_decrypt() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message";
    let (mut nonce, ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    nonce[0] ^= 0xFF; // codeql[rust/hard-coded-cryptographic-value] NIST test vector bit-flip
    let result = encryption.decrypt(&nonce, &ciphertext);
    assert!(result.is_err(), "Modified nonce should fail decryption");
}

/// Verify wrong key fails decryption.
#[test]
fn wrong_key_fails_decrypt() {
    let enc1 = ModelEncryption::new(create_test_key());
    let wrong_key: [u8; KEY_SIZE] = rand::random();
    let enc2 = ModelEncryption::new(wrong_key);
    let plaintext = b"Test message";
    let (nonce, ciphertext) = enc1.encrypt(plaintext.as_slice()).unwrap();
    let result = enc2.decrypt(&nonce, &ciphertext);
    assert!(result.is_err(), "Wrong key should fail decryption");
}

/// PBKDF2 key derivation is deterministic with same inputs.
#[test]
fn pbkdf2_deterministic() {
    let pw: String = (0..10).map(|_| rand::random::<char>()).collect();
    let salt: Vec<u8> = (0..16).map(|_| rand::random()).collect();
    let enc1 = ModelEncryption::from_password(&pw, &salt);
    let enc2 = ModelEncryption::from_password(&pw, &salt);
    let plaintext = b"Test message";
    let (nonce, ct) = enc1.encrypt(plaintext.as_slice()).unwrap();
    let decrypted = enc2.decrypt(&nonce, &ct).unwrap();
    assert_eq!(plaintext.as_slice(), decrypted.as_slice());
}

/// PBKDF2 different passwords produce different keys.
#[test]
fn pbkdf2_different_passwords() {
    let pw1: String = (0..10).map(|_| rand::random::<char>()).collect();
    let pw2: String = (0..10).map(|_| rand::random::<char>()).collect();
    let salt: Vec<u8> = (0..16).map(|_| rand::random()).collect();
    let enc1 = ModelEncryption::from_password(&pw1, &salt);
    let enc2 = ModelEncryption::from_password(&pw2, &salt);
    let plaintext = b"Test message";
    let (nonce, ct) = enc1.encrypt(plaintext.as_slice()).unwrap();
    let result = enc2.decrypt(&nonce, &ct);
    assert!(result.is_err(), "Different passwords should fail");
}

/// PBKDF2 different salts produce different keys.
#[test]
fn pbkdf2_different_salts() {
    let pw: String = (0..10).map(|_| rand::random::<char>()).collect();
    let salt1: Vec<u8> = (0..16).map(|_| rand::random()).collect();
    let salt2: Vec<u8> = (0..16).map(|_| rand::random()).collect();
    let enc1 = ModelEncryption::from_password(&pw, &salt1);
    let enc2 = ModelEncryption::from_password(&pw, &salt2);
    let plaintext = b"Test message";
    let (nonce, ct) = enc1.encrypt(plaintext.as_slice()).unwrap();
    let result = enc2.decrypt(&nonce, &ct);
    assert!(result.is_err(), "Different salts should fail");
}

/// Verify empty plaintext encryption works.
#[test]
fn encrypt_empty_plaintext() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext: &[u8] = &b""[..];
    let (nonce, ciphertext) = encryption.encrypt(plaintext).unwrap();
    let decrypted = encryption.decrypt(&nonce, &ciphertext).unwrap();
    assert!(decrypted.is_empty());
}

/// Verify large plaintext encryption works.
#[test]
fn encrypt_large_plaintext() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
    let (nonce, ciphertext) = encryption.encrypt(&plaintext).unwrap();
    let decrypted = encryption.decrypt(&nonce, &ciphertext).unwrap();
    assert_eq!(plaintext, decrypted);
}

/// Verify ciphertext includes 16-byte authentication tag.
#[test]
fn ciphertext_includes_tag() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message";
    let (_, ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    assert_eq!(
        ciphertext.len(),
        plaintext.len() + 16,
        "Ciphertext should include 16-byte tag"
    );
}

/// Verify invalid nonce size is rejected.
#[test]
fn invalid_nonce_size_rejected() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test";
    let (_, ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    let wrong_nonce: Vec<u8> = (0..8).map(|_| rand::random()).collect();
    let result = encryption.decrypt(&wrong_nonce, &ciphertext);
    assert!(result.is_err(), "Invalid nonce size should be rejected");
}

/// Encryption/decryption performance test.
#[test]
fn crypto_performance() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
    let start = Instant::now();
    let (nonce, ciphertext) = encryption.encrypt(&plaintext).unwrap();
    let encrypt_time = start.elapsed();
    let start = Instant::now();
    let decrypted = encryption.decrypt(&nonce, &ciphertext).unwrap();
    let decrypt_time = start.elapsed();
    assert_eq!(plaintext, decrypted);
    assert!(
        encrypt_time.as_millis() < 1000,
        "Encryption too slow: {:?}",
        encrypt_time
    );
    assert!(
        decrypt_time.as_millis() < 1000,
        "Decryption too slow: {:?}",
        decrypt_time
    );
}

/// Verify encryption is not deterministic (IND-CPA security).
#[test]
fn encryption_not_deterministic() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Same message";
    let (nonce1, ct1) = encryption.encrypt(plaintext.as_slice()).unwrap();
    let (nonce2, ct2) = encryption.encrypt(plaintext.as_slice()).unwrap();
    assert_ne!(nonce1, nonce2, "Nonces should be different");
    assert_ne!(ct1, ct2, "Ciphertexts should be different");
}

/// Verify key size is 256 bits (32 bytes).
#[test]
fn key_size_256_bits() {
    assert_eq!(KEY_SIZE, 32, "Key size must be 32 bytes (256 bits)");
}

/// Verify nonce size is 96 bits (12 bytes) for GCM.
#[test]
fn nonce_size_96_bits() {
    assert_eq!(
        NONCE_SIZE, 12,
        "Nonce size must be 12 bytes (96 bits) for GCM"
    );
}

/// Verify decryption fails with truncated ciphertext.
#[test]
fn truncated_ciphertext_fails() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message for truncation";
    let (nonce, ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    let truncated = &ciphertext[..ciphertext.len() / 2];
    let result = encryption.decrypt(&nonce, truncated);
    assert!(result.is_err(), "Truncated ciphertext should fail");
}

/// Verify decryption fails with swapped ciphertext parts.
#[test]
fn swapped_ciphertext_fails() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext = b"Test message for swapping test";
    let (nonce, mut ciphertext) = encryption.encrypt(plaintext.as_slice()).unwrap();
    let mid = ciphertext.len() / 2;
    ciphertext.swap(0, mid);
    let result = encryption.decrypt(&nonce, &ciphertext);
    assert!(result.is_err(), "Swapped ciphertext should fail");
}

/// Verify encryption roundtrip for binary data with all byte values.
#[test]
fn binary_data_all_byte_values() {
    let encryption = ModelEncryption::new(create_test_key());
    let plaintext: Vec<u8> = (0..=255).collect();
    let (nonce, ciphertext) = encryption.encrypt(&plaintext).unwrap();
    let decrypted = encryption.decrypt(&nonce, &ciphertext).unwrap();
    assert_eq!(plaintext, decrypted);
}

/// Verify repeated encryption/decryption cycles are consistent.
#[test]
fn repeated_encrypt_decrypt_consistent() {
    let encryption = ModelEncryption::new(create_test_key());
    let original = b"Message for repeated testing";
    let mut data = original.to_vec();
    for _ in 0..10 {
        let (nonce, ct) = encryption.encrypt(&data).unwrap();
        data = encryption.decrypt(&nonce, &ct).unwrap();
    }
    assert_eq!(original.as_slice(), data.as_slice());
}

/// Verify empty salt still derives a key.
#[test]
fn pbkdf2_empty_salt() {
    let pw: String = (0..10).map(|_| rand::random::<char>()).collect();
    let enc = ModelEncryption::from_password(&pw, b"".as_slice());
    let plaintext = b"Test";
    let (nonce, ct) = enc.encrypt(plaintext.as_slice()).unwrap();
    let decrypted = enc.decrypt(&nonce, &ct).unwrap();
    assert_eq!(plaintext.as_slice(), decrypted.as_slice());
}

/// Verify empty password still derives a key.
#[test]
fn pbkdf2_empty_password() {
    let salt: Vec<u8> = (0..16).map(|_| rand::random()).collect();
    let enc = ModelEncryption::from_password("", &salt); // codeql[rust/hard-coded-cryptographic-value] empty password boundary test
    let plaintext = b"Test";
    let (nonce, ct) = enc.encrypt(plaintext.as_slice()).unwrap();
    let decrypted = enc.decrypt(&nonce, &ct).unwrap();
    assert_eq!(plaintext.as_slice(), decrypted.as_slice());
}
