pub mod config;
pub mod error;
pub mod process;
pub mod queries;
pub mod rpc;
pub mod tx_builder;

use std::sync::{Arc, Mutex};

use ckb_types::H256;

pub use ckb_sdk::rpc::ckb_indexer::{CellType, Tx, TxWithCell, TxWithCells};
pub use config::{NetworkType, NodeConfig, NodeType};
pub use error::NodeManagerError;
pub use process::{LightClientProcess, NodeProcess};
pub use queries::{DepositedCell, PreparedCell};
pub use rpc::{CkbRpc, LightClientRpc, TransactionStatus};
pub use tx_builder::{
    fill_witness, QpDaoDepositBuilder, QpDaoPrepareBuilder, QpDaoWithdrawBuilder, QpTransferBuilder,
};

/// High-level handle that owns the RPC client, the (optional) running
/// local node process, and the config they were built with. Exposes every
/// node-related operation as a method.
///
/// The GUI keeps a single instance on `App` and clones it cheaply into
/// background threads. All cloned handles share the same process slot
/// behind a mutex; in practice only the UI thread ever touches
/// `spawn` / `stop` / `is_process_running`, so the lock sees no
/// contention.
#[derive(Clone)]
pub struct NodeManager {
    config: NodeConfig,
    /// Shared RPC client. Public so callers can invoke low-level `CkbRpc`
    /// trait methods (e.g. `get_transaction`, `get_header`) directly
    /// without every method needing a thin wrapper on `NodeManager`.
    pub rpc: Arc<dyn CkbRpc>,
    /// Single-writer slot for the local node child process. `None` when
    /// the active backend is `PublicRpc` or when a local backend hasn't
    /// been started yet. Held behind `Arc<Mutex<_>>` so the outer
    /// `NodeManager` stays cheap to clone; only `spawn` / `stop` /
    /// `is_process_running` actually acquire the lock, and only from the
    /// UI thread.
    process: Arc<Mutex<Option<Box<dyn NodeProcess>>>>,
}

