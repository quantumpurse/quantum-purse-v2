//! Wallet lifecycle: create, unlock, lock, new account, config.

use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use crate::passkey::{PRF_SALT, RP_ID};
use crate::tx_history_store::TxHistoryStore;
use crate::types::{PasskeyOp, Screen, Status, Tab, TransactionStatus};
use crate::App;

impl App {
    /// Whether the app is configured for CKB mainnet (derived from node config).
    pub(crate) fn is_mainnet(&self) -> bool {
        self.node_manager.is_mainnet()
    }

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
    pub(crate) fn register_lock_scripts_with_light_client(
        &mut self,
        lock_args_list: &[String],
    ) {
        if self.node_manager.config().node_type != node_manager::NodeType::LightClient
            || !self.node_manager.has_local_process()
            || lock_args_list.is_empty()
        {
            return;
        }
        if let Err(e) = self.node_manager.register_lock_scripts(lock_args_list) {
            self.status = Status::Error(format!("Failed to register scripts: {}", e));
        }
    }

    /// Kicks off a background detection of the earliest funding block
    /// across all accounts via an ad-hoc `FullNodeRpc` against the
    /// public RPC endpoint for the active network. Result lands in
    /// `earliest_funding_block_rx`; the poller writes it into
    /// `set_block_input`. No-op when accounts is empty or another
    /// detection is already in flight.
    pub(crate) fn detect_earliest_funding_block_async(&mut self) {
        if self.earliest_funding_block_rx.is_some() || self.accounts.is_empty() {
            return;
        }

        let network = self.node_manager.network();
        // Always use the network's public RPC — even if the active
        // backend is already PublicRpc, building a fresh client keeps
        // this a self-contained one-shot.
        let public_rpc_url =
            node_manager::NodeConfig::default_rpc_url_for(node_manager::NodeType::PublicRpc, network)
                .to_string();
        let accounts = self.accounts.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        self.earliest_funding_block_rx = Some(rx);

        std::thread::spawn(move || {
            let pub_rpc = node_manager::rpc::FullNodeRpc::new(&public_rpc_url);
            let result = pub_rpc
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
        if self.node_manager.config().node_type != node_manager::NodeType::LightClient
            || !self.node_manager.has_local_process()
            || self.accounts.is_empty()
        {
            return;
        }
        let accounts = self.accounts.clone();
        if let Err(e) = self
            .node_manager
            .register_all_lock_scripts(&accounts, start_block)
        {
            self.status = Status::Error(format!("Failed to set scan block: {}", e));
        } else {
            // Reflect the new value in the Synced cell immediately.
            self.node_status.synced_block = Some(start_block);
            self.status = Status::Info(format!(
                "Rescan from block {} requested.",
                start_block
            ));
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
        self.settings_rpc_url = node_manager::NodeConfig::default_rpc_url_for(
            self.temp_node_type,
            self.temp_network,
        )
        .to_string();
    }

    /// Apply settings edits, persist to disk, and rebuild `NodeManager`.
    ///
    /// Builds the new config from the settings-buffer fields
    /// (`settings_*`, `temp_*`) on top of the current
    /// `node_manager.config()` snapshot — there is no separate mutable
    /// `node_config` on `App`, so this is the only place where an edit
    /// becomes committed state.
    pub(crate) fn save_node_config(&mut self) {
        let mut new_cfg = self.node_manager.config().clone();
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

        // Replace the manager with one bound to the newly-saved config.
        self.node_manager = node_manager::NodeManager::new(new_cfg);
        self.status = Status::Info("Configuration saved. RPC reconnected.".to_string());

        // Refresh balances with new connection.
        self.fetch_all_balances();
    }

    /// Kick off async passkey registration.
    pub(crate) fn create_wallet_start(&mut self, frame: &mut eframe::Frame) {
        let window = match crate::window_handle::get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                return;
            }
        };

        // TODO users must specify these info.
        let user_id = b"qpv2-user";
        let user_name = "tea";

        match passkey_prf::register_passkey_async(&window, RP_ID, user_id, user_name) {
            Ok(op) => {
                self.passkey_op = Some(PasskeyOp::Registration {
                    op,
                    variant: self.selected_variant,
                    window,
                });
            }
            Err(e) => {
                self.status = Status::Error(format!("Passkey registration failed: {}", e));
            }
        }
    }

    /// Complete wallet creation after receiving the PRF output.
    pub(crate) fn create_wallet_finish(
        &mut self,
        variant: SpxVariant,
        credential_id: &[u8],
        prf_output: &qpv2_core::SecureVec,
    ) {
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let vault = KeyVault::new(variant);
        let auth_method = AuthMethod::PasskeyPrf {
            credential_id: credential_id.to_vec(),
        };
        if let Err(e) = vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method) {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        // Re-derive key to generate the first account.
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                self.screen = Screen::Locked;
                return;
            }
        };
        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.screen = Screen::Locked;
            return;
        }

        // Read lock args from accounts.json (no decryption needed).
        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                // First account of a brand-new wallet — if a light
                // client is running, start indexing it from the tip.
                self.register_lock_scripts_with_light_client(&self.accounts.clone());
                self.screen = Screen::Unlocked;
                self.status = Status::Info("Wallet created successfully!".to_string());
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.screen = Screen::Locked;
            }
        }
    }

    /// Seeds `tx_history` from the active network's on-disk cache so the
    /// dashboard renders instantly on unlock instead of waiting for the
    /// first sync tick. The incremental-sync floor (`tx_history_watermark`)
    /// is derived from the loaded records. Silent on absence (fresh wallet
    /// or first time on this network) or read failure (corrupted file →
    /// surfaces as a status warning; next sync rebuilds from scratch).
    pub(crate) fn load_tx_history_from_disk(&mut self) {
        match TxHistoryStore::load(self.node_manager.network().tag()) {
            Ok(Some(store)) => {
                self.tx_history = store.records;
            }
            Ok(None) => {
                self.tx_history.clear();
            }
            Err(e) => {
                self.tx_history.clear();
                self.status =
                    Status::Error(format!("Failed to read cached tx history: {}", e));
            }
        }
    }

    /// Kick off async credential-only assertion (no PRF) for unlock.
    pub(crate) fn unlock_start(&mut self, frame: &mut eframe::Frame) {
        let window = match crate::window_handle::get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                return;
            }
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => return,
        };

        match passkey_prf::assert_async(&window, RP_ID, &credential_id, None) {
            Ok(op) => {
                self.passkey_op = Some(PasskeyOp::UnlockAssert { op });
            }
            Err(passkey_prf::PrfError::Cancelled) => {
                self.status = Status::Info("Cancelled.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Credential assertion failed: {}", e));
            }
        }
    }

    /// Complete wallet unlock after credential assertion succeeds.
    pub(crate) fn unlock_finish(&mut self) {
        match KeyVault::get_all_sphincs_lock_args() {
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
        }
    }

    /// Kick off async PRF assertion to create a new account (requires seed decryption).
    pub(crate) fn create_new_account_start(&mut self, frame: &mut eframe::Frame) {
        let window = match crate::window_handle::get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                return;
            }
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => return,
        };

        match passkey_prf::assert_async(&window, RP_ID, &credential_id, Some(PRF_SALT)) {
            Ok(op) => {
                self.passkey_op = Some(PasskeyOp::NewAccountAssert { op });
                self.status = Status::Info("Authenticate with Touch ID...".to_string());
            }
            Err(passkey_prf::PrfError::Cancelled) => {
                self.status = Status::Info("Cancelled.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("PRF assertion failed: {}", e));
            }
        }
    }

    /// Complete new account creation after receiving the PRF output.
    pub(crate) fn create_new_account_finish(&mut self, prf_output: &qpv2_core::SecureVec) {
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
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
                // Mark as loading and fetch balance in the background.
                self.balances.insert(lock_args.clone(), None);
                let nm = self.node_manager.clone();
                let args = lock_args.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.balance_receiver = Some(rx);

                std::thread::spawn(move || {
                    let result = nm
                        .fetch_quantum_lock_balance(&args)
                        .map_err(|e| e.to_string());
                    let _ = tx.send((args, result));
                });
                self.accounts.push(lock_args.clone());
                // Register only the new account with the light client
                // (no-op on other backends). Don't pass all accounts
                // here — `set_scripts(Partial)` would overwrite the
                // sync cursors of the existing ones.
                self.register_lock_scripts_with_light_client(std::slice::from_ref(&lock_args));
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }
}
