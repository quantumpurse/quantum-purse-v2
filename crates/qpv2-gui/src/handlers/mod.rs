//! Business logic modules — passkey flows, transaction building, signing, and wallet management.
mod fetcher;
#[cfg(target_os = "macos")]
mod passkey;
mod poller;
mod signer;
mod transactions;
mod wallet;
