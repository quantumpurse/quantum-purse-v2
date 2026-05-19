//! Wallet lifecycle: create, unlock, lock, new account, config.

use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use crate::tx_history_store::TxHistoryStore;
use crate::types::{Screen, Status, Tab, TransactionStatus};
use crate::App;

impl App {
    /// Tells the running light client to start indexing the given
    /// accounts from the current tip onward, in a single `set_scripts`
    /// RPC call. No-op when the backend isn't LightClient, no local
    /// process is running, or the input is empty — full nodes / public
    /// RPC index everything by default, and a stopped light client has
    /// nothing to register against.
    ///
    /// Import-existing-wallet (register every account with an earlier
    /// start block per account) is deliberately not covered here; that
    /// flow isn't implemented yet.
    pub(crate) fn register_lock_scripts_with_light_client(&mut self, lock_args_list: &[String]) {
        if self.qp_client.config().node_type != ckb_node::NodeType::LightClient
            || !self.local_node.has_local_process()
            || lock_args_list.is_empty()
        {
            return;
        }
        if let Err(e) =
            ckb_node::wallet_helpers::lc::register_lock_scripts(&self.qp_client, lock_args_list)
        {
            self.status = Status::Error(format!("Failed to register scripts: {}", e));
        }
    }

    /// Kicks off a background detection of the earliest funding block
    /// across all accounts via an ad-hoc `FullNodeClient` against the
    /// public RPC endpoint for the active network. Result lands in
    /// `earliest_funding_block_rx`; the poller writes it into
    /// `set_block_input`. No-op when accounts is empty or another
    /// detection is already in flight.
    pub(crate) fn detect_earliest_funding_block_async(&mut self) {
        if self.earliest_funding_block_rx.is_some() || self.accounts.is_empty() {
            return;
        }

        let network = self.qp_client.network();
        // Always use the network's public RPC — even if the active
        // backend is already PublicRpc, building a fresh client keeps
        // this a self-contained one-shot.
        let public_rpc_url =
            ckb_node::NodeConfig::default_rpc_url_for(ckb_node::NodeType::PublicRpc, network)
                .to_string();
        let accounts = self.accounts.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        self.earliest_funding_block_rx = Some(rx);

        std::thread::spawn(move || {
            let pub_rpc_client = ckb_node::client::FullNodeClient::new(&public_rpc_url);
            let result = pub_rpc_client
                .find_earliest_funding_block(&accounts, network)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    /// Manual override: force the running light client to start scanning
    /// every account from `start_block`. Bypasses the cursor-preservation
    /// filter — use only from a UI control where the user explicitly
    /// asked for it. No-op outside LightClient or with no local process.
    pub(crate) fn set_all_accounts_lock_script_block(&mut self, start_block: u64) {
        if self.qp_client.config().node_type != ckb_node::NodeType::LightClient
            || !self.local_node.has_local_process()
            || self.accounts.is_empty()
        {
            return;
        }
        let accounts = self.accounts.clone();
        if let Err(e) = ckb_node::wallet_helpers::lc::register_all_lock_scripts(
            &self.qp_client,
            &accounts,
            start_block,
        ) {
            self.status = Status::Error(format!("Failed to set scan block: {}", e));
        } else {
            // Reflect the new value in the Synced cell immediately.
            self.node_status.synced_block = Some(start_block);
            self.status = Status::Info(format!("Rescan from block {} requested.", start_block));
        }
    }

    /// Highest committed block number in `tx_history`, or 0 when empty.
    /// Used as `after_block` for the next incremental sync. Derived from
    /// the in-memory vector — no cached state to keep in sync.
    pub(crate) fn tx_history_watermark(&self) -> u64 {
        self.tx_history
            .iter()
            .filter(|r| !r.is_pending)
            .map(|r| r.block_number)
            .max()
            .unwrap_or(0)
    }

    /// Lock the wallet: clear sensitive state and return to the Locked screen.
    pub(crate) fn lock_wallet(&mut self) {
        self.accounts.clear();
        self.balances.clear();
        self.confirm_remove = false;
        self.active_tab = Tab::Dashboard;
        self.screen = Screen::Locked;
        self.status = Status::None;

        // Drop the receiver *before* clearing in-memory state. If we don't,
        // an in-flight sync thread's late `Done` event would repopulate
        // `tx_history` and — worse — write it back to disk, undoing both
        // lock and a subsequent `clear_database()`. Dropping the receiver
        // also short-circuits `poll_tx_history()`. The background thread
        // exits on its next `send(...)` (channel disconnected).
        self.tx_history_rx = None;
        // Drop the in-memory tx history; the on-disk file is kept so the
        // next unlock can reload it instantly.
        self.tx_history.clear();

        // Clear form state so stale values don't persist across sessions.
        self.transfer_recipient.clear();
        self.transfer_amount.clear();
        self.transfer_all = false;
        self.transfer_from_account = 0;
        self.dao_deposit_amount.clear();
        self.dao_deposit_all = false;
        self.dao_deposit_from_account = 0;
        self.tx_status = TransactionStatus::Idle;
    }

    /// Called when the node type or network changes in the UI. Refreshes
    /// `settings_rpc_url` to the default URL for the pending
    /// `(temp_node_type, temp_network)` pair so the form preview matches
    /// what the user is about to commit.
    pub(crate) fn on_node_type_changed(&mut self) {
        self.settings_rpc_url =
            ckb_node::NodeConfig::default_rpc_url_for(self.temp_node_type, self.temp_network)
                .to_string();
    }

    /// Commit the staged settings edits as the active node config:
    ///
    /// 1. Build the new `NodeConfig` from the settings-buffer fields
    ///    (`settings_*`, `temp_*`) on top of the current
    ///    `qp_client.config()` snapshot. There is no separate mutable
    ///    `node_config` on `App`, so this is the only place where an
    ///    edit becomes committed state.
    /// 2. Persist the config to disk.
    /// 3. Replace `qp_client` and `local_node` with fresh instances
    ///    bound to it.
    /// 4. Wipe cached metrics (`node_status`) and drop any in-flight
    ///    poll receiver from the previous backend so its result can't
    ///    land after the swap and resurrect stale values.
    /// 5. Kick off fresh balance + node-status fetches.
    pub(crate) fn apply_node_config(&mut self) {
        let mut new_cfg = self.qp_client.config().clone();
        new_cfg.node_type = self.temp_node_type;
        new_cfg.network = self.temp_network;
        new_cfg.rpc_url = self.settings_rpc_url.clone();

        if new_cfg.requires_binary() && !self.settings_binary_path.is_empty() {
            new_cfg.binary_path = Some(self.settings_binary_path.clone().into());
        } else if !new_cfg.requires_binary() {
            new_cfg.binary_path = None;
        }

        if !self.settings_data_dir.is_empty() {
            new_cfg.data_dir = self.settings_data_dir.clone().into();
        }

        if let Err(e) = new_cfg.save() {
            self.status = Status::Error(format!("Failed to save config: {}", e));
            return;
        }

        // Replace the manager + client with fresh ones bound to the
        // newly-saved config. The old LocalNodeProcess's drop stops its
        // child cleanly via the inner *Process::drop; the old
        // QpClient lives on inside any in-flight thread until that
        // thread finishes its unit of work.
        self.qp_client = ckb_node::QpClient::new(new_cfg.clone());
        self.local_node = ckb_node::LocalNodeProcess::new(new_cfg);

        // Cached node-status metrics — tip block, peer count, RPC
        // port, DB size, synced block — are all backend-specific and
        // instantly stale on switch. Drop any in-flight poll from the
        // previous backend at the same time so its result can't land
        // *after* the reset and resurrect old values.
        self.node_status = crate::types::NodeStatus::default();
        self.node_status_rx = None;

        // The QR-lock-script cell-dep warmup is per-LC-instance.
        // Reset the latch so the poller refires `fetch_qr_lock_dep`
        // against the new backend (and skips it altogether for
        // FullNode / PublicRpc).
        self.lc_qr_dep_warmup_done = false;

        self.status = Status::Info("Configuration saved. RPC reconnected.".to_string());

        // Refresh balances + node status against the new connection so
        // the card repopulates promptly instead of waiting for the
        // next ~10s tick.
        self.fetch_all_balances();
        self.fetch_node_status();
    }

    /// Seeds `tx_history` from the active network's on-disk cache so the
    /// dashboard renders instantly on unlock instead of waiting for the
    /// first sync tick. The incremental-sync floor (`tx_history_watermark`)
    /// is derived from the loaded records. Silent on absence (fresh wallet
    /// or first time on this network) or read failure (corrupted file →
    /// surfaces as a status warning; next sync rebuilds from scratch).
    pub(crate) fn load_tx_history_from_disk(&mut self) {
        match TxHistoryStore::load(self.qp_client.network().tag()) {
            Ok(Some(store)) => {
                self.tx_history = store.records;
            }
            Ok(None) => {
                self.tx_history.clear();
            }
            Err(e) => {
                self.tx_history.clear();
                self.status = Status::Error(format!("Failed to read cached tx history: {}", e));
            }
        }
    }

    /// Create a password-mode wallet. Opens the pinentry dialog with
    /// a confirmation field — the dialog itself enforces the match
    /// (the user can't submit until both fields agree). On submit,
    /// validates strength via `password_checker`, generates the
    /// master seed, derives the first account, and transitions to
    /// `Screen::Unlocked`. Cancellation surfaces as a quiet info
    /// banner; nothing else changes.
    pub(crate) fn create_wallet_with_password(&mut self, variant: SpxVariant) {
        let pw = match qpv2_core::pinentry::prompt_password_with_confirmation(
            "Choose a password for your wallet. You'll be prompted for it \
             again on every signing operation.",
            "Password:",
            "Confirm:",
            "Passwords do not match.",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let strength_str = match qpv2_core::utilities::password_checker(&pw) {
            Ok(bits) => format!(" Password strength: {} bits.", bits),
            Err(e) => {
                self.status = Status::Error(format!("Weak password: {}", e));
                return;
            }
        };

        // `generate_master_seed` and `gen_new_account` each consume an
        // owned `AuthKey::Password(SecureString)`. Mirror the passkey
        // path (one Touch ID → PRF used twice) and the CLI path (one
        // input → reused) by cloning the SecureString once instead of
        // re-prompting. Both copies zeroize-on-drop.
        let pw_for_account = pw.clone();
        let vault = KeyVault::new(variant);
        if let Err(e) = vault.generate_master_seed(AuthKey::Password(pw), AuthMethod::Password) {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::Password(pw_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(AuthMethod::Password);
            return;
        }

        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.auth_method = Some(AuthMethod::Password);
                self.register_lock_scripts_with_light_client(&self.accounts.clone());
                self.screen = Screen::Unlocked;
                self.status = Status::Info(format!("Wallet created successfully!{}", strength_str));
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.auth_method = Some(AuthMethod::Password);
            }
        }
    }

    /// Prompt for the wallet password and derive a new account.
    /// Synchronous: blocks the egui update loop while the pinentry
    /// dialog is up.
    pub(crate) fn create_new_account_with_password(&mut self) {
        let pw = match qpv2_core::pinentry::prompt_password(
            "Enter your wallet password to create a new account.",
            "Password:",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant);
        match vault.gen_new_account(AuthKey::Password(pw)) {
            Ok(lock_args) => {
                self.balances.insert(lock_args.clone(), None);
                let qp_client = self.qp_client.clone();
                let args = lock_args.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.balance_receiver = Some(rx);
                std::thread::spawn(move || {
                    let result = ckb_node::wallet_helpers::queries::fetch_quantum_lock_balance(
                        &qp_client, &args,
                    )
                    .map_err(|e| e.to_string());
                    let _ = tx.send((args, result));
                });
                self.accounts.push(lock_args.clone());
                self.register_lock_scripts_with_light_client(std::slice::from_ref(&lock_args));
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }

    /// Create a Keychain wallet. Generates a random 32-byte key,
    /// stores it in the platform credential store, then creates the
    /// wallet and first account.
    pub(crate) fn create_wallet_with_keychain(&mut self, variant: SpxVariant) {
        let key = match qpv2_core::utilities::get_random_bytes(32) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(format!("Failed to generate key: {}", e));
                return;
            }
        };

        if let Err(e) = credential_gate::store_key(&key) {
            self.status = Status::Error(format!("Failed to store key in Keychain: {}", e));
            return;
        }

        let key_for_account = key.clone();

        let vault = KeyVault::new(variant);
        if let Err(e) = vault.generate_master_seed(AuthKey::CryptoKey(key), AuthMethod::Keychain) {
            let _ = credential_gate::delete_key();
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(AuthMethod::Keychain);
            return;
        }

        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.auth_method = Some(AuthMethod::Keychain);
                self.register_lock_scripts_with_light_client(&self.accounts.clone());
                self.screen = Screen::Unlocked;
                self.status = Status::Info(format!(
                    "Wallet created with {}!",
                    credential_gate::short_name()
                ));
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.auth_method = Some(AuthMethod::Keychain);
            }
        }
    }

    /// Unlock via the platform credential store, then transition to
    /// Unlocked.
    pub(crate) fn unlock_with_keychain(&mut self) {
        match credential_gate::retrieve_key() {
            Ok(_) => match KeyVault::get_all_sphincs_lock_args() {
                Ok(lock_args) => {
                    self.accounts = lock_args;
                    self.screen = Screen::Unlocked;
                    self.status = Status::None;
                    self.last_poll_time = std::time::Instant::now();
                    self.load_tx_history_from_disk();
                    self.fetch_all_balances();
                    self.fetch_tx_history(true);
                    self.fetch_dao_cells();
                    self.fetch_node_status();
                }
                Err(e) => {
                    self.status = Status::Error(format!("Failed to unlock: {}", e));
                }
            },
            Err(e) => {
                self.status = Status::Error(e);
            }
        }
    }

    /// Derive a new account using the platform credential store.
    pub(crate) fn create_new_account_with_keychain(&mut self) {
        let key = match credential_gate::retrieve_key() {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant);
        match vault.gen_new_account(AuthKey::CryptoKey(key)) {
            Ok(lock_args) => {
                self.balances.insert(lock_args.clone(), None);
                let qp_client = self.qp_client.clone();
                let args = lock_args.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.balance_receiver = Some(rx);
                std::thread::spawn(move || {
                    let result = ckb_node::wallet_helpers::queries::fetch_quantum_lock_balance(
                        &qp_client, &args,
                    )
                    .map_err(|e| e.to_string());
                    let _ = tx.send((args, result));
                });
                self.accounts.push(lock_args.clone());
                self.register_lock_scripts_with_light_client(std::slice::from_ref(&lock_args));
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }

    /// Create a FIDO2-authenticated wallet. Prompts for the device PIN
    /// via pinentry, registers a credential, then derives the encryption
    /// key via hmac-secret.
    pub(crate) fn create_wallet_with_fido2(&mut self, variant: SpxVariant) {
        let pin = match qpv2_core::pinentry::prompt_password(
            "Enter your FIDO2 security key PIN to register a new credential.",
            "PIN:",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let credential = match credential_gate::fido2::register(&pin) {
            Ok(c) => c,
            Err(e) => {
                self.status = Status::Error(format!("FIDO2 registration failed: {}", e));
                return;
            }
        };

        let credential_id = hex::encode(&credential.credential_id);

        let hmac_output =
            match credential_gate::fido2::authenticate(&credential.credential_id, &pin) {
                Ok(h) => h,
                Err(e) => {
                    self.status = Status::Error(format!("FIDO2 authentication failed: {}", e));
                    return;
                }
            };

        let key = match qpv2_core::utilities::derive_vault_enc_key(&hmac_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let key_for_account = key.clone();
        let auth_method = AuthMethod::Fido2 {
            credential_id: credential_id.clone(),
        };

        let vault = KeyVault::new(variant);
        if let Err(e) = vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method.clone()) {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(auth_method);
            return;
        }

        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.auth_method = Some(AuthMethod::Fido2 { credential_id });
                self.register_lock_scripts_with_light_client(&self.accounts.clone());
                self.screen = Screen::Unlocked;
                self.status = Status::Info("Wallet created with FIDO2 security key!".to_string());
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.auth_method = Some(AuthMethod::Fido2 { credential_id });
            }
        }
    }

    /// Unlock via FIDO2 hmac-secret, then transition to Unlocked.
    pub(crate) fn unlock_with_fido2(&mut self, credential_id: &str) {
        let cred_bytes = match hex::decode(credential_id) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(format!("Invalid credential ID: {}", e));
                return;
            }
        };

        let pin = match qpv2_core::pinentry::prompt_password(
            "Enter your FIDO2 security key PIN to unlock.",
            "PIN:",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        match credential_gate::fido2::authenticate(&cred_bytes, &pin) {
            Ok(_) => match KeyVault::get_all_sphincs_lock_args() {
                Ok(lock_args) => {
                    self.accounts = lock_args;
                    self.screen = Screen::Unlocked;
                    self.status = Status::None;
                    self.last_poll_time = std::time::Instant::now();
                    self.load_tx_history_from_disk();
                    self.fetch_all_balances();
                    self.fetch_tx_history(true);
                    self.fetch_dao_cells();
                    self.fetch_node_status();
                }
                Err(e) => {
                    self.status = Status::Error(format!("Failed to unlock: {}", e));
                }
            },
            Err(e) => {
                self.status = Status::Error(e);
            }
        }
    }

    /// Derive a new account using FIDO2 hmac-secret.
    pub(crate) fn create_new_account_with_fido2(&mut self, credential_id: &str) {
        let cred_bytes = match hex::decode(credential_id) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(format!("Invalid credential ID: {}", e));
                return;
            }
        };

        let pin = match qpv2_core::pinentry::prompt_password(
            "Enter your FIDO2 security key PIN to create a new account.",
            "PIN:",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let hmac_output = match credential_gate::fido2::authenticate(&cred_bytes, &pin) {
            Ok(h) => h,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let key = match qpv2_core::utilities::derive_vault_enc_key(&hmac_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant);
        match vault.gen_new_account(AuthKey::CryptoKey(key)) {
            Ok(lock_args) => {
                self.balances.insert(lock_args.clone(), None);
                let qp_client = self.qp_client.clone();
                let args = lock_args.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.balance_receiver = Some(rx);
                std::thread::spawn(move || {
                    let result = ckb_node::wallet_helpers::queries::fetch_quantum_lock_balance(
                        &qp_client, &args,
                    )
                    .map_err(|e| e.to_string());
                    let _ = tx.send((args, result));
                });
                self.accounts.push(lock_args.clone());
                self.register_lock_scripts_with_light_client(std::slice::from_ref(&lock_args));
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }
}
