use crate::{ACCOUNT, KEY_LEN, SERVICE};
use qpv2_core::SecureVec;
use security_framework::passwords::{
    delete_generic_password_options, generic_password, set_generic_password_options,
    AccessControlOptions, PasswordOptions,
};

fn protected_opts() -> PasswordOptions {
    let mut opts = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
    opts.use_protected_keychain();
    opts
}

fn map_err(e: security_framework::base::Error) -> String {
    match e.code() {
        -128 => "Cancelled.".to_string(),
        -25293 => "Touch ID authentication failed.".to_string(),
        -25300 => "Keychain key not found.".to_string(),
        -25308 => "Keychain interaction not allowed.".to_string(),
        -34018 => {
            "Keychain access denied — binary may need code signing with entitlements.".to_string()
        }
        code => format!("Keychain error ({}): {}", code, e),
    }
}

pub fn store_key(key: &[u8]) -> Result<(), String> {
    if key.len() != KEY_LEN {
        return Err(format!("Expected {KEY_LEN}-byte key, got {}", key.len()));
    }
    if let Err(e) = delete_generic_password_options(protected_opts()) {
        if e.code() != -25300 {
            return Err(map_err(e));
        }
    }
    let mut opts = protected_opts();
    opts.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
    set_generic_password_options(key, opts).map_err(map_err)
}

pub fn retrieve_key() -> Result<SecureVec, String> {
    let bytes = generic_password(protected_opts()).map_err(map_err)?;
    let secure = SecureVec::from_vec(bytes);
    if secure.len() != KEY_LEN {
        return Err(format!(
            "Keychain returned {}-byte key, expected {KEY_LEN}",
            secure.len()
        ));
    }
    Ok(secure)
}

/// Delete the encryption key from the Keychain.
pub fn delete_key() -> Result<(), String> {
    delete_generic_password_options(protected_opts()).map_err(map_err)
}
