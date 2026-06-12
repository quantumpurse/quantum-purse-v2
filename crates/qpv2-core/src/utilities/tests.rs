use super::*;
use std::sync::atomic::Ordering;

#[test]
fn test_pass_encrypt_decrypt() {
    let password = vec![1, 2, 3];
    let data = b"test";
    let payload = encrypt_with_password(&password, data).unwrap();
    let decrypted = decrypt_with_password(&password, payload).unwrap();
    assert_eq!(decrypted.as_ref(), data);
}

#[test]
fn test_fail_encrypt_decrypt() {
    let password = vec![1, 2, 3];
    let data = b"test";
    let payload = encrypt_with_password(&password, data).unwrap();
    let password1 = vec![2, 2, 3];
    let result = decrypt_with_password(&password1, payload);
    assert!(result.is_err());
}

#[test]
fn test_zeroize_on_drop_decrypt_output() {
    use crate::containers::ZEROIZED;
    ZEROIZED.store(false, Ordering::SeqCst);
    let password = vec![1, 2, 3];
    let data = b"test";
    let payload = encrypt_with_password(&password, data).unwrap();
    {
        let _decrypted = decrypt_with_password(&password, payload).unwrap();
    } // decrypted is dropped here
    assert!(ZEROIZED.load(Ordering::SeqCst));
}

#[test]
fn test_encrypt_decrypt_with_key() {
    let prf_output = vec![0x42u8; 32]; // Simulated 32-byte PRF output
    let key = derive_vault_enc_key(&prf_output).unwrap();
    let data = b"test key-based encryption";
    let payload = encrypt_with_key(&key, data).unwrap();
    assert!(
        payload.salt.is_empty(),
        "Salt should be empty for key-based encryption"
    );
    let decrypted = decrypt_with_key(&key, payload).unwrap();
    assert_eq!(decrypted.as_ref(), data);
}

#[test]
fn test_fail_decrypt_with_wrong_key() {
    let prf_output_1 = vec![0x42u8; 32];
    let prf_output_2 = vec![0x43u8; 32];
    let key_1 = derive_vault_enc_key(&prf_output_1).unwrap();
    let key_2 = derive_vault_enc_key(&prf_output_2).unwrap();
    let data = b"test";
    let payload = encrypt_with_key(&key_1, data).unwrap();
    let result = decrypt_with_key(&key_2, payload);
    assert!(result.is_err());
}

#[test]
fn test_derive_key_from_prf_deterministic() {
    let prf_output = vec![0xABu8; 32];
    let key_1 = derive_vault_enc_key(&prf_output).unwrap();
    let key_2 = derive_vault_enc_key(&prf_output).unwrap();
    assert_eq!(
        key_1.as_ref(),
        key_2.as_ref(),
        "Same PRF output should derive same key"
    );
}

#[test]
fn test_parse_ckb_to_shannons_exact() {
    // The f64 path turned "0.00000003" into 2 shannons; integer parsing
    // must be exact for every representable amount.
    assert_eq!(parse_ckb_to_shannons("0.00000003"), Ok(3));
    assert_eq!(parse_ckb_to_shannons("0"), Ok(0));
    assert_eq!(parse_ckb_to_shannons("1"), Ok(100_000_000));
    assert_eq!(parse_ckb_to_shannons("0.1"), Ok(10_000_000));
    assert_eq!(parse_ckb_to_shannons("12.5"), Ok(1_250_000_000));
    assert_eq!(
        parse_ckb_to_shannons("37774.55673077"),
        Ok(3_777_455_673_077)
    );
    // f64 rounded this one UP by a shannon; must be exact.
    assert_eq!(
        parse_ckb_to_shannons("90216076.29597175"),
        Ok(9_021_607_629_597_175)
    );
}

#[test]
fn test_parse_ckb_to_shannons_forms() {
    assert_eq!(parse_ckb_to_shannons(" 2.5 "), Ok(250_000_000));
    assert_eq!(parse_ckb_to_shannons("12."), Ok(1_200_000_000));
    assert_eq!(parse_ckb_to_shannons(".5"), Ok(50_000_000));
    // Full u64 range survives.
    assert_eq!(parse_ckb_to_shannons("184467440737.09551615"), Ok(u64::MAX));
}

#[test]
fn test_parse_ckb_to_shannons_rejects() {
    assert!(parse_ckb_to_shannons("").is_err());
    assert!(parse_ckb_to_shannons(".").is_err());
    assert!(parse_ckb_to_shannons("abc").is_err());
    assert!(parse_ckb_to_shannons("-1").is_err());
    assert!(parse_ckb_to_shannons("+1").is_err());
    assert!(parse_ckb_to_shannons("1.2.3").is_err());
    assert!(parse_ckb_to_shannons("1e8").is_err());
    // More than 8 fraction digits must be rejected, not truncated.
    assert!(parse_ckb_to_shannons("0.123456789").is_err());
    // Overflow past u64::MAX shannons.
    assert!(parse_ckb_to_shannons("184467440737.09551616").is_err());
    assert!(parse_ckb_to_shannons("99999999999999999999").is_err());
}
