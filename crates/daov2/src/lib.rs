//! CKB DAO v2 address binding, wallet side.
//!
//! Binding proves "these CKB addresses give their voting power to this DAO
//! account key". The wallet signs nothing it takes from the server alone:
//! the account key comes from the API key the user pasted, the address list
//! from the user's own choice, and time and chain height from this device.
//! Only when the server's event agrees with all of them does the wallet sign
//! one challenge per address, which it builds itself from that event.
//!
//! The rules referenced throughout are the consensus rules in the DAO v2
//! design (`ckb-dao-v2-code/README.md`, section 2a). The event hash must
//! match the DAO backend byte for byte; the tests pin values computed by the
//! backend's own code.

pub mod api_key;
pub mod checks;
pub mod client;
pub mod event;
pub mod flow;
mod hash;
pub mod proof;
pub mod receipt;
pub mod settings;
