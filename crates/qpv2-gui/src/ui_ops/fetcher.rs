//! DAO transaction building, signing, and sending.

use std::sync::mpsc;

use crate::types::DaoQueryEvent;
use crate::App;

impl App {
    /// Kick off background queries for deposited + prepared DAO cells across all accounts.
    pub(crate) fn fetch_dao_cells(&mut self) {
        if self.accounts.is_empty() || self.dao_cells_query_rx.is_some() {
            return;
        }

        // avoid showing duplicated cells from previous poll.
        self.dao_deposited_cells.clear();
        self.dao_prepared_cells.clear();

        let is_mainnet = self.is_mainnet();
        let node_config = self.node_config.clone();
        let all_lock_args: Vec<String> = self.accounts.clone();

        let (tx, rx) = mpsc::channel();
        self.dao_cells_query_rx = Some(rx);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);

            for lock_args in &all_lock_args {
                let address_str =
                    match qpv2_core::utilities::lock_args_to_address(lock_args, is_mainnet) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = tx.send(Err(format!("Invalid address: {}", e)));
                            continue;
                        }
                    };
                let address: ckb_sdk::Address = match address_str.parse() {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(Err(format!("Invalid address: {}", e)));
                        continue;
                    }
                };

                let (deposited, prepared) =
                    match node_manager::categozire_dao_cells(rpc.as_ref(), &address) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = tx.send(Err(format!("Failed to query DAO cells: {}", e)));
                            continue;
                        }
                    };

                for cell in deposited {
                    // If the receiver is dropped (e.g. wallet locked), stop.
                    if tx
                        .send(Ok(DaoQueryEvent::Deposited(lock_args.clone(), cell)))
                        .is_err()
                    {
                        return;
                    }
                }

                for cell in prepared {
                    // If the receiver is dropped (e.g. wallet locked), stop.
                    if tx
                        .send(Ok(DaoQueryEvent::Prepared(lock_args.clone(), cell)))
                        .is_err()
                    {
                        return;
                    }
                }
            }

            let _ = tx.send(Ok(DaoQueryEvent::Done));
        });
    }

    /// Fetch balances for all accounts in a background thread.
    pub(crate) fn fetch_all_balances(&mut self) {
        if self.rpc_client.is_none() || self.balance_receiver.is_some() {
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
                let result =
                    node_manager::fetch_quantum_lock_balance(rpc.as_ref(), &lock_args, network)
                        .map_err(|e| e.to_string());
                // If the receiver is dropped (e.g. wallet locked), stop.
                if tx.send((lock_args, result)).is_err() {
                    break;
                }
            }
        });
    }
}
