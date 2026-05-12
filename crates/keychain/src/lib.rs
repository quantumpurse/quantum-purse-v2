//! macOS Keychain + Touch ID key storage.
//!
//! Stores and retrieves a 32-byte AES-256 encryption key in the macOS
//! Keychain, gated by `kSecAccessControlBiometryCurrentSet` (Touch ID).
//! Reads block until the user authenticates or cancels; writes do not
//! trigger biometric.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{delete_key, retrieve_key, store_key};
