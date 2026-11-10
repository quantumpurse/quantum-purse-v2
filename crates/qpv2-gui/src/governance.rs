//! DAO v2 address binding: Start reads the account behind a pasted API key
//! and offers this wallet's unbound addresses; Confirm signs them and binds
//! them on the DAO server. The checks and the server calls live in the
//! `daov2` crate; this file runs them off the UI thread. The screens are in
//! `ui/governance.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use daov2::flow::{Bound, Prepared};
use qpv2_core::types::{AuthKey, AuthMethod, SingleSigConvention};
use qpv2_core::KeyVault;

use crate::types::Status;
use crate::utils::lock_args_to_address;
use crate::wallet::{get_fido2_dek, spawn_fido2_job};
use crate::App;

/// What the binding asks the user to authenticate for.
const AUTH_PURPOSE: &str = "bind addresses to your DAO v2 account";

/// Binding state on `App`. The server URL survives wallet switches; the
/// rest belongs to the wallet it was started in.
pub(crate) struct Governance {
    /// Edit buffer for the DAO server URL; saved when Start succeeds.
    pub(crate) server_url: String,
    /// The pasted API key. Spent by one binding and expires after 60 s.
    pub(crate) api_key: String,
    pub(crate) prepare_rx: Option<Receiver<Result<Prepared, String>>>,
    /// The address picker, open between Start and the end of a binding.
    pub(crate) picker: Option<Picker>,
    pub(crate) bind_rx: Option<Receiver<Result<Bound, String>>>,
    /// Whether the binding in flight waits on a security key.
    pub(crate) bind_uses_fido2: bool,
    pub(crate) last_receipt: Option<PathBuf>,
}

pub(crate) struct Picker {
    pub(crate) prepared: Prepared,
    pub(crate) candidates: Vec<Candidate>,
}

pub(crate) struct Candidate {
    pub(crate) address: String,
    pub(crate) lock_args: String,
    pub(crate) selected: bool,
}

impl Governance {
    pub(crate) fn new() -> Self {
        let server_url = match settings_path().and_then(|p| daov2::settings::Settings::load(&p)) {
            Ok(settings) => settings.server_url,
            Err(e) => {
                tracing::error!("{}", e);
                daov2::settings::DEFAULT_SERVER_URL.to_string()
            }
        };
        Self {
            server_url,
            api_key: String::new(),
            prepare_rx: None,
            picker: None,
            bind_rx: None,
            bind_uses_fido2: false,
            last_receipt: None,
        }
    }

    /// Drops everything tied to the current wallet, including any job in
    /// flight, so a late result cannot land in another wallet.
    pub(crate) fn reset(&mut self) {
        self.api_key.clear();
        self.prepare_rx = None;
        self.picker = None;
        self.bind_rx = None;
        self.bind_uses_fido2 = false;
        self.last_receipt = None;
    }

    pub(crate) fn busy(&self) -> bool {
        self.prepare_rx.is_some() || self.bind_rx.is_some()
    }
}

/// `<data dir>/daov2.json`: the server URL and each server's pinned key.
fn settings_path() -> Result<PathBuf, String> {
    Ok(qpv2_core::db::get_data_dir()
        .map_err(|e| e.to_string())?
        .join("daov2.json"))
}

/// `<data dir>/daov2/receipts`, where binding receipts are kept.
pub(crate) fn receipts_dir() -> Result<PathBuf, String> {
    Ok(qpv2_core::db::get_data_dir()
        .map_err(|e| e.to_string())?
        .join("daov2")
        .join("receipts"))
}

impl App {
    /// Why this wallet cannot bind, if it cannot.
    pub(crate) fn governance_blocker(&self) -> Option<String> {
        if matches!(self.auth_method, Some(AuthMethod::Trezor { .. })) {
            return Some(
                "Binding signs a message, which the Trezor integration cannot do yet.".to_string(),
            );
        }
        match KeyVault::get_single_sig_convention(self.wallet_id) {
            Ok(SingleSigConvention::Standard) => None,
            Ok(SingleSigConvention::V1) => Some(
                "This wallet uses the Quantum Purse v1 address format, which DAO v2 does not \
                 accept. Bind from a wallet created in this app."
                    .to_string(),
            ),
            Err(e) => Some(format!(
                "Failed to read this wallet's address format: {}",
                e
            )),
        }
    }

