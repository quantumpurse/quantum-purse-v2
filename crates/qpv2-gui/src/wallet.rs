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
        self.node_config.network == node_manager::NetworkType::Mainnet
    }

    /// Short tag identifying the active network. Used to namespace
    /// per-network caches like `tx_history_{tag}.json`.
    pub(crate) fn network_tag(&self) -> &'static str {
        match self.node_config.network {
            node_manager::NetworkType::Mainnet => "mainnet",
            node_manager::NetworkType::Testnet => "testnet",
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

    /// Called when the node type dropdown changes in settings.
    pub(crate) fn on_node_type_changed(&mut self) {
        let default_url = self.node_config.default_rpc_url().to_string();
        self.node_config.rpc_url = default_url.clone();
        self.settings_rpc_url = default_url;
    }

    /// Apply settings edits, save config to disk, and reconnect the RPC client.
    pub(crate) fn save_node_config(&mut self) {
        self.node_config.rpc_url = self.settings_rpc_url.clone();

        if self.node_config.requires_binary() && !self.settings_binary_path.is_empty() {
            self.node_config.binary_path = Some(self.settings_binary_path.clone().into());
        } else if !self.node_config.requires_binary() {
            self.node_config.binary_path = None;
        }

        if !self.settings_data_dir.is_empty() {
            self.node_config.data_dir = self.settings_data_dir.clone().into();
        }

        if let Err(e) = self.node_config.save() {
            self.status = Status::Error(format!("Failed to save config: {}", e));
            return;
        }

        // Rebuild the node manager with the new config.
        self.node_manager = node_manager::NodeManager::new(self.node_config.clone());
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
                self.screen = Screen::Unlocked;
                self.status = Status::Info("Wallet created successfully!".to_string());
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
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
        match TxHistoryStore::load(self.network_tag()) {
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
                self.accounts.push(lock_args);
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }
}
