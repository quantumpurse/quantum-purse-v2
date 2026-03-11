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
    let key = derive_key_from_prf(&prf_output).unwrap();
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
    let key_1 = derive_key_from_prf(&prf_output_1).unwrap();
    let key_2 = derive_key_from_prf(&prf_output_2).unwrap();
    let data = b"test";
    let payload = encrypt_with_key(&key_1, data).unwrap();
    let result = decrypt_with_key(&key_2, payload);
    assert!(result.is_err());
}

#[test]
fn test_derive_key_from_prf_deterministic() {
    let prf_output = vec![0xABu8; 32];
    let key_1 = derive_key_from_prf(&prf_output).unwrap();
    let key_2 = derive_key_from_prf(&prf_output).unwrap();
    assert_eq!(
        key_1.as_ref(),
        key_2.as_ref(),
        "Same PRF output should derive same key"
    );
}
