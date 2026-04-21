//! Background data fetchers (balances, DAO cells, spendable capacity, tx history).

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use crate::types::{
    DaoQueryEvent, SpendableCapacityTarget, TransactionStatus, TxHistoryEvent, TxKind, TxRecord,
};
use crate::App;

impl App {
    /// Kick off background queries for deposited + prepared DAO cells across all accounts.
    pub(crate) fn fetch_dao_cells(&mut self) {
        if self.accounts.is_empty() || self.dao_cells_query_rx.is_some() {
            return;
        }

        self.dao_deposited_staging.clear();
        self.dao_prepared_staging.clear();

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

    /// Fetch the total spendable capacity for an account in a background thread.
    /// The `target` determines which account index to use and where to route the result.
    pub(crate) fn fetch_spendable_capacity(&mut self, target: SpendableCapacityTarget) {
        if self.accounts.is_empty() {
            self.tx_status = TransactionStatus::Error("No accounts available.".to_string());
            return;
        }
        if self.spendable_capacity_rx.is_some() {
            return;
        }

        let from_idx = match target {
            SpendableCapacityTarget::Transfer => self.transfer_from_account,
            SpendableCapacityTarget::DaoDeposit => self.dao_deposit_from_account,
        }
        .min(self.accounts.len() - 1);
        let lock_args = self.accounts[from_idx].clone();

        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let node_config = self.node_config.clone();
        let (tx, rx) = mpsc::channel();
        self.spendable_capacity_rx = Some((target, rx));

        std::thread::spawn(move || {
            let result = (|| -> Result<u64, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                node_manager::spendable_capacity(rpc.as_ref(), &from_address)
                    .map_err(|e| format!("Failed to fetch spendable capacity: {}", e))
            })();

            let _ = tx.send(result);
        });
    }

    /// Fetch recent transaction history for all accounts in a background thread.
    ///
    /// When `incremental` is false (cold start), clears existing records and fetches. When true, 
    /// only fetches transactions newer than the highest confirmed block already in the list.
    pub(crate) fn fetch_tx_history(&mut self, incremental: bool) {
        if self.accounts.is_empty() || self.tx_history_rx.is_some() {
            return;
        }

        // In incremental mode, only fetch transactions after the latest known block.
        let after_block = if incremental {
            self.tx_history
                .iter()
                .filter(|r| !r.is_pending)
                .map(|r| r.block_number)
                .max()
        } else {
            self.tx_history.clear();
            None
        };

        let node_config = self.node_config.clone();
        let network = self.node_config.network;
        let all_lock_args: Vec<String> = self.accounts.clone();

        let (sender, rx) = mpsc::channel();
        self.tx_history_rx = Some(rx);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);

            // DAO type script code hash for classification.
            let dao_type_hash = format!("{:#x}", ckb_sdk::constants::DAO_TYPE_HASH);

            // Wallet lock script code hash for filtering outputs that belong to us.
            let wallet_code_hash = match network {
                node_manager::NetworkType::Mainnet => qpv2_core::constants::CKB_MAINNET_CODE_HASH,
                node_manager::NetworkType::Testnet => qpv2_core::constants::CKB_TESTNET_CODE_HASH,
            };
            let all_lock_args_set: HashSet<&str> =
                all_lock_args.iter().map(|s| s.as_str()).collect();

            // Extract the lock_args hex from an output if it matches the
            // wallet's code hash. Returns None for external outputs.
            fn wallet_lock_args(
                out: &ckb_jsonrpc_types::CellOutput,
                code_hash: &str,
                args_set: &HashSet<&str>,
            ) -> Option<String> {
                let ch = format!("{:#x}", out.lock.code_hash);
                if ch != code_hash {
                    return None;
                }
                let args_hex = hex::encode(out.lock.args.as_bytes());
                if args_set.contains(args_hex.as_str()) {
                    Some(args_hex)
                } else {
                    None
                }
            }

            // Collect all tx entries across accounts, grouped by tx_hash.
            // Per-account IO tracking: which accounts have inputs/outputs in each tx.
            struct TxInfo {
                block_number: u64,
                input_accounts: HashSet<String>,
                output_accounts: HashSet<String>,
                /// First account to encounter this tx (used as primary owner).
                owner_lock_args: String,
            }
            let mut seen: HashMap<String, TxInfo> = HashMap::new();

