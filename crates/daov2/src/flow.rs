//! The two steps of a binding, each run off the UI thread: `prepare` when the
//! user pastes an API key, `bind` when they confirm the addresses.

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::api_key;
use crate::checks::{verify_event, Expected};
use crate::client::{AccountInfo, DaoClient};
use crate::event::challenge;
use crate::proof::verify_ack;
use crate::receipt::write_receipt;
use crate::settings::Settings;

/// What Start learns: whose account this is, the server's pinned key, and
/// the addresses the account already has.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub server_url: String,
    pub api_key: String,
    pub account_pubkey: String,
    pub server_pubkey: String,
    pub account: AccountInfo,
    pub bound: Vec<String>,
}

/// A finished binding.
#[derive(Debug, Clone)]
pub struct Bound {
    pub count: usize,
    pub receipt: PathBuf,
}

/// Start: reads the account key from `api_key`, pins the server's key on
/// first contact (stored in `settings_path`), and reads what the account
/// has bound. `is_mainnet` is this wallet's network; a server whose bound
/// addresses are on the other network is refused here, before any signing.
pub fn prepare(
    settings_path: &Path,
    server_url: &str,
    api_key: &str,
    is_mainnet: bool,
) -> Result<Prepared, String> {
    let account_pubkey = api_key::account_pubkey(api_key)?;
    let client = DaoClient::new(server_url)?;

    let server_pubkey = client.server_public_key()?;
    let mut settings = Settings::load(settings_path)?;
    settings.server_url = client.base_url().to_string();
    settings.pin_server_key(client.base_url(), &server_pubkey)?;
    settings.save(settings_path)?;

    let (bound, account) = client.bound_addresses(api_key)?;
    // Bech32 accepts either case, so the prefix is compared case-blind.
    let prefix = if is_mainnet { "ckb1" } else { "ckt1" };
    if let Some(other) = bound
        .iter()
        .find(|a| !a.to_ascii_lowercase().starts_with(prefix))
    {
        return Err(format!(
            "The DAO server lists {other} for this account, which is not a {} address. \
             Switch this wallet's network or use that network's DAO server.",
            if is_mainnet { "mainnet" } else { "testnet" }
        ));
    }

    Ok(Prepared {
        server_url: client.base_url().to_string(),
        api_key: api_key.to_string(),
        account_pubkey,
        server_pubkey,
        account,
        bound,
    })
}

/// Confirm: asks for an event over `addresses`, checks it, signs every
/// address's challenge, submits, checks the server's ack and saves a
/// receipt in `receipts_dir`.
///
/// `read_tip` returns this wallet's own chain tip and is called once the
/// event has arrived. `sign` gets `(address, challenge)` pairs in the
/// event's order and returns one hex signature per pair, in that order.
pub fn bind(
    prepared: &Prepared,
    addresses: &[String],
    read_tip: impl FnOnce() -> Result<Option<u64>, String>,
    sign: impl FnOnce(&[(String, String)]) -> Result<Vec<String>, String>,
    receipts_dir: &Path,
) -> Result<Bound, String> {
    let client = DaoClient::new(&prepared.server_url)?;
    let mut event = client.open_session(&prepared.api_key, addresses)?;
    // Clock skew is judged at arrival: the tip read and the checks below
    // are this wallet's own time, not the server's.
    let received_at = Utc::now();
    let tip = read_tip()?;

    let expected = Expected {
        account_pubkey: &prepared.account_pubkey,
        addresses,
        server_pubkey: &prepared.server_pubkey,
    };
    verify_event(&event, &expected, received_at, tip)?;

    let pairs: Vec<(String, String)> = event
        .ckb_addresses
        .iter()
        .map(|address| (address.clone(), challenge(&event.event_hash, address)))
        .collect();
    let signatures = sign(&pairs)?;
    if signatures.len() != pairs.len() {
        return Err(format!(
            "Signed {} of {} addresses; refusing to submit a partial binding.",
            signatures.len(),
            pairs.len()
        ));
    }
    event.bind_signatures = signatures;

    let ack = client.submit(&prepared.api_key, &event)?;
    match verify_ack(&event, &ack, &prepared.server_pubkey) {
        Ok(()) => {
            let receipt = write_receipt(receipts_dir, &event, Some(&ack))?;
            tracing::info!("DAO v2 binding acked (leaf_index={})", ack.leaf_index);
            Ok(Bound {
                count: event.ckb_addresses.len(),
                receipt,
            })
        }
        Err(e) => {
            let receipt = write_receipt(receipts_dir, &event, None)?;
            Err(format!(
                "{e} The signed event was saved to {}; the binding may not be safely archived.",
                receipt.display()
            ))
        }
    }
}
