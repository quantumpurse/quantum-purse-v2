//! Background data fetchers (balances, DAO cells, spendable capacity, tx history).

use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::sync::mpsc;
use std::time::Duration;

use qpv2_core::constants::{
    CKB_MAINNET_CODE_HASH, CKB_MAINNET_HASH_TYPE, CKB_TESTNET_CODE_HASH, CKB_TESTNET_HASH_TYPE,
};

use crate::types::{
    DaoQueryEvent, NodeStatus, NodeStatusUpdate, SpendableCapacityTarget, TransactionStatus,
    TxHistoryEvent, TxKind, TxRecord,
};
use crate::App;

/// Initial backoff between retry attempts on a transient RPC failure.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
/// Cap on the exponential backoff.
const RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

/// Retries `f` forever with exponential backoff (capped at
/// `RETRY_MAX_DELAY`) until it returns `Ok(Some(v))`. Both `Err(_)` and
/// `Ok(None)` are treated as "try again" and logged distinctly.
///
/// Callers that can never produce a legitimate `None` (e.g. the indexer
/// returning `Vec<Tx>`) adapt with `.map(Some)`. Callers that do
/// (`get_transaction`, `get_header`) pass their `Result<Option<T>, _>`
/// through unchanged.
///
/// Used by the tx-history sync thread so a transient public-RPC failure
/// never drops a tx silently. See `BACKLOG.md` ("Reorg handling") for the
/// cancellation story once reorg-aware sync lands.
fn retry_until_ready<T, E: Display>(
    tag: &str,
    mut f: impl FnMut() -> Result<Option<T>, E>,
) -> T {
    let mut delay = RETRY_BASE_DELAY;
    loop {
        match f() {
            Ok(Some(v)) => return v,
            Ok(None) => {
                eprintln!("tx history: {} returned None, retrying in {:?}", tag, delay);
            }
            Err(e) => {
                eprintln!("tx history: {} failed ({}), retrying in {:?}", tag, e, delay);
            }
        }
        std::thread::sleep(delay);
        delay = (delay * 2).min(RETRY_MAX_DELAY);
    }
}

/// Converts a hex-encoded wallet lock_args to a bech32m CKB address (post-2021 format).
///
/// Assumes the wallet's quantum-resistant lock script code_hash / hash_type.
/// Use `script_to_address` for arbitrary external locks instead.
pub(crate) fn lock_args_to_address(lock_args: &str, is_mainnet: bool) -> Result<String, String> {
    use ckb_sdk::{Address, AddressPayload, NetworkType};
    use ckb_types::{bytes::Bytes, core::ScriptHashType};

    let (code_hash_hex, hash_type_str, network) = if is_mainnet {
        (
            CKB_MAINNET_CODE_HASH,
            CKB_MAINNET_HASH_TYPE,
            NetworkType::Mainnet,
        )
    } else {
        (
            CKB_TESTNET_CODE_HASH,
            CKB_TESTNET_HASH_TYPE,
            NetworkType::Testnet,
        )
    };

    let code_hash_bytes = hex::decode(code_hash_hex.trim_start_matches("0x"))
        .map_err(|e| format!("Failed to decode code_hash: {:?}", e))?;
    let mut code_hash_array = [0u8; 32];
    code_hash_array.copy_from_slice(&code_hash_bytes);

    let script_hash_type = match hash_type_str {
        "type" => ScriptHashType::Type,
        "data1" => ScriptHashType::Data1,
        _ => return Err(format!("Unsupported hash_type: {}", hash_type_str)),
    };

    let args_bytes =
        hex::decode(lock_args).map_err(|e| format!("Failed to decode lock_args: {:?}", e))?;

    let payload = AddressPayload::new_full(
        script_hash_type,
        code_hash_array.into(),
        Bytes::from(args_bytes),
    );
    Ok(Address::new(network, payload, true).to_string())
}

/// Converts an arbitrary on-chain lock `Script` to its bech32m address string.
///
/// Unlike `lock_args_to_address`, this accepts any lock script (any code_hash /
/// hash_type), so it works for external recipients in the Address Book.
pub(crate) fn script_to_address(script: &ckb_types::packed::Script, is_mainnet: bool) -> String {
    use ckb_sdk::{Address, AddressPayload, NetworkType};

    let network = if is_mainnet {
        NetworkType::Mainnet
    } else {
        NetworkType::Testnet
    };
    let payload = AddressPayload::from(script.clone());
    Address::new(network, payload, true).to_string()
}

