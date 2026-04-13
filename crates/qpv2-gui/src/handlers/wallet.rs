//! Wallet lifecycle: lock, config, balance fetching.
use crate::types::{Screen, Status, Tab, TransactionStatus};
use crate::App;

impl App {
    /// Whether the app is configured for CKB mainnet (derived from node config).
    pub(crate) fn is_mainnet(&self) -> bool {
        self.node_config.network == node_manager::NetworkType::Mainnet
    }

    /// Lock the wallet: clear sensitive state and return to the Locked screen.
    pub(crate) fn lock_wallet(&mut self) {
        self.accounts.clear();
        self.balances.clear();
        self.confirm_remove = false;
        self.rpc_client = None;
        self.active_tab = Tab::Dashboard;
        self.screen = Screen::Locked;
        self.status = Status::None;

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

        // Reconnect RPC client.
        self.rpc_client = Some(node_manager::connect(&self.node_config));
        self.status = Status::Info("Configuration saved. RPC reconnected.".to_string());

        // Refresh balances with new connection.
        self.fetch_all_balances();
    }

    /// Connect to the RPC endpoint and fetch balances for all accounts.
    pub(crate) fn connect_and_fetch_balances(&mut self) {
        self.rpc_client = Some(node_manager::connect(&self.node_config));
        self.fetch_all_balances();
    }
}