impl NodeManager {
    /// Builds the RPC client and initializes an empty process slot. Call
    /// `spawn()` afterward to launch the local node when the config
    /// calls for one.
    pub fn new(config: NodeConfig) -> Self {
        // Pick the concrete RPC implementation for the backend and erase
        // it to `Arc<dyn CkbRpc>` for shared use. Callers that need
        // backend-specific methods downcast via `CkbRpc::as_any`.
        let rpc: Arc<dyn CkbRpc> = match config.node_type {
            NodeType::PublicRpc | NodeType::FullNode => {
                Arc::new(rpc::FullNodeRpc::new(&config.rpc_url))
            }
            NodeType::LightClient => Arc::new(rpc::LightClientRpc::new(&config.rpc_url)),
        };
        Self {
            config,
            rpc,
            process: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the configuration this manager was built with.
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Returns the active network.
    pub fn network(&self) -> NetworkType {
        self.config.network
    }

    /// Returns `true` if the active network is mainnet.
    pub fn is_mainnet(&self) -> bool {
        self.config.network == NetworkType::Mainnet
    }

    // ── Query helpers ─────────────────────────────────────────────────

    /// Returns the total spendable capacity (in shannons) for the given address.
    pub fn spendable_capacity(
        &self,
        from_address: &ckb_sdk::Address,
    ) -> Result<u64, NodeManagerError> {
        queries::spendable_capacity(self.rpc.as_ref(), from_address)
    }

    /// Queries all DAO cells for an address and partitions them into
    /// deposited and prepared cells.
    pub fn categorize_dao_cells(
        &self,
        address: &ckb_sdk::Address,
    ) -> Result<(Vec<DepositedCell>, Vec<PreparedCell>), NodeManagerError> {
        queries::categorize_dao_cells(self.rpc.as_ref(), address)
    }

    /// Total balance (in shannons) for the QuantumPurse lock with the given
    /// `lock_args`, using the manager's active network.
    pub fn fetch_quantum_lock_balance(
        &self,
        lock_args_hex: &str,
    ) -> Result<u64, NodeManagerError> {
        queries::fetch_quantum_lock_balance(self.rpc.as_ref(), lock_args_hex, self.config.network)
    }

    /// Recent transactions touching the QuantumPurse lock with the given
    /// `lock_args`, using the manager's active network.
    pub fn fetch_recent_transactions(
        &self,
        lock_args_hex: &str,
        after_block: Option<u64>,
        limit: Option<usize>,
    ) -> Result<Vec<Tx>, NodeManagerError> {
        queries::fetch_recent_transactions(
            self.rpc.as_ref(),
            lock_args_hex,
            self.config.network,
            after_block,
            limit,
        )
    }

    // ── Signing / broadcast helpers ───────────────────────────────────

    /// Fetches the input cells (CellOutput + data) for every input in `tx`.
    /// Required by `generate_ckb_tx_message_all` when signing.
    pub fn fetch_input_cells(
        &self,
        tx: &ckb_types::core::TransactionView,
    ) -> Result<Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>, NodeManagerError>
    {
        tx_builder::fetch_input_cells(self.rpc.as_ref(), tx)
    }

    /// Submits a signed transaction to the network.
    pub fn send_transaction(
        &self,
        tx: &ckb_types::core::TransactionView,
    ) -> Result<H256, NodeManagerError> {
        tx_builder::send_transaction(self.rpc.as_ref(), tx)
    }

    // ── Transaction builders ──────────────────────────────────────────

    /// Creates a transfer builder bound to this manager's RPC and network.
    pub fn transfer_builder(&self) -> QpTransferBuilder<'_> {
        QpTransferBuilder::new(self.rpc.as_ref(), self.is_mainnet())
    }

    /// Creates a DAO deposit builder bound to this manager's RPC and network.
    pub fn dao_deposit_builder(&self) -> QpDaoDepositBuilder<'_> {
        QpDaoDepositBuilder::new(self.rpc.as_ref(), self.is_mainnet())
    }

