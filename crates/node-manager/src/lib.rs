pub mod config;
pub mod error;
pub mod light_client_spawn;
pub mod process;
pub mod queries;
pub mod rpc;
pub mod tx_builder;

use std::sync::{Arc, Mutex};

use ckb_types::H256;

pub use ckb_sdk::rpc::ckb_indexer::{CellType, Tx, TxWithCell, TxWithCells};
pub use config::{NetworkType, NodeConfig, NodeType};
pub use error::NodeManagerError;
pub use process::NodeProcess;
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
    process: Arc<Mutex<Option<NodeProcess>>>,
}

impl NodeManager {
    /// Builds the RPC client and initializes an empty process slot. Call
    /// `spawn()` afterward to launch the local node when the config
    /// calls for one.
    pub fn new(config: NodeConfig) -> Self {
        let rpc = rpc::connect(&config);
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

    /// Returns a fresh light-client RPC handle for operations outside the
    /// unified `CkbRpc` trait (e.g. `set_scripts`). Errors if the active
    /// backend is not a light client.
    pub fn connect_light_client(&self) -> Result<LightClientRpc, NodeManagerError> {
        rpc::connect_light_client(&self.config)
    }

    /// Number of peers for local-node backends (`LightClient`, `FullNode`).
    /// Returns `Ok(None)` for `PublicRpc` — the remote endpoint's peer
    /// count isn't a meaningful wallet-side metric. `Err` when the local
    /// node is unreachable or returns a malformed response.
    pub fn peer_count(&self) -> Result<Option<usize>, NodeManagerError> {
        match self.config.node_type {
            NodeType::LightClient => rpc::connect_light_client(&self.config)?
                .get_peer_count()
                .map(Some),
            NodeType::FullNode => rpc::FullNodeRpc::new(&self.config.rpc_url)
                .get_peer_count()
                .map(Some),
            NodeType::PublicRpc => Ok(None),
        }
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
        let process = match self.config.node_type {
            NodeType::LightClient => light_client_spawn::spawn(&self.config)?,
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
