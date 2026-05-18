//! Multi-platform credential storage.
//!
//! Stores and retrieves a 32-byte AES-256 encryption key using
//! the platform's native credential store:
//! - macOS: Data Protection Keychain with Touch ID biometric gating.
//! - Windows: Credential Manager (DPAPI-protected).
//! - Linux: Secret Service D-Bus (GNOME Keyring / KDE Wallet).
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
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{delete_key, retrieve_key, store_key};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{delete_key, retrieve_key, store_key};

pub fn keystore_display_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Touch ID (Keychain)"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Credential Manager"
    }
    #[cfg(target_os = "linux")]
    {
        "Secret Service (D-Bus)"
    }
}

pub fn keystore_short_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Touch ID"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Credential Manager"
    }
    #[cfg(target_os = "linux")]
    {
        "Secret Service"
    }
}
