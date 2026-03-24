//! Business logic modules — passkey flows, transaction building, signing, and wallet management.
#[cfg(target_os = "macos")]
mod passkey;
mod tx_builder;
mod poller;
mod wallet;
mod fetcher;
mod signer;