//! Business logic modules — passkey flows, transaction building, signing, and wallet management.

mod dao;
#[cfg(target_os = "macos")]
mod passkey;
mod transfer;
mod wallet;
