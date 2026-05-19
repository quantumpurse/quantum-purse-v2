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

#[cfg(feature = "fido2")]
pub mod fido2;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{delete_key, retrieve_key, store_key};

#[cfg(target_os = "windows")]
mod windows_hello;
#[cfg(target_os = "windows")]
pub use windows_hello::{delete_key, retrieve_key, store_key};

#[cfg(target_os = "linux")]
mod linux_tpm;
#[cfg(target_os = "linux")]
pub use linux_tpm::{delete_key, retrieve_key, store_key};

pub fn keystore_display_name() -> &'static str {
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

pub fn keystore_short_name() -> &'static str {
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