    /// Start: check the API key and read the account on a worker.
    pub(crate) fn start_governance_binding(&mut self) {
        let api_key = self.governance.api_key.trim().to_string();
        if api_key.is_empty() {
            self.status = Status::Error("Paste the API key from the DAO website first.".into());
            return;
        }
        let settings_path = match settings_path() {
            Ok(p) => p,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let server_url = self.governance.server_url.trim().to_string();
        let is_mainnet = self.qp_client.is_mainnet();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("daov2-prepare".to_string())
            .spawn(move || {
                let _ = tx.send(daov2::flow::prepare(
                    &settings_path,
                    &server_url,
                    &api_key,
                    is_mainnet,
                ));
            });
        match spawned {
            Ok(_) => {
                self.governance.last_receipt = None;
                self.governance.prepare_rx = Some(rx);
            }
            Err(e) => self.status = Status::Error(format!("Failed to start binding: {}", e)),
        }
    }

    /// Confirm: authenticate first, so typing a password or touching a key
    /// does not eat the server's 20-second signing window, then open the
    /// session, check, sign and submit on a worker.
    pub(crate) fn confirm_governance_binding(&mut self) {
        let Some(picker) = &self.governance.picker else {
            return;
        };
        let chosen: Vec<&Candidate> = picker.candidates.iter().filter(|c| c.selected).collect();
        if chosen.is_empty() {
            return;
        }
        let addresses: Vec<String> = chosen.iter().map(|c| c.address.clone()).collect();
        let lock_args: HashMap<String, String> = chosen
            .iter()
            .map(|c| (c.address.clone(), c.lock_args.clone()))
            .collect();
        let prepared = picker.prepared.clone();

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read the key variant: {}", e));
                return;
            }
        };
        let receipts = match receipts_dir() {
            Ok(d) => d,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
        let qp_client = self.qp_client.clone();
        let job = move |auth: AuthKey| {
            daov2::flow::bind(
                &prepared,
                &addresses,
                || {
                    qp_client
                        .get_tip_header()
                        .map(|header| Some(u64::from(header.inner.number)))
                        .map_err(|e| format!("Failed to read the chain tip: {}", e))
                },
                |pairs| {
                    let requests = pairs
                        .iter()
                        .map(|(address, challenge)| {
                            (lock_args[address].clone(), challenge.as_bytes().to_vec())
                        })
                        .collect();
                    vault
                        .ckb_sign_batch(auth, requests)
                        .map(|signatures| signatures.iter().map(hex::encode).collect())
                },
                &receipts,
            )
        };

        if let Some(AuthMethod::Fido2 { .. }) = &self.auth_method {
            let credential_id = match self.get_fido2_credential() {
                Ok(c) => c,
                Err(e) => {
                    self.status = Status::Error(e);
                    return;
                }
            };
            let fido2_job = move || {
                let auth = get_fido2_dek(&credential_id, AUTH_PURPOSE)
                    .map_err(|e| format!("Auth failed: {}", e))?;
                job(auth)
            };
            match spawn_fido2_job(fido2_job) {
                Ok(rx) => {
                    self.governance.bind_rx = Some(rx);
                    self.governance.bind_uses_fido2 = true;
                }
                Err(e) => self.status = Status::Error(e),
            }
            return;
        }

        let auth = match self.resolve_auth_key(AUTH_PURPOSE) {
            Ok(a) => a,
            Err(e) => {
                self.status = Status::Error(format!("Auth failed: {}", e));
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("daov2-bind".to_string())
            .spawn(move || {
                let _ = tx.send(job(auth));
            });
        match spawned {
            Ok(_) => {
                self.governance.bind_rx = Some(rx);
                self.governance.bind_uses_fido2 = false;
            }
            Err(e) => self.status = Status::Error(format!("Failed to start binding: {}", e)),
        }
    }

    pub(crate) fn cancel_governance_binding(&mut self) {
        self.governance.picker = None;
        self.status = Status::Info("Address binding cancelled.".to_string());
    }

    /// Picks up the results of Start and Confirm.
    pub(crate) fn poll_governance(&mut self) {
        if let Some(rx) = &self.governance.prepare_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.governance.prepare_rx = None;
                    match result {
                        Ok(prepared) => self.open_governance_picker(prepared),
                        Err(e) => {
                            tracing::error!("{}", e);
                            self.status = Status::Error(e);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.governance.prepare_rx = None;
                    self.status = Status::Error("The binding thread stopped unexpectedly.".into());
                }
            }
        }

        if let Some(rx) = &self.governance.bind_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.governance.bind_rx = None;
                    self.governance.bind_uses_fido2 = false;
                    self.governance.picker = None;
                    match result {
                        Ok(bound) => {
                            self.governance.api_key.clear();
                            self.status = Status::Info(format!(
                                "Bound {} address(es) to your DAO account.",
                                bound.count
                            ));
                            self.governance.last_receipt = Some(bound.receipt);
                        }
                        Err(e) => {
                            tracing::error!("{}", e);
                            self.status = Status::Error(e);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.governance.bind_rx = None;
                    self.governance.bind_uses_fido2 = false;
                    self.governance.picker = None;
                    self.status = Status::Error("The binding thread stopped unexpectedly.".into());
                }
            }
        }
    }

    /// Offers this wallet's single-sig addresses the account has not bound.
    fn open_governance_picker(&mut self, prepared: Prepared) {
        let accounts = match KeyVault::get_singlesig_accounts(self.wallet_id) {
            Ok(a) => a,
            Err(e) => {
                self.status = Status::Error(format!("Failed to load accounts: {}", e));
                return;
            }
        };
        let is_mainnet = self.qp_client.is_mainnet();
        let mut candidates = Vec::new();
        for account in accounts {
            match lock_args_to_address(&account.lock_args, is_mainnet) {
                Ok(address) => {
                    let address = address.to_string();
                    if !prepared.bound.contains(&address) {
                        candidates.push(Candidate {
                            address,
                            lock_args: account.lock_args,
                            selected: true,
                        });
                    }
                }
                Err(e) => tracing::error!("Skipping account {}: {}", account.lock_args, e),
            }
        }
        if candidates.is_empty() {
            self.status =
                Status::Info("All your addresses are already bound to this account.".to_string());
            return;
        }
        self.governance.picker = Some(Picker {
            prepared,
            candidates,
        });
    }
}
