//! Wallet lifecycle: lock, config, balance fetching.

use std::sync::mpsc;

use crate::types::{Screen, Status, Tab};
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
    }

    /// Called when the node type dropdown changes in settings.
    #[allow(dead_code)]
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

    /// Fetch balances for all accounts in a background thread.
    pub(crate) fn fetch_all_balances(&mut self) {
        if self.rpc_client.is_none() {
            return;
        }

        // Mark all accounts as loading.
        for lock_args in &self.accounts {
            self.balances.insert(lock_args.clone(), None);
        }

        let accounts = self.accounts.clone();
        if accounts.is_empty() {
            return;
        }

        let node_config = self.node_config.clone();
        let network = self.node_config.network;
        let (tx, rx) = mpsc::channel();
        self.balance_receiver = Some(rx);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);
            for lock_args in accounts {
                let result = node_manager::fetch_quantum_lock_balance(
                    rpc.as_ref(),
                    &lock_args,
                    network,
                )
                .map_err(|e| e.to_string());
                // If the receiver is dropped (e.g. wallet locked), stop.
                if tx.send((lock_args, result)).is_err() {
                    break;
                }
            }
        });
    }

    /// Drain available balance results from the background thread.
    pub(crate) fn poll_balance_results(&mut self) {
        let rx = match &self.balance_receiver {
            Some(rx) => rx,
            None => return,
        };

        // fetching all available results from the mpsc::channel's buffer.
        loop {
            match rx.try_recv() {
                Ok((lock_args, Ok(balance))) => {
                    self.balances.insert(lock_args, Some(balance));
                }
                Ok((lock_args, Err(e))) => {
                    self.balances.insert(lock_args, None);
                    self.status = Status::Error(format!("Failed to fetch balance: {}", e));
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Background thread finished; drop the receiver.
                    self.balance_receiver = None;
                    break;
                }
            }
        }
    }
}
