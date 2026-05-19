//! Multi-platform credential storage.
//!
//! Stores and retrieves a 32-byte AES-256 encryption key using
//! the platform's native credential store:
//! - macOS: Data Protection Keychain with Touch ID biometric gating.
//! - Windows: TPM + Windows Hello via Microsoft Passport KSP.
//! - Linux: TPM seal/unseal via `tss-esapi`.
//!
//! Optionally provides FIDO2 hardware key authentication via
//! the `fido2` feature flag.

pub(crate) const SERVICE: &str = "quantumpurse";
pub(crate) const ACCOUNT: &str = "vault-encryption-key";
pub(crate) const KEY_LEN: usize = 32;

pub mod hw_backed;
#[cfg(feature = "fido2")]
pub use hw_backed::fido2;
pub use hw_backed::{delete_key, retrieve_key, store_key};

pub fn display_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Touch ID (Keychain)"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Hello (TPM)"
    }
    #[cfg(target_os = "linux")]
    {
        "TPM"
    }
}

pub fn short_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Touch ID"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Hello"
    }
    #[cfg(target_os = "linux")]
    {
        "TPM"
    }
}
