//! Wallet lifecycle: create, unlock, lock, new account, config.

use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use crate::tx_history::TxHistoryStore;
use crate::types::{CurrentWallet, Screen, Status, TransactionStatus};
use crate::App;

impl App {
    /// Build the wallet cache from disk. Called as an associated function
    /// during `App::new` (before `self` exists) and as a method thereafter.
    pub(crate) fn current_wallet_cache() -> Vec<CurrentWallet> {
        let wallets = KeyVault::list_wallets().unwrap_or_default();
        wallets
            .into_iter()
            .filter_map(|entry| {
                let info = KeyVault::read_wallet_info(entry.id).ok()?;
                let account_count = KeyVault::get_all_sphincs_lock_args(entry.id)
                    .map(|a| a.len())
                    .unwrap_or(0);
                Some(CurrentWallet {
                    id: entry.id,
                    name: entry.name,
                    spx_variant: info.spx_variant,
                    auth_method: info.auth_method,
                    account_count,
                })
            })
            .collect()
    }

    /// Refresh the in-memory wallet cache from disk.
    pub(crate) fn refresh_wallet_cache(&mut self) {
        self.wallet_cache = Self::current_wallet_cache();
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
    pub(crate) fn register_lock_scripts_with_light_client(&mut self, lock_args_list: &[String]) {
        if self.qp_client.config().node_type != ckb_node::NodeType::LightClient
            || !self.local_node.has_local_process()
            || lock_args_list.is_empty()
        {
            return;
        }
        let start_block = match self.qp_client.get_tip_header() {
            Ok(h) => h.inner.number.value(),
            Err(e) => {
                self.status = Status::Error(format!("Failed to get tip header: {}", e));
                return;
            }
        };
        if let Err(e) = ckb_node::wallet_helpers::lc::register_lock_scripts(
            &self.qp_client,
            lock_args_list,
            start_block,
        ) {
            self.status = Status::Error(format!("Failed to register scripts: {}", e));
        }
    }

    /// Common transition from Setup to Unlocked after successful wallet
    /// creation or import. Loads accounts, registers LC scripts, sets
    /// the screen, and kicks off all background fetches.
    fn finalize_wallet_setup(
        &mut self,
        auth_method: AuthMethod,
        success_msg: &str,
        wallet_id: u32,
        wallet_name: String,
    ) {
        self.wallet_id = wallet_id;
        self.wallet_name = wallet_name;
        match KeyVault::get_all_sphincs_lock_args(self.wallet_id) {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.auth_method = Some(auth_method);
                self.register_lock_scripts_with_light_client(&self.accounts.clone());
                self.screen = Screen::Unlocked;
                self.status = Status::Info(success_msg.to_string());
                self.last_poll_time = std::time::Instant::now();
                self.new_wallet_name.clear();
                self.wallet_selector_open = false;
                save_last_wallet_id(self.wallet_id);
                self.refresh_wallet_cache();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.auth_method = Some(auth_method);
            }
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

    /// Wipe all wallet-specific runtime state: accounts, balances,
    /// in-flight receivers, form inputs, and DAO caches. Does NOT
    /// change `screen` or `status` — callers decide what transition
    /// follows.
    fn clear_wallet_state(&mut self) {
        self.accounts.clear();
        self.balances.clear();
        self.confirm_remove_id = None;
        self.import_mode = false;

        // Drop all in-flight receivers so background threads from the
        // previous wallet can't land stale results into the new one.
        self.tx_history_rx = None;
        self.balance_receiver = None;
        self.spendable_capacity_rx = None;
        self.transaction_build_rx = None;
        self.transaction_send_rx = None;
        self.node_status_rx = None;
        self.earliest_funding_block_rx = None;

        self.tx_history.clear();

        self.transfer_recipient.clear();
        self.transfer_amount.clear();
        self.transfer_all = false;
        self.transfer_from_account = 0;
        self.dao_deposit_amount.clear();
        self.dao_deposit_all = false;
        self.dao_deposit_from_account = 0;
        self.tx_status = TransactionStatus::Idle;

        self.dao_deposited_cells.clear();
        self.dao_prepared_cells.clear();
        self.dao_deposited_staging.clear();
        self.dao_prepared_staging.clear();
        self.dao_cells_query_rx = None;
    }

    /// Lock the wallet: clear state and return to the Locked screen.
    pub(crate) fn lock_wallet(&mut self) {
        self.clear_wallet_state();
        self.screen = Screen::Locked;
        self.status = Status::None;
    }

    /// Validate the wallet name and claim the next available wallet ID.
    /// Returns the pair without modifying `self` — the caller threads
    /// them through creation calls and only commits via
    /// `finalize_wallet_setup` on success.
    pub(crate) fn prepare_new_wallet(&self) -> Result<(u32, String), String> {
        let name = self.new_wallet_name.trim().to_string();
        if name.is_empty() {
            return Err("Wallet name is required.".to_string());
        }
        let wallets = KeyVault::list_wallets().map_err(|e| e.to_string())?;
        if wallets.iter().any(|w| w.name == name) {
            return Err(format!("Wallet '{}' already exists.", name));
        }
        let wallet_id = qpv2_core::db::wallets::next_wallet_id().map_err(|e| e.to_string())?;
        Ok((wallet_id, name))
    }

    /// Switch the active wallet. Clears previous wallet state, loads
    /// the new wallet's metadata, and transitions directly to Unlocked.
    pub(crate) fn switch_wallet(&mut self, wallet_id: u32, wallet_name: &str) {
        self.clear_wallet_state();
        self.wallet_id = wallet_id;
        self.wallet_name = wallet_name.to_string();
        save_last_wallet_id(wallet_id);

        self.auth_method = KeyVault::read_wallet_info(wallet_id)
            .ok()
            .map(|w| w.auth_method);

        self.lc_scripts_registered = false;
        self.accounts = KeyVault::get_all_sphincs_lock_args(wallet_id).unwrap_or_default();
        self.screen = Screen::Unlocked;
        self.needs_initial_fetch = true;
        self.wallet_selector_open = false;
        self.refresh_wallet_cache();
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

        // Both LC latches are per-instance. Reset so the poller
        // re-warms the cell dep and re-registers lock scripts against
        // the new backend (and skips both for FullNode / PublicRpc).
        self.lc_qr_dep_warmup_done = false;
        self.lc_scripts_registered = false;

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
        match TxHistoryStore::load(self.wallet_id, self.qp_client.network().tag()) {
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
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
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

        let pw_for_account = pw.clone();
        let vault = KeyVault::new(variant, wallet_id);
        if let Err(e) =
            vault.generate_master_seed(AuthKey::Password(pw), AuthMethod::Password, &wallet_name)
        {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::Password(pw_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(AuthMethod::Password);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Password,
            &format!("Wallet created successfully!{}", strength_str),
            wallet_id,
            wallet_name,
        );
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

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
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

    pub(crate) fn import_seed_phrase_with_password(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let seed_phrase = match qpv2_core::pinentry::prompt_seed_phrase(variant) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let pw = match qpv2_core::pinentry::prompt_password_with_confirmation(
            "Choose a password for your imported wallet. You'll be prompted for it \
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

        let pw_for_account = pw.clone();
        let vault = KeyVault::new(variant, wallet_id);

        if let Err(e) = vault.import_seed_phrase(
            seed_phrase,
            AuthKey::Password(pw),
            AuthMethod::Password,
            &wallet_name,
        ) {
            self.status = Status::Error(format!("Failed to import wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::Password(pw_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(AuthMethod::Password);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Password,
            &format!("Wallet imported successfully!{}", strength_str),
            wallet_id,
            wallet_name,
        );
    }

    pub(crate) fn export_seed_phrase_with_password(&mut self) {
        let pw = match qpv2_core::pinentry::prompt_password(
            "Enter your wallet password to export the seed phrase.",
            "Password:",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
        match vault.export_seed_phrase(AuthKey::Password(pw)) {
            Ok(phrase) => {
                if let Err(e) = qpv2_core::pinentry::show_seed_phrase(&phrase) {
                    self.status = Status::Error(e);
                }
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to export seed phrase: {}", e));
            }
        }
    }

    /// Create a Keychain wallet. Generates a random 32-byte key,
    /// stores it in the platform credential store, then creates the
    /// wallet and first account.
    pub(crate) fn create_wallet_with_keychain(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let key = match qpv2_core::utilities::get_random_bytes(32) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(format!("Failed to generate key: {}", e));
                return;
            }
        };

        if let Err(e) = keychain::store_key(wallet_id, &key) {
            self.status = Status::Error(format!("Failed to store key in Keychain: {}", e));
            return;
        }

        let key_for_account = key.clone();

        let vault = KeyVault::new(variant, wallet_id);
        if let Err(e) =
            vault.generate_master_seed(AuthKey::CryptoKey(key), AuthMethod::Keychain, &wallet_name)
        {
            let _ = keychain::delete_key(wallet_id);
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(AuthMethod::Keychain);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Keychain,
            &format!("Wallet created with {}!", keychain::short_name()),
            wallet_id,
            wallet_name,
        );
    }

    /// Unlock via the platform credential store, then transition to
    /// Unlocked.
    pub(crate) fn unlock_with_keychain(&mut self) {
        match keychain::retrieve_key(self.wallet_id) {
            Ok(_) => match KeyVault::get_all_sphincs_lock_args(self.wallet_id) {
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
        let key = match keychain::retrieve_key(self.wallet_id) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
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

    pub(crate) fn import_seed_phrase_with_keychain(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let seed_phrase = match qpv2_core::pinentry::prompt_seed_phrase(variant) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let key = match qpv2_core::utilities::get_random_bytes(32) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(format!("Failed to generate key: {}", e));
                return;
            }
        };

        if let Err(e) = keychain::store_key(wallet_id, &key) {
            self.status = Status::Error(format!("Failed to store key in Keychain: {}", e));
            return;
        }

        let key_for_account = key.clone();
        let vault = KeyVault::new(variant, wallet_id);

        if let Err(e) = vault.import_seed_phrase(
            seed_phrase,
            AuthKey::CryptoKey(key),
            AuthMethod::Keychain,
            &wallet_name,
        ) {
            let _ = keychain::delete_key(wallet_id);
            self.status = Status::Error(format!("Failed to import wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(AuthMethod::Keychain);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Keychain,
            &format!("Wallet imported with {}!", keychain::short_name()),
            wallet_id,
            wallet_name,
        );
    }

    pub(crate) fn export_seed_phrase_with_keychain(&mut self) {
        let key = match keychain::retrieve_key(self.wallet_id) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
        match vault.export_seed_phrase(AuthKey::CryptoKey(key)) {
            Ok(phrase) => {
                if let Err(e) = qpv2_core::pinentry::show_seed_phrase(&phrase) {
                    self.status = Status::Error(e);
                }
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to export seed phrase: {}", e));
            }
        }
    }

    /// Create a FIDO2-authenticated wallet. Prompts for the device PIN
    /// via pinentry, registers a credential, then derives the encryption
    /// key via hmac-secret.
    pub(crate) fn create_wallet_with_fido2(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
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

        let credential = match keychain::fido2::register(&pin) {
            Ok(c) => c,
            Err(e) => {
                self.status = Status::Error(format!("FIDO2 registration failed: {}", e));
                return;
            }
        };

        let credential_id = hex::encode(&credential.credential_id);

        let hmac_output = match keychain::fido2::authenticate(&credential.credential_id, &pin) {
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

        let vault = KeyVault::new(variant, wallet_id);
        if let Err(e) =
            vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method.clone(), &wallet_name)
        {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(auth_method);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Fido2 { credential_id },
            "Wallet created with FIDO2 security key!",
            wallet_id,
            wallet_name,
        );
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

        match keychain::fido2::authenticate(&cred_bytes, &pin) {
            Ok(_) => match KeyVault::get_all_sphincs_lock_args(self.wallet_id) {
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

        let hmac_output = match keychain::fido2::authenticate(&cred_bytes, &pin) {
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

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
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

    pub(crate) fn import_seed_phrase_with_fido2(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let seed_phrase = match qpv2_core::pinentry::prompt_seed_phrase(variant) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

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

        let credential = match keychain::fido2::register(&pin) {
            Ok(c) => c,
            Err(e) => {
                self.status = Status::Error(format!("FIDO2 registration failed: {}", e));
                return;
            }
        };

        let credential_id = hex::encode(&credential.credential_id);

        let hmac_output = match keychain::fido2::authenticate(&credential.credential_id, &pin) {
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
        let vault = KeyVault::new(variant, wallet_id);

        if let Err(e) = vault.import_seed_phrase(
            seed_phrase,
            AuthKey::CryptoKey(key),
            auth_method.clone(),
            &wallet_name,
        ) {
            self.status = Status::Error(format!("Failed to import wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key_for_account)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.auth_method = Some(auth_method);
            return;
        }

        self.finalize_wallet_setup(
            AuthMethod::Fido2 { credential_id },
            "Wallet imported with FIDO2 security key!",
            wallet_id,
            wallet_name,
        );
    }

    pub(crate) fn export_seed_phrase_with_fido2(&mut self, credential_id: &str) {
        let pin = match qpv2_core::pinentry::prompt_password(
            "Enter your FIDO2 security key PIN to export the seed phrase.",
            "PIN:",
        ) {
            Ok(s) => s,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        let cred_bytes = match hex::decode(credential_id) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Error(format!("Invalid credential ID: {}", e));
                return;
            }
        };

        let hmac_output = match keychain::fido2::authenticate(&cred_bytes, &pin) {
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

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
        match vault.export_seed_phrase(AuthKey::CryptoKey(key)) {
            Ok(phrase) => {
                if let Err(e) = qpv2_core::pinentry::show_seed_phrase(&phrase) {
                    self.status = Status::Error(e);
                }
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to export seed phrase: {}", e));
            }
        }
    }
}

// ── Last-wallet persistence ──

const LAST_WALLET_FILE: &str = "last_wallet.json";

pub(crate) fn save_last_wallet_id(wallet_id: u32) {
    if let Ok(dir) = qpv2_core::db::get_data_dir() {
        let path = dir.join(LAST_WALLET_FILE);
        let json = format!("{{\"wallet_id\":{}}}", wallet_id);
        let _ = std::fs::write(path, json);
    }
}

pub(crate) fn load_last_wallet_id() -> Option<u32> {
    let dir = qpv2_core::db::get_data_dir().ok()?;
    let path = dir.join(LAST_WALLET_FILE);
    let data = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    v.get("wallet_id")?.as_u64().map(|id| id as u32)
}
