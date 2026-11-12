//! Wallet lifecycle: create, unlock, lock, new account, config.

use qpv2_core::types::{AuthKey, AuthMethod};
use qpv2_core::{KeyVault, SecureString};

use crate::tx_history::TxHistoryStore;
use crate::types::{CurrentWallet, Screen, Status, TransactionStatus};
use crate::App;

mod with_fido2;
mod with_keychain;
mod with_password;
mod with_trezor;

pub(crate) use with_fido2::{get_fido2_dek, spawn_fido2_job};

/// Where light-client registration starts scanning for new scripts.
pub(crate) enum StartingBlock {
    /// Freshly generated key — no history can exist, scan from the tip.
    Tip,
    /// Imported key (mnemonic or device) — history unknown, ask a public
    /// indexer where funding started; fall back to genesis if unreachable.
    Detect,
}

impl App {
    /// Build the wallet cache from disk. Called as an associated function
    /// during `App::new` (before `self` exists) and as a method thereafter.
    pub(crate) fn current_wallet_cache() -> Vec<CurrentWallet> {
        let wallets = KeyVault::list_wallets().unwrap_or_default();
        wallets
            .into_iter()
            .filter_map(|entry| {
                let info = KeyVault::read_wallet_info(entry.id).ok()?;
                let account_count = KeyVault::get_all_lock_args(entry.id)
                    .map(|a| a.len())
                    .unwrap_or(0);
                let path = qpv2_core::db::get_wallet_dir(entry.id)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Some(CurrentWallet {
                    id: entry.id,
                    name: entry.name,
                    spx_variant: info.spx_variant,
                    auth_method: info.auth_method,
                    single_sig_convention: info.single_sig_convention,
                    account_count,
                    path,
                })
            })
            .collect()
    }

    /// Refresh the in-memory wallet cache from disk.
    pub(crate) fn refresh_wallet_cache(&mut self) {
        self.wallet_cache = Self::current_wallet_cache();
    }

