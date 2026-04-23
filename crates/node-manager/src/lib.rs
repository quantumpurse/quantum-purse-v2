pub mod config;
pub mod error;
pub mod process;
pub mod queries;
pub mod rpc;
pub mod tx_builder;

use std::sync::Arc;

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

/// High-level handle that owns the RPC client and exposes every node-related
/// operation as a method.
///
/// The GUI keeps a single instance on `App` and clones it cheaply into
/// background threads (the inner `Arc` bumps a refcount; no RPC client
/// is rebuilt per task).
#[derive(Clone)]
pub struct NodeManager {
    config: NodeConfig,
    /// Shared RPC client. Public so callers can invoke low-level `CkbRpc`
    /// trait methods (e.g. `get_transaction`, `get_header`) directly without
    /// every method needing a thin wrapper on `NodeManager`.
    pub rpc: Arc<dyn CkbRpc>,
}

impl NodeManager {
    /// Builds the RPC client once and wraps it with the given config.
    pub fn new(config: NodeConfig) -> Self {
        let rpc = rpc::connect(&config);
        Self { config, rpc }
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
}
