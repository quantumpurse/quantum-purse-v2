//! macOS Passkey PRF bridge via the AuthenticationServices framework.
//!
//! Provides passkey registration with PRF support and assertion with PRF output
//! retrieval, suitable for deriving symmetric encryption keys from hardware-backed
//! credentials via Touch ID.
//!
//! Requires macOS 15.0+ and a signed `.app` bundle with associated domains entitlement.

#[cfg(not(target_os = "macos"))]
compile_error!("passkey-prf only supports macOS");

#[cfg(target_os = "macos")]
mod bridge;

#[cfg(target_os = "macos")]
pub use bridge::*;