    /// Creates a DAO prepare builder bound to this manager's RPC and network.
    pub fn dao_prepare_builder(&self) -> QpDaoPrepareBuilder<'_> {
        QpDaoPrepareBuilder::new(self.rpc.as_ref(), self.is_mainnet())
    }

    /// Creates a DAO withdraw builder bound to this manager's RPC and network.
    pub fn dao_withdraw_builder(&self) -> QpDaoWithdrawBuilder<'_> {
        QpDaoWithdrawBuilder::new(self.rpc.as_ref(), self.is_mainnet())
    }

    /// Number of peers for local-node backends (`LightClient`, `FullNode`).
    /// Returns `Ok(None)` for `PublicRpc` — the remote endpoint's peer
    /// count isn't a meaningful wallet-side metric. `Err` when the local
    /// node is unreachable or returns a malformed response.
    ///
    /// Reuses the shared `self.rpc` instance via `Any` downcast so we
    /// don't construct a second RPC client per call.
    pub fn peer_count(&self) -> Result<Option<usize>, NodeManagerError> {
        if let Some(light) = self.rpc.as_any().downcast_ref::<rpc::LightClientRpc>() {
            return light.get_peer_count().map(Some);
        }
        if let Some(full) = self.rpc.as_any().downcast_ref::<rpc::FullNodeRpc>() {
            // FullNode and PublicRpc share the same concrete type
            // (`FullNodeRpc`); only the local FullNode case has a
            // meaningful "peers connected to my node" answer.
            return match self.config.node_type {
                NodeType::FullNode => full.get_peer_count().map(Some),
                _ => Ok(None),
            };
        }
        Ok(None)
    }

    /// Minimum synced block across every script registered with the
    /// light client. `Ok(None)` outside `LightClient` (PublicRpc / FullNode
    /// index everything; the concept doesn't apply) and when no scripts
    /// are registered yet.
    ///
    /// Per the upstream LC behavior, every script's stored block_number
    /// advances to the current sync front once sync passes its anchor;
    /// scripts whose start_block is still ahead of sync show their own
    /// anchor. Taking the **min** is therefore the most honest "how far
    /// has the light client got" — it equals the sync front in steady
    /// state and reveals "we still haven't reached every script's start"
    /// in the transient.
    pub fn synced_block(&self) -> Result<Option<u64>, NodeManagerError> {
        let Some(light) = self.rpc.as_any().downcast_ref::<rpc::LightClientRpc>() else {
            return Ok(None);
        };
        let scripts = light.get_scripts()?;
        Ok(scripts.iter().map(|s| s.block_number.value()).min())
    }

    /// Asks the light client to pull the QR-lock-script deployment
    /// transaction into its local store. Required because the LC only
    /// indexes cells whose lock matches a registered filter script — the
    /// dep cell's lock isn't ours, so without an explicit fetch the LC
    /// will reject any transfer that uses it as a `cell_dep`.
    ///
    /// Returns `true` when the dep is in the LC's store; `false` when a
    /// fetch was enqueued / is in progress / peers couldn't find it.
    /// Errors when called against a non-LightClient backend — that's a
    /// caller bug, not a runtime condition (full nodes / public RPC
    /// index every cell and don't need this call).
    pub fn fetch_qr_lock_dep(&self) -> Result<bool, NodeManagerError> {
        let Some(light) = self.rpc.as_any().downcast_ref::<rpc::LightClientRpc>() else {
            return Err(NodeManagerError::UnsupportedOperation {
                node_type: self.config.node_type.to_string(),
                reason: "fetch_qr_lock_dep is light-client-only.".to_string(),
            });
        };
        let dep_tx_hash_hex = match self.config.network {
            NetworkType::Mainnet => qpv2_core::constants::CKB_MAINNET_CELL_DEP_TX_HASH,
            NetworkType::Testnet => qpv2_core::constants::CKB_TESTNET_CELL_DEP_TX_HASH,
        };
        let tx_hash: H256 = dep_tx_hash_hex
            .trim_start_matches("0x")
            .parse()
            .map_err(|e| {
                NodeManagerError::RpcError(format!("Invalid QR lock dep tx hash: {}", e))
            })?;
        light.fetch_transaction(tx_hash)
    }

    /// Registers one or more wallet lock scripts with the light client's
    /// indexer in a single `set_scripts` call. Empty input is a no-op.
    /// Errors when called against a non-LightClient backend — that's a
    /// caller bug, not a runtime condition (full nodes / public RPC
    /// index every cell and don't need this call). Callers that need
    /// per-account start blocks (e.g. importing an existing wallet with
    /// funded history) can call `LightClientRpc::register_lock_scripts`
    /// directly on a downcasted reference.
    ///
    /// Start-block policy
    /// ------------------
    /// - **First time on this LC** (`get_scripts` returns empty): anchor
    ///   every entry at `0`. Triggers a full rescan from genesis —
    ///   slow but correct. Handles the "user used PublicRpc, funded an
    ///   account, then switched to LC" case: pre-switch deposits live
    ///   below current tip and would be missed if we anchored at tip.
    /// - **LC already has entries** (returning to a network the LC has
    ///   seen before, or adding a new account to a running LC): anchor
    ///   new entries at `tip`. Already-tracked accounts are filtered
    ///   out by `LightClientRpc::register_lock_scripts` so their
    ///   existing sync cursors stay put.
    ///
    /// Note: `get_tip_header` against a freshly-spawned LC returns the
    /// genesis header (block 0) until peer headers arrive, so anchoring
    /// at "tip" right after spawn would also yield 0 by accident. The
    /// explicit branch below makes the behavior deterministic instead
    /// of relying on that timing.
    pub fn register_lock_scripts(
        &self,
        lock_args_list: &[String],
    ) -> Result<(), NodeManagerError> {
        let Some(light) = self.rpc.as_any().downcast_ref::<rpc::LightClientRpc>() else {
            return Err(NodeManagerError::UnsupportedOperation {
                node_type: self.config.node_type.to_string(),
                reason: "register_lock_scripts is light-client-only.".to_string(),
            });
        };
        if lock_args_list.is_empty() {
            return Ok(());
        }

        let start_block = if light.get_scripts()?.is_empty() {
            0
        } else {
            light.get_tip_header()?.inner.number.value()
        };

        let scripts: Vec<(&str, u64)> = lock_args_list
            .iter()
            .map(|a| (a.as_str(), start_block))
            .collect();
        light.register_lock_scripts(&scripts, self.config.network)
    }

    /// Forces every given lock script to `start_block` on the light
    /// client, **without** the cursor-preservation filter. Use for the
    /// manual "set scan from block" UI; do not use for the auto-flow
    /// (account creation, network switch) — those should stay on
    /// [`Self::register_lock_scripts`] so existing cursors aren't
    /// clobbered. Errors when called against a non-LightClient backend.
    pub fn register_all_lock_scripts(
        &self,
        lock_args_list: &[String],
        start_block: u64,
    ) -> Result<(), NodeManagerError> {
        let Some(light) = self.rpc.as_any().downcast_ref::<rpc::LightClientRpc>() else {
            return Err(NodeManagerError::UnsupportedOperation {
                node_type: self.config.node_type.to_string(),
                reason: "register_all_lock_scripts is light-client-only.".to_string(),
            });
        };
        if lock_args_list.is_empty() {
            return Ok(());
        }
        let scripts: Vec<(&str, u64)> = lock_args_list
            .iter()
            .map(|a| (a.as_str(), start_block))
            .collect();
        light.register_all_lock_scripts(&scripts, self.config.network)
    }

    // ── Local process lifecycle ───────────────────────────────────────

    /// Spawns the local node process for the active backend and stores
    /// the handle internally. Idempotent: if a process is already running
    /// returns `Ok(())` without doing anything. `PublicRpc` is a no-op
    /// (nothing to spawn). `FullNode` is not yet implemented and returns
    /// `UnsupportedOperation`.
    pub fn spawn(&self) -> Result<(), NodeManagerError> {
        let mut guard = self
            .process
            .lock()
            .expect("node_manager process mutex poisoned");
        if guard.is_some() {
            return Ok(());
        }
        let process: Box<dyn NodeProcess> = match self.config.node_type {
            NodeType::LightClient => Box::new(LightClientProcess::start(&self.config)?),
            NodeType::FullNode => {
                // TODO: full-node spawn path — build `ckb`'s config.toml
                // from `config/default/*` (the upstream `ckb init` output),
                // run `ckb run --config <path>`, wait for 8114 to accept
                // TCP like the light client does. Binary name: `ckb`.
                return Err(NodeManagerError::UnsupportedOperation {
                    node_type: self.config.node_type.to_string(),
                    reason: "FullNode spawn path not implemented yet.".to_string(),
                });
            }
            NodeType::PublicRpc => return Ok(()),
        };
        *guard = Some(process);
        Ok(())
    }

    /// Stops and drops any running local node process. No-op when the
    /// slot is already empty.
    pub fn stop(&self) {
        let mut guard = self
            .process
            .lock()
            .expect("node_manager process mutex poisoned");
        if let Some(mut proc) = guard.take() {
            let _ = proc.stop();
        }
    }

    /// `true` when the manager owns a local node child handle — i.e.
    /// `spawn()` was called successfully and `stop()` hasn't been called.
    ///
    /// This is **not** a strict liveness check: if the child crashed or
    /// was killed externally, the handle slot is still `Some(_)` and
    /// this method still returns `true`. For true online-ness, probe the
    /// RPC (a successful `get_tip_header` is the authoritative signal).
    /// Callers that only need "did we start a local backend for this
    /// wallet session" (e.g., gating a DB-size readout) can rely on
    /// this value.
    pub fn has_local_process(&self) -> bool {
        self.process
            .lock()
            .expect("node_manager process mutex poisoned")
            .is_some()
    }
}