impl App {
    /// Kick off background queries for deposited + prepared DAO cells across all accounts.
    pub(crate) fn fetch_dao_cells(&mut self) {
        if self.accounts.is_empty() || self.dao_cells_query_rx.is_some() {
            return;
        }

        self.dao_deposited_staging.clear();
        self.dao_prepared_staging.clear();

        let is_mainnet = self.is_mainnet();
        let nm = self.node_manager.clone();
        let all_lock_args: Vec<String> = self.accounts.clone();

        let (tx, rx) = mpsc::channel();
        self.dao_cells_query_rx = Some(rx);

        std::thread::spawn(move || {
            for lock_args in &all_lock_args {
                let address_str = match lock_args_to_address(lock_args, is_mainnet) {
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

                let (deposited, prepared) = match nm.categorize_dao_cells(&address) {
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
        let from_addr_str = match lock_args_to_address(&lock_args, is_mainnet) {
            Ok(a) => a,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let nm = self.node_manager.clone();
        let (tx, rx) = mpsc::channel();
        self.spendable_capacity_rx = Some((target, rx));

        std::thread::spawn(move || {
            let result = (|| -> Result<u64, String> {
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                nm.spendable_capacity(&from_address)
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

        // Incremental mode uses the current watermark (derived from
        // `tx_history`). A cold fetch clears memory so everything is
        // re-materialized from block 0.
        let after_block = if incremental {
            Some(self.tx_history_watermark())
        } else {
            self.tx_history.clear();
            None
        };

        let nm = self.node_manager.clone();
        let network = nm.network();
        let all_lock_args: Vec<String> = self.accounts.clone();

        let (sender, rx) = mpsc::channel();
        self.tx_history_rx = Some(rx);

        std::thread::spawn(move || {
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
                // Retries transient indexer failures — we must never skip an
                // account silently because a dropped page becomes a permanently
                // missing tx once the watermark advances.
                let txs = retry_until_ready(
                    &format!("fetch_recent_transactions | lock_args=0x{} | after_block={}", lock_args, after_block.map_or("none".to_string(), |b| b.to_string())),
                    || nm.fetch_recent_transactions(lock_args, after_block, None).map(Some),
                );

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

                // Retry until we get a concrete tx_status. The indexer
                // just handed us this hash; `Ok(None)` means the node hasn't
                // caught up to its own indexer or is briefly unhappy — retry.
                let tx_hash_key = tx_hash_bytes.clone();
                let tx_status = retry_until_ready(
                    &format!("get_transaction tx_hash={:#x}", tx_hash_key),
                    || nm.rpc.get_transaction(tx_hash_bytes.clone()),
                );

                let is_pending = tx_status.status != "Committed" && tx_status.status != "committed";

                // A committed tx must have a transaction view. If it's missing
                // we want to retry rather than drop — so re-poll until it lands.
                // (For pending txs this is valid too: they should at least be
                // returned by the node.)
                let tx_view = match tx_status.transaction {
                    Some(tv) => tv,
                    None => retry_until_ready(
                        &format!("get_transaction.tx_view tx_hash={:#x}", tx_hash_key),
                        || {
                            nm.rpc
                                .get_transaction(tx_hash_bytes.clone())
                                .map(|opt| opt.and_then(|s| s.transaction))
                        },
                    ),
                };

                // Determine timestamp from block header. Retry until the
                // header resolves — we need a stable timestamp to avoid
                // rewriting the stored record on the next tick.
                let timestamp = if let Some(ref bh) = tx_status.block_hash {
                    if let Some(&cached) = header_cache.get(bh) {
                        cached
                    } else {
                        let bh_clone = bh.clone();
                        let header = retry_until_ready(
                            &format!("get_header block_hash={:#x}", bh_clone),
                            || nm.rpc.get_header(bh_clone.clone()),
                        );
                        // CKB header timestamp is in milliseconds.
                        let ts = header.inner.timestamp.value() / 1000;
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
                                external_recipient = Some(script_to_address(&packed, is_mainnet));
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
        if self.balance_receiver.is_some() {
            return;
        }

        let accounts = self.accounts.clone();
        if accounts.is_empty() {
            return;
        }

        let nm = self.node_manager.clone();
        let (tx, rx) = mpsc::channel();
        self.balance_receiver = Some(rx);

        std::thread::spawn(move || {
            for lock_args in accounts {
                let result = nm
                    .fetch_quantum_lock_balance(&lock_args)
                    .map_err(|e| e.to_string());
                // If the receiver is dropped (e.g. wallet locked), stop.
                if tx.send((lock_args, result)).is_err() {
                    break;
                }
            }
        });
    }

    /// Refresh the Node Manager card's cached status in a background
    /// thread. One in-flight poll at a time (`node_status_rx` guards).
    pub(crate) fn fetch_node_status(&mut self) {
        if self.node_status_rx.is_some() {
            return;
        }

        let nm = self.node_manager.clone();
        let cfg = nm.config();
        let rpc_port = parse_rpc_port(&cfg.rpc_url);
        let data_dir = cfg.node_data_dir();
        let is_local = cfg.requires_binary();
        let has_process = self.node_manager.has_local_process();

        let (tx, rx) = mpsc::channel();
        self.node_status_rx = Some(rx);

        std::thread::spawn(move || {
            // Tip header — the primary online-ness signal.
            let tip_block = match nm.rpc.get_tip_header() {
                Ok(h) => Some(h.inner.number.value()),
                Err(_) => None,
            };
            let online = tip_block.is_some();

            // Peer count — None for PublicRpc by design.
            let peer_count = nm.peer_count().ok().flatten();

            // Synced block — None for PublicRpc/FullNode and when no
            // scripts are registered yet (fresh light client).
            let synced_block = nm.synced_block().ok().flatten();

            // DB size — only meaningful for local backends. Treat walk
            // failures as "not available" rather than propagating; the
            // directory may not exist yet on a fresh spawn.
            let db_size_bytes = if is_local && has_process {
                directory_size(&data_dir).ok()
            } else {
                None
            };

            let status = NodeStatus {
                tip_block,
                peer_count,
                db_size_bytes,
                rpc_port,
                synced_block,
                online,
            };
            let _ = tx.send(Ok(status) as NodeStatusUpdate);
        });
    }
}

/// Parses the port out of an RPC URL (`http://host:port` or
/// `https://host:port`). Returns `None` on malformed input.
fn parse_rpc_port(url: &str) -> Option<u16> {
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host_port = stripped.split('/').next().unwrap_or(stripped);
    let (_, port) = host_port.rsplit_once(':')?;
    port.parse().ok()
}

/// Recursively sums the size of every regular file under `path`.
/// Symlinks are followed via `metadata()` just like `du` would; cycles
/// aren't a concern for the node's own data directory.
fn directory_size(path: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) if !dir.exists() => return Ok(0),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}
