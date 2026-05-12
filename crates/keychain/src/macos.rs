use qpv2_core::SecureVec;
use security_framework::passwords::{
    delete_generic_password_options, generic_password, set_generic_password_options,
    AccessControlOptions, PasswordOptions,
};

const SERVICE: &str = "quantumpurse";
const ACCOUNT: &str = "vault-encryption-key";
const KEY_LEN: usize = 32;

fn protected_opts() -> PasswordOptions {
    let mut opts = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
    opts.use_protected_keychain();
    opts
}

fn map_err(e: security_framework::base::Error) -> String {
    let code = e.code();
    match code {
        -128 => "Cancelled.".to_string(),
        -25293 => "Touch ID authentication failed.".to_string(),
        -25300 => "Keychain key not found.".to_string(),
        -25308 => "Keychain interaction not allowed.".to_string(),
        _ => format!("Keychain error ({}): {}", code, e),
    }
}

/// Store a 32-byte encryption key in the Keychain with biometric
/// access control. Does NOT trigger Touch ID — writes are unguarded.
/// Deletes any existing item first to guarantee the biometric access
/// control attributes are applied cleanly.
pub fn store_key(key: &[u8]) -> Result<(), String> {
    if key.len() != KEY_LEN {
        return Err(format!("Expected {KEY_LEN}-byte key, got {}", key.len()));
    }
    let _ = delete_generic_password_options(protected_opts());
    let mut opts = protected_opts();
    opts.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
    set_generic_password_options(key, opts).map_err(map_err)
}

/// Retrieve the encryption key from the Keychain. Blocks the calling
/// thread until the user authenticates with Touch ID or cancels.
pub fn retrieve_key() -> Result<SecureVec, String> {
    let bytes = generic_password(protected_opts()).map_err(map_err)?;
    if bytes.len() != KEY_LEN {
        return Err(format!(
            "Keychain returned {}-byte key, expected {KEY_LEN}",
            bytes.len()
        ));
    }
    Ok(SecureVec::from_vec(bytes))
}

/// Delete the encryption key from the Keychain.
pub fn delete_key() -> Result<(), String> {
    delete_generic_password_options(protected_opts()).map_err(map_err)
}
