use super::*;

#[test]
fn test_power_on_self_tests() {
    let results = run_power_on_self_tests().unwrap();
    assert!(results.all_passed());
}

#[test]
fn test_aes_gcm_kat() {
    assert!(aes_gcm_kat().is_ok());
}

#[test]
fn test_pbkdf2_kat() {
    assert!(pbkdf2_kat().is_ok());
}

#[test]
fn test_rng_health() {
    assert!(rng_health_test().is_ok());
}

#[test]
fn test_integrity() {
    assert!(integrity_self_test().is_ok());
}

#[test]
fn test_self_test_error_display() {
    let err = SelfTestError::AesGcmEncryptFailed;
    assert!(err.to_string().contains("AES-GCM"));

    let err = SelfTestError::Pbkdf2Failed;
    assert!(err.to_string().contains("PBKDF2"));

    let err = SelfTestError::RngHealthFailed;
    assert!(err.to_string().contains("RNG"));
}

#[test]
fn test_count_differing_bits() {
    let a = [0x00u8; 32];
    let b = [0xFFu8; 32];
    assert_eq!(count_differing_bits(&a, &b), 256);

    let c = [0x00u8; 32];
    assert_eq!(count_differing_bits(&a, &c), 0);
}