            for lock_args in &all_lock_args {
                // With group_by_transaction=true, each result is one unique tx.
                // Paginates through all results; merged and deduped across accounts.
                let txs = match node_manager::fetch_recent_transactions(
                    rpc.as_ref(),
                    lock_args,
                    network,
                    after_block,
                    None,
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = sender.send(Err(format!("Failed to fetch tx history: {}", e)));
                        continue;
                    }
                };

                for tx_entry in txs {
                    let tx_hash = format!("{:#x}", tx_entry.tx_hash());
                    let info = seen.entry(tx_hash).or_insert_with(|| TxInfo {
                        block_number: 0,
                        input_accounts: HashSet::new(),
                        output_accounts: HashSet::new(),
                        owner_lock_args: lock_args.clone(),
                    });

                    let mut record_io = |cell_type: &node_manager::CellType| match cell_type {
                        node_manager::CellType::Input => {
                            info.input_accounts.insert(lock_args.clone());
                        }
                        node_manager::CellType::Output => {
                            info.output_accounts.insert(lock_args.clone());
                        }
                    };

                    match tx_entry {
                        node_manager::Tx::Grouped(ref grouped) => {
                            info.block_number = grouped.block_number.value();
                            for (cell_type, _idx) in &grouped.cells {
                                record_io(cell_type);
                            }
                        }
                        node_manager::Tx::Ungrouped(ref cell) => {
                            info.block_number = cell.block_number.value();
                            record_io(&cell.io_type);
                        }
                    }
                }
            }

            // Cache block headers to avoid redundant RPC calls.
            let mut header_cache: HashMap<ckb_types::H256, u64> = HashMap::new();

            // Process each unique transaction, sorted newest first.
            let mut tx_list: Vec<(String, TxInfo)> = seen.into_iter().collect();
            tx_list.sort_by(|a, b| b.1.block_number.cmp(&a.1.block_number));

            for (tx_hash_str, tx_info) in tx_list {
                let block_number = tx_info.block_number;
                let owner_lock_args = tx_info.owner_lock_args;
                // Use the owner's specific IO role, not a global merge.
                let has_input = tx_info.input_accounts.contains(&owner_lock_args);
                let has_output = tx_info.output_accounts.contains(&owner_lock_args);

                // For incoming internal transfers: the sender is a different
                // wallet account found in the input side.
                let sender_account: Option<String> = tx_info
                    .input_accounts
                    .iter()
                    .find(|a| a.as_str() != owner_lock_args)
                    .cloned();
                let tx_hash_clean = tx_hash_str.strip_prefix("0x").unwrap_or(&tx_hash_str);
                let tx_hash_bytes = match hex::decode(tx_hash_clean) {
                    Ok(b) if b.len() == 32 => {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&b);
                        ckb_types::H256(arr)
                    }
                    _ => continue,
                };

                let tx_status = match rpc.get_transaction(tx_hash_bytes) {
                    Ok(Some(s)) => s,
                    _ => continue,
                };

                let is_pending = tx_status.status != "Committed" && tx_status.status != "committed";

                let tx_view = match tx_status.transaction {
                    Some(tv) => tv,
                    None => continue,
                };

                // Determine timestamp from block header.
                let timestamp = if let Some(ref bh) = tx_status.block_hash {
                    if let Some(&cached) = header_cache.get(bh) {
                        cached
                    } else {
                        let ts = rpc
                            .get_header(bh.clone())
                            .ok()
                            .flatten()
                            .map(|h| {
                                // CKB header timestamp is in milliseconds.
                                h.inner.timestamp.value() / 1000
                            })
                            .unwrap_or(0);
                        header_cache.insert(bh.clone(), ts);
                        ts
                    }
                } else {
                    0
                };

                // Check for DAO type script in outputs.
                let has_dao_output = tx_view.inner.outputs.iter().any(|out| {
                    out.type_
                        .as_ref()
                        .is_some_and(|t| format!("{:#x}", t.code_hash) == dao_type_hash)
                });

                // Check DAO data to distinguish deposit vs prepare.
                let dao_output_data_is_zero = tx_view
                    .inner
                    .outputs
                    .iter()
                    .zip(tx_view.inner.outputs_data.iter())
                    .any(|(out, data)| {
                        out.type_
                            .as_ref()
                            .is_some_and(|t| format!("{:#x}", t.code_hash) == dao_type_hash)
                            && data.len() == 8
                            && data.as_bytes().iter().all(|&b| b == 0)
                    });

                // Classify the transaction.
                // DAO withdraw: has a DAO cell dep but no DAO type script in outputs
                // (the DAO cell is consumed, capacity returned to regular lock).
                let has_dao_cell_dep = tx_view.inner.cell_deps.iter().any(|dep| {
                    // The DAO cell dep is always at genesis tx index 2.
                    dep.out_point.index.value() == 2
                });

                let tx_kind = if has_dao_output && dao_output_data_is_zero {
                    TxKind::DaoDeposit
                } else if has_dao_output {
                    TxKind::DaoPrepare
                } else if has_dao_cell_dep && has_input && !has_dao_output {
                    TxKind::DaoWithdraw
                } else if has_output && !has_input {
                    TxKind::Incoming
                } else {
                    TxKind::Outgoing
                };

                // Classify each output: owner's capacity (incl. change), other
                // wallet accounts' capacity, and external capacity.
                let mut owner_capacity: u64 = 0;
                let mut internal_counterparty: Option<String> = None;
                let mut internal_capacity: u64 = 0;
                let mut external_capacity: u64 = 0;
                let mut external_recipient: Option<String> = None;

                for out in &tx_view.inner.outputs {
                    let cap = out.capacity.value();
                    match wallet_lock_args(out, wallet_code_hash, &all_lock_args_set) {
                        Some(args) if args == owner_lock_args => {
                            owner_capacity += cap;
                        }
                        Some(args) => {
                            // Output goes to a different wallet account.
                            internal_counterparty = Some(args);
                            internal_capacity += cap;
                        }
                        None => {
                            external_capacity += cap;
                            // Skip DAO type outputs — those belong to DAO flow,
                            // not a user-facing recipient address.
                            let is_dao_output = out
                                .type_
                                .as_ref()
                                .is_some_and(|t| format!("{:#x}", t.code_hash) == dao_type_hash);
                            if external_recipient.is_none() && !is_dao_output {
                                let packed: ckb_types::packed::Script = out.lock.clone().into();
                                let is_mainnet =
                                    matches!(network, node_manager::NetworkType::Mainnet);
                                external_recipient = Some(
                                    qpv2_core::utilities::script_to_address(&packed, is_mainnet),
                                );
                            }
                        }
                    }
                }

                let amount = match tx_kind {
                    TxKind::Incoming => owner_capacity,
                    TxKind::Outgoing => {
                        // Prefer external send amount; for internal transfers
                        // use the amount sent to the other wallet account.
                        if external_capacity > 0 {
                            external_capacity
                        } else {
                            internal_capacity
                        }
                    }
                    TxKind::DaoDeposit | TxKind::DaoPrepare => {
                        // Use the DAO cell's capacity for deposit/prepare.
                        tx_view
                            .inner
                            .outputs
                            .iter()
                            .find(|out| {
                                out.type_
                                    .as_ref()
                                    .is_some_and(|t| format!("{:#x}", t.code_hash) == dao_type_hash)
                            })
                            .map(|out| out.capacity.value())
                            .unwrap_or(owner_capacity)
                    }
                    TxKind::DaoWithdraw => owner_capacity,
                };

                // For outgoing: counterparty is the recipient (from outputs).
                // For incoming: counterparty is the sender (from input accounts).
                let internal_counterparty_lock_args = match tx_kind {
                    TxKind::Incoming => sender_account,
                    _ => internal_counterparty,
                };

                let external_recipient_address = match tx_kind {
                    TxKind::Outgoing => external_recipient,
                    _ => None,
                };

                let record = TxRecord {
                    tx_hash: tx_hash_str,
                    tx_kind,
                    amount,
                    block_number,
                    timestamp,
                    is_pending,
                    owner_lock_args,
                    internal_counterparty_lock_args,
                    external_recipient_address,
                };

                if sender.send(Ok(TxHistoryEvent::Record(record))).is_err() {
                    return;
                }
            }

            let _ = sender.send(Ok(TxHistoryEvent::Done));
        });
    }

    /// Fetch balances for all accounts in a background thread.
    pub(crate) fn fetch_all_balances(&mut self) {
        if self.rpc_client.is_none() || self.balance_receiver.is_some() {
            return;
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