    /// Tells the running light client to start indexing the given
    /// accounts from the block `anchor` resolves to, in a single
    /// `set_scripts` RPC call. No-op when the backend isn't LightClient,
    /// no local process is running, or the input is empty — full nodes /
    /// public RPC index everything by default, and a stopped light
    /// client has nothing to register against.
    pub(crate) fn register_lock_scripts_with_light_client(
        &mut self,
        lock_args_list: &[String],
        start: StartingBlock,
    ) {
        if self.qp_client.config().node_type != ckb_node::NodeType::LightClient
            || !self.local_node.has_local_process()
            || lock_args_list.is_empty()
        {
            return;
        }
        let tip = match self.qp_client.get_tip_header() {
            Ok(h) => h.inner.number.value(),
            Err(e) => {
                let msg = format!("Failed to get tip header: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };
        let start_block = match start {
            StartingBlock::Tip => tip,
            StartingBlock::Detect => {
                let network = self.qp_client.config().network;
                let url = ckb_node::NodeConfig::default_rpc_url_for(
                    ckb_node::NodeType::PublicRpc,
                    network,
                );
                let client = ckb_node::client::FullNodeClient::new(url);
                match client.find_earliest_funding_block(lock_args_list, network) {
                    Ok(Some(earliest)) => earliest.saturating_sub(1),
                    Ok(None) => tip,
                    Err(e) => {
                        tracing::error!("earliest-funding detection failed: {}", e);
                        self.status = Status::Info(
                            "Could not detect funding history; scanning from genesis.".to_string(),
                        );
                        0
                    }
                }
            }
        };
        if let Err(e) = ckb_node::wallet_helpers::lc::register_lock_scripts(
            &self.qp_client,
            lock_args_list,
            start_block,
        ) {
            let msg = format!("Failed to register scripts: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
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
        start: StartingBlock,
    ) {
        // Same wipe as `switch_wallet`: creating or importing switches
        // the active wallet too, and the previous wallet's runtime state
        // (balance maps, DAO caches, in-flight receivers) would
        // otherwise bleed into the new wallet's dashboard.
        self.clear_wallet_state();
        self.wallet_id = wallet_id;
        self.wallet_name = wallet_name;
        match KeyVault::get_all_accounts(self.wallet_id) {
            Ok(accounts) => {
                self.accounts = accounts;
                self.auth_method = Some(auth_method);
                let lock_args: Vec<String> =
                    self.accounts.iter().map(|a| a.lock_args.clone()).collect();
                self.screen = Screen::Unlocked;
                self.status = Status::Info(success_msg.to_string());
                self.register_lock_scripts_with_light_client(&lock_args, start);
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
                tracing::info!("Wallet setup finalized (wallet_id={})", self.wallet_id);
            }
            Err(e) => {
                let msg = format!("Failed to read accounts: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
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

        let network = self.qp_client.config().network;
        // Always use the network's public RPC — even if the active
        // backend is already PublicRpc, building a fresh client keeps
        // this a self-contained one-shot.
        let public_rpc_url =
            ckb_node::NodeConfig::default_rpc_url_for(ckb_node::NodeType::PublicRpc, network)
                .to_string();
        let lock_args: Vec<String> = self.accounts.iter().map(|a| a.lock_args.clone()).collect();

        let (tx, rx) = std::sync::mpsc::channel();
        self.earliest_funding_block_rx = Some(rx);

        std::thread::spawn(move || {
            let pub_rpc_client = ckb_node::client::FullNodeClient::new(&public_rpc_url);
            let result = pub_rpc_client
                .find_earliest_funding_block(&lock_args, network)
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
        let lock_args: Vec<String> = self.accounts.iter().map(|a| a.lock_args.clone()).collect();
        if let Err(e) = ckb_node::wallet_helpers::lc::register_all_lock_scripts(
            &self.qp_client,
            &lock_args,
            start_block,
        ) {
            let msg = format!("Failed to set scan block: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
        } else {
            // Reflect the new value in the Synced cell immediately.
            self.node_status.synced_block = Some(start_block);
            self.status = Status::Info(format!("Rescan from block {} requested.", start_block));
        }
    }

    /// Highest confirmed block persisted to the history file. Records
    /// at or below it are final; everything above is provisional and
    /// re-fetched on every sync.
    pub(crate) fn tx_history_watermark(&self) -> u64 {
        self.confirmed_watermark
    }

    /// Wipe all wallet-specific runtime state: accounts, balances,
    /// in-flight receivers, form inputs, and DAO caches. Does NOT
    /// change `screen` or `status` — callers decide what transition
    /// follows.
    fn clear_wallet_state(&mut self) {
        self.accounts.clear();
        self.balances.clear();
        self.spendable_balances.clear();
        self.rename_wallet_id = None;
        self.rename_wallet_buf.clear();
        self.is_import_mode = false;
        self.wallet_modal = crate::types::WalletModal::None;
        self.set_block_editing = false;
        self.set_block_input.clear();

        // Drop all in-flight receivers so background threads from the
        // previous wallet can't land stale results into the new one.
        self.tx_history_rx = None;
        self.balance_receiver = None;
        self.transaction_build_rx = None;
        self.transaction_send_rx = None;
        self.node_status_rx = None;
        self.earliest_funding_block_rx = None;
        // Also cancels a pending Trezor import when the user creates or
        // switches to another wallet instead of finishing it — otherwise its
        // late result would create a second wallet and yank the user into it.
        self.trezor_connect_rx = None;
        // Same for a pending FIDO2 create or import.
        self.fido2_connect_rx = None;
        // And a pending new account: it belongs to the wallet being left.
        self.trezor_new_account_rx = None;
        self.fido2_new_account_rx = None;
        // And a pending security-key signature, co-signature or seed export.
        self.fido2_sign_rx = None;
        self.fido2_cosign_rx = None;
        self.fido2_export_rx = None;
        // Link status belongs to the wallet being left; a late probe result
        // would otherwise describe the old wallet's device.
        self.device_status_rx = None;
        self.device_status_probed_at = None;
        self.device_status_strikes = 0;
        self.device_status_pending = None;
        self.device_status = trezor_connect::DeviceStatus::Working;
        self.trezor_reconnect_rx = None;
        self.device_popup_open = false;

        self.tx_history.clear();
        self.unconfirmed_tx_records.clear();
        self.confirmed_watermark = 0;

        self.transfer_recipient.clear();
        self.transfer_amount.clear();
        self.transfer_all = false;
        self.transfer_from_account = 0;
        self.multisig_local_signer_idx = 0;
        self.dao_deposit_amount.clear();
        self.dao_deposit_all = false;
        self.dao_deposit_from_account = 0;
        self.tx_status = TransactionStatus::Idle;
        // A co-signer response belongs to the wallet that signed it.
        self.cosign_request_json.clear();
        self.cosign_response_json = None;

        self.dao_deposited_cells.clear();
        self.dao_prepared_cells.clear();
        self.dao_deposited_staging.clear();
        self.dao_prepared_staging.clear();
        self.dao_cells_query_rx = None;
        self.deposit_headers.clear();
        self.deposit_headers_rx = None;
    }

    /// Lock the wallet: clear state and return to the Locked screen.
    pub(crate) fn lock_wallet(&mut self) {
        self.clear_wallet_state();
        self.screen = Screen::Locked;
        self.status = Status::None;
        tracing::info!("Wallet locked (wallet_id={})", self.wallet_id);
    }

    /// Cover the open wallet. Nothing is cleared: balances, history and any
    /// work in flight (a Trezor signature, a broadcast) stay, so showing it
    /// again is instant and loses nothing. Popups close so none draws over
    /// the cover, and the status line clears so it reveals nothing.
    pub(crate) fn hide_wallet(&mut self) {
        self.wallet_selector_open = false;
        self.node_selector_open = false;
        self.device_popup_open = false;
        self.screen = Screen::Locked;
        self.status = Status::None;
        tracing::info!("Wallet hidden (wallet_id={})", self.wallet_id);
    }

    /// Uncover the wallet: the one `hide_wallet` covered, or the one the app
    /// started on, whose accounts and first fetch were loaded behind the cover.
    pub(crate) fn show_wallet(&mut self) {
        self.screen = Screen::Unlocked;
        tracing::info!("Wallet shown (wallet_id={})", self.wallet_id);
    }

    /// Trim the wallet name, check it is free, and claim the next wallet ID.
    /// Called before any prompt, so a duplicate name fails early.
    pub(crate) fn prepare_new_wallet(&self) -> Result<(u32, String), String> {
        let trimmed = self.new_wallet_name.trim();
        let name = if trimmed.is_empty() {
            names::Generator::default()
                .next()
                .unwrap_or_else(|| "wallet".to_string())
        } else {
            trimmed.to_string()
        };
        qpv2_core::db::wallets::validate_wallet_name(&name, None).map_err(|e| e.to_string())?;
        let wallet_id = qpv2_core::db::wallets::next_wallet_id().map_err(|e| e.to_string())?;
        Ok((wallet_id, name))
    }

    /// Switch the active wallet. Clears previous wallet state, loads
    /// the new wallet's metadata, and transitions directly to Unlocked.
    pub(crate) fn switch_wallet(&mut self, wallet_id: u32, wallet_name: &str) {
        tracing::info!(
            "Switching wallet (wallet_id={}, name={})",
            wallet_id,
            wallet_name
        );
        self.clear_wallet_state();
        self.wallet_id = wallet_id;
        self.wallet_name = wallet_name.to_string();
        save_last_wallet_id(wallet_id);

        self.auth_method = KeyVault::read_wallet_info(wallet_id)
            .ok()
            .map(|w| w.auth_method);

        self.lc_script_registration_reeval = false;
        self.accounts = KeyVault::get_all_accounts(wallet_id).unwrap_or_default();
        self.screen = Screen::Unlocked;
        self.needs_initial_fetch = true;
        self.wallet_selector_open = false;
        self.refresh_wallet_cache();
    }

    /// Called when the node type or network changes in the UI. Refreshes
    /// `settings_rpc_url` to the default URL for the pending
    /// `(node_type, network)` pair so the form preview matches
    /// what the user is about to commit.
    pub(crate) fn on_node_type_changed(&mut self) {
        self.settings_rpc_url =
            ckb_node::NodeConfig::default_rpc_url_for(self.node_type, self.network).to_string();
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
        new_cfg.node_type = self.node_type;
        new_cfg.network = self.network;
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
            let msg = format!("Failed to save config: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
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
        self.deposit_headers.clear();
        self.deposit_headers_rx = None;

        // Both LC latches are per-instance. Reset so the poller
        // re-warms the cell dep and re-registers lock scripts against
        // the new backend (and skips both for FullNode / PublicRpc).
        self.lc_qr_dep_warmup_done = false;
        self.lc_script_registration_reeval = false;

        // The adoption series is per-network: drop any in-flight
        // fetch, show the new network's cached series immediately, and
        // rebuild against the chain on the next tick.
        self.qr_adoption_rx = None;
        self.qr_adoption_next_fetch = None;
        self.qr_adoption_series =
            crate::qr_adoption::load_series(self.qp_client.config().network.tag());

        // Balances, tx history, and DAO cells are network-scoped but
        // keyed by lock args alone — left in place they keep rendering
        // the previous network's numbers (and the balance chart would
        // replay the previous network's tape against the new total)
        // until a successful fetch overwrites them, which a slow-to-
        // boot local node can delay indefinitely. Drop them together
        // with their in-flight receivers; the per-network tx history
        // reloads from disk and the poller refetches the rest.
        self.balances.clear();
        self.spendable_balances.clear();
        self.balance_receiver = None;
        self.dao_deposited_cells.clear();
        self.dao_prepared_cells.clear();
        self.dao_deposited_staging.clear();
        self.dao_prepared_staging.clear();
        self.dao_cells_query_rx = None;
        self.tx_history.clear();
        self.unconfirmed_tx_records.clear();
        self.tx_history_rx = None;
        self.balance_chart_cache = None;
        self.load_tx_history_from_disk();

        self.status = Status::Info("Configuration saved. RPC reconnected.".to_string());
        tracing::info!(
            "Node config applied (node_type={:?}, network={:?})",
            self.node_type,
            self.network
        );

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
        match TxHistoryStore::load(self.wallet_id, self.qp_client.config().network.tag()) {
            Ok(Some(store)) => {
                // The file holds only confirmed records, so its highest
                // block is the sync floor.
                self.confirmed_watermark = store
                    .records
                    .iter()
                    .map(|r| r.block_number)
                    .max()
                    .unwrap_or(0);
                self.tx_history = store.records;
            }
            Ok(None) => {
                self.tx_history.clear();
                self.confirmed_watermark = 0;
            }
            Err(e) => {
                self.tx_history.clear();
                self.confirmed_watermark = 0;
                let msg = format!("Failed to read cached tx history: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
            }
        }
    }

    /// Resolve the authentication key based on the current auth method.
    /// Synchronous: blocks the egui update loop while the password dialog or
    /// Touch ID prompt is open. A security key is never asked here: it waits
    /// for a touch with nothing on screen, so `get_fido2_dek` asks it on a
    /// worker.
    pub(crate) fn resolve_auth_key(&self, purpose: &str) -> Result<AuthKey, String> {
        match &self.auth_method {
            Some(AuthMethod::Password) => qpv2_core::pinentry::prompt_password(
                &format!("Enter your wallet password to {}.", purpose),
                "Password:",
            )
            .map(AuthKey::Password),
            Some(AuthMethod::Keychain) => {
                keychain::retrieve_key(self.wallet_id, purpose).map(AuthKey::CryptoKey)
            }
            Some(AuthMethod::Fido2 { .. }) => Err("FIDO2 key Not available.".to_string()),
            Some(AuthMethod::Trezor { .. }) => Err("Trezor key not available".to_string()),
            None => Err("No authentication method set.".to_string()),
        }
    }

    pub(crate) fn create_singlesig_account(&mut self) {
        match &self.auth_method {
            Some(AuthMethod::Trezor { .. }) => {
                self.start_create_hw_wallet_account();
                return;
            }
            // A security key waits for a touch, so it runs on a worker; the
            // password and Touch ID dialogs stay inline, as in wallet creation.
            Some(AuthMethod::Fido2 { credential_id }) => {
                let credential_id = credential_id.clone();
                self.start_create_fido2_wallet_account(credential_id);
                return;
            }
            _ => {}
        }

        let auth = match self.resolve_auth_key("create a new account") {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("Failed to read wallet variant: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
        match vault.gen_singlesig_account(auth) {
            Ok(account) => {
                tracing::info!(
                    "Account created (wallet_id={}, lock_args={}...)",
                    self.wallet_id,
                    &account.lock_args[..8.min(account.lock_args.len())]
                );
                self.balances.insert(account.lock_args.clone(), Some(0));
                self.spendable_balances
                    .insert(account.lock_args.clone(), Some(0));
                // A wallet's first account may hold history: an import whose
                // first account failed opens empty, and this makes it.
                let start = if self.accounts.is_empty() {
                    StartingBlock::Detect
                } else {
                    StartingBlock::Tip
                };
                self.register_lock_scripts_with_light_client(
                    std::slice::from_ref(&account.lock_args),
                    start,
                );
                self.accounts.push(account);
                self.refresh_wallet_cache();
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                let msg = format!("Failed to create account: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
            }
        }
    }

    /// Shows the seed phrase on a password or Touch ID wallet: asks for the
    /// key now, decrypts the phrase, then opens the secure dialog.
    pub(crate) fn export_seed_phrase(&mut self) {
        if self.seed_export_dialog_done_rx.is_some() {
            return;
        }
        let auth = match self.resolve_auth_key("export the seed phrase") {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };
        match get_exported_seed_phrase(self.wallet_id, auth) {
            Ok(phrase) => self.show_seed_phrase_dialog(phrase),
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
            }
        }
    }

    /// Opens the secure dialog showing `phrase`.
    pub(crate) fn show_seed_phrase_dialog(&mut self, phrase: SecureString) {
        let wallet_id = self.wallet_id;
        let (tx, rx) = std::sync::mpsc::channel();
        self.seed_export_dialog_done_rx = Some(rx);

        // `phrase` moves into the thread, one owner throughout, and is wiped
        // there when the dialog closes. Only the outcome comes back.
        std::thread::spawn(move || {
            let result = qpv2_core::pinentry::show_seed_phrase(&phrase);
            if result.is_ok() {
                tracing::info!("Seed phrase exported (wallet_id={})", wallet_id);
            }
            let _ = tx.send(result);
        });
    }

    /// True while a Trezor connect or a security-key setup started from the
    /// create / import form waits on its device. The form stays open and
    /// locked meanwhile.
    pub(crate) fn external_wallet_setup_in_flight(&self) -> bool {
        self.trezor_connect_rx.is_some() || self.fido2_connect_rx.is_some()
    }

    /// True while a Trezor or security-key task runs in the background and
    /// the user has to act on the device. The user stays where they are
    /// meanwhile: LOCK, tab switching, wallet switching and New Wallet are
    /// blocked. Leaving would hide the line saying what to do on the device,
    /// and switching or deleting the open wallet would drop the task while
    /// its worker still holds the device.
    pub(crate) fn device_task_in_flight(&self) -> bool {
        self.trezor_operation_in_flight() || self.fido2_operation_in_flight()
    }

    /// Create a multisig account from the modal form state.
    pub(crate) fn create_multisig_account(&mut self) {
        let singlesig: Vec<_> = self
            .accounts
            .iter()
            .filter(|a| a.config.signers().len() == 1)
            .collect();

        let local_lock_args = match singlesig.get(self.multisig_local_signer_idx) {
            Some(a) => a.lock_args.clone(),
            None => {
                self.status = Status::Error("No single-sig account selected.".to_string());
                return;
            }
        };

        let co_signers: Vec<qpv2_core::types::Signer> = match self
            .multisig_co_signers
            .iter()
            .enumerate()
            .map(|(i, (hex, variant))| {
                hex::decode(hex.trim())
                    .map_err(|e| format!("Invalid pubkey hex: {}", e))
                    .and_then(|pubkey| qpv2_core::types::Signer::new(*variant, pubkey))
                    // Numbered as the dialog's cards are.
                    .map_err(|e| format!("Signer {:02}: {}", i + 1, e))
            })
            .collect::<Result<Vec<_>, String>>()
        {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };

        match KeyVault::gen_multisig_account(
            self.wallet_id,
            &local_lock_args,
            co_signers,
            self.multisig_threshold,
            self.multisig_required_first_n,
        ) {
            Ok(account) => {
                tracing::info!(
                    "Multisig account created (wallet_id={}, lock_args={}...)",
                    self.wallet_id,
                    &account.lock_args[..8.min(account.lock_args.len())]
                );
                self.balances.insert(account.lock_args.clone(), Some(0));
                self.spendable_balances
                    .insert(account.lock_args.clone(), Some(0));
                self.register_lock_scripts_with_light_client(
                    std::slice::from_ref(&account.lock_args),
                    StartingBlock::Tip,
                );
                self.accounts.push(account);
                self.refresh_wallet_cache();
                self.status = Status::Info("Multisig account created!".to_string());
            }
            Err(e) => {
                let msg = format!("Failed to create multisig account: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
            }
        }
    }
}

/// Decrypts the wallet's seed phrase with the wallet key `auth`. Touches no
/// UI state, so it can run on a worker.
fn get_exported_seed_phrase(wallet_id: u32, auth: AuthKey) -> Result<SecureString, String> {
    let variant = KeyVault::get_spx_variant(wallet_id)
        .map_err(|e| format!("Failed to read wallet variant: {}", e))?;
    KeyVault::new(variant, wallet_id)
        .export_seed_phrase(auth)
        .map_err(|e| format!("Failed to export seed phrase: {}", e))
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
