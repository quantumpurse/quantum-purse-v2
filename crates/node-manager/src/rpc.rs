use crate::config::{NodeConfig, NodeType};
use crate::error::NodeManagerError;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::rpc::ckb_indexer::{Cell, CellsCapacity, Order, Pagination, SearchKey, Tx};
use ckb_sdk::rpc::ckb_light_client::{LightClientRpcClient, ScriptStatus, SetScriptsCommand};
use ckb_sdk::rpc::{CkbRpcClient, ResponseFormatGetter};
use ckb_types::H256;
use std::sync::Arc;

/// Unified RPC interface for wallet operations.
///
/// Abstracts over full node (`CkbRpcClient`) and light client (`LightClientRpcClient`)
/// so wallet code does not need to know which backend is active.
///
/// The `Send + Sync` supertraits allow a single client instance to be shared
/// across background threads via `Arc<dyn CkbRpc>` inside `NodeManager`.
pub trait CkbRpc: Send + Sync {
    /// Returns the tip (latest) block header.
    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError>;

    /// Queries live cells matching the given search key.
    fn get_cells(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>, NodeManagerError>;

    /// Returns the total capacity of live cells matching the search key.
    fn get_cells_capacity(
        &self,
        search_key: SearchKey,
    ) -> Result<Option<CellsCapacity>, NodeManagerError>;

    /// Submits a transaction to the network.
    fn send_transaction(
        &self,
        tx: ckb_jsonrpc_types::Transaction,
    ) -> Result<H256, NodeManagerError>;

    /// Retrieves a transaction by hash, returning its status.
    fn get_transaction(&self, hash: H256) -> Result<Option<TransactionStatus>, NodeManagerError>;

    /// Gets a header by its hash.
    fn get_header(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>, NodeManagerError>;

    /// Gets detailed transaction with status (needed for DAO calculations).
    fn get_transaction_with_status(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::TransactionWithStatusResponse>, NodeManagerError>;

    /// Queries transactions matching the given search key via the indexer.
    fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, NodeManagerError>;

    /// Gets the RPC URL (temporary method for SDK components).
    /// TODO: Remove when we implement custom collectors using the trait.
    fn get_rpc_url(&self) -> String;
}

/// Simplified transaction status returned by `get_transaction`.
///
/// Normalizes the different response types from full node and light client
/// into a common representation.
#[derive(Debug, Clone)]
pub struct TransactionStatus {
    /// The transaction view, if available.
    pub transaction: Option<ckb_jsonrpc_types::TransactionView>,
    /// Transaction status string: "pending", "proposed", "committed", "rejected", or "unknown".
    pub status: String,
    /// Block hash of the block that committed this transaction, if committed.
    pub block_hash: Option<H256>,
}

/// Full node / public RPC implementation.
pub struct FullNodeRpc {
    client: CkbRpcClient,
    rpc_url: String,
}

impl FullNodeRpc {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            client: CkbRpcClient::new(rpc_url),
            rpc_url: rpc_url.to_string(),
        }
    }
}

impl CkbRpc for FullNodeRpc {
    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError> {
        self.client
            .get_tip_header()
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_cells(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>, NodeManagerError> {
        self.client
            .get_cells(search_key, order, limit.into(), after)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_cells_capacity(
        &self,
        search_key: SearchKey,
    ) -> Result<Option<CellsCapacity>, NodeManagerError> {
        self.client
            .get_cells_capacity(search_key)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn send_transaction(
        &self,
        tx: ckb_jsonrpc_types::Transaction,
    ) -> Result<H256, NodeManagerError> {
        self.client
            .send_transaction(tx, None)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_transaction(&self, hash: H256) -> Result<Option<TransactionStatus>, NodeManagerError> {
        let resp = self
            .client
            .get_transaction(hash)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

        Ok(resp.map(|r| TransactionStatus {
            transaction: r.transaction.and_then(|inner| inner.get_value().ok()),
            status: format!("{:?}", r.tx_status.status),
            block_hash: r.tx_status.block_hash,
        }))
    }

    fn get_header(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>, NodeManagerError> {
        self.client
            .get_header(hash)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_transaction_with_status(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::TransactionWithStatusResponse>, NodeManagerError> {
        self.client
            .get_transaction(hash)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, NodeManagerError> {
        self.client
            .get_transactions(search_key, order, limit.into(), after)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_rpc_url(&self) -> String {
        self.rpc_url.clone()
    }
}

/// Light client RPC implementation.
///
/// Provides the same `CkbRpc` interface plus light-client-specific methods
/// for script registration.
pub struct LightClientRpc {
    client: LightClientRpcClient,
    rpc_url: String,
}

impl LightClientRpc {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            client: LightClientRpcClient::new(rpc_url),
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Registers lock/type scripts with the light client so it indexes matching cells.
    ///
    /// Must be called after creating a new account so the light client tracks its cells.
    pub fn set_scripts(
        &self,
        scripts: Vec<ScriptStatus>,
        command: Option<SetScriptsCommand>,
    ) -> Result<(), NodeManagerError> {
        self.client
            .set_scripts(scripts, command)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    /// Returns the list of scripts currently being tracked by the light client.
    pub fn get_scripts(&self) -> Result<Vec<ScriptStatus>, NodeManagerError> {
        self.client
            .get_scripts()
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }
}

impl CkbRpc for LightClientRpc {
    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError> {
        self.client
            .get_tip_header()
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_cells(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>, NodeManagerError> {
        self.client
            .get_cells(search_key, order, limit.into(), after)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_cells_capacity(
        &self,
        search_key: SearchKey,
    ) -> Result<Option<CellsCapacity>, NodeManagerError> {
        // Light client returns CellsCapacity directly (not Option), wrap in Some.
        let capacity = self
            .client
            .get_cells_capacity(search_key)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;
        Ok(Some(capacity))
    }

    fn send_transaction(
        &self,
        tx: ckb_jsonrpc_types::Transaction,
    ) -> Result<H256, NodeManagerError> {
        self.client
            .send_transaction(tx)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_transaction(&self, hash: H256) -> Result<Option<TransactionStatus>, NodeManagerError> {
        let resp = self
            .client
            .get_transaction(hash)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

        // The light client's TransactionWithStatus has private fields.
        // Round-trip through serde_json to extract the data.
        Ok(resp.map(|r| {
            let json_value = serde_json::to_value(&r).unwrap_or_default();
            let transaction = json_value.get("transaction").and_then(|t| {
                serde_json::from_value::<ckb_jsonrpc_types::TransactionView>(t.clone()).ok()
            });
            let status = json_value
                .get("tx_status")
                .and_then(|s| s.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            let block_hash = json_value
                .get("tx_status")
                .and_then(|s| s.get("block_hash"))
                .and_then(|h| serde_json::from_value::<H256>(h.clone()).ok());
            TransactionStatus {
                transaction,
                status,
                block_hash,
            }
        }))
    }

    fn get_header(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>, NodeManagerError> {
        self.client
            .get_header(hash)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_transaction_with_status(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::TransactionWithStatusResponse>, NodeManagerError> {
        // Light client returns a different type, we need to convert
        let resp = self
            .client
            .get_transaction(hash)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

        // Convert light client's TransactionWithStatus to the full node's format
        Ok(resp.and_then(|r| {
            let json_value = serde_json::to_value(&r).ok()?;
            serde_json::from_value(json_value).ok()
        }))
    }

    fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, NodeManagerError> {
        let resp = self
            .client
            .get_transactions(search_key, order, limit.into(), after)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

        // TODO: Create an issue on CKB SDK.
        // The light client's TxWithCell/TxWithCells have private fields (likely
        // an oversight in ckb-sdk — the indexer equivalents are pub). This forces a
        // JSON round-trip to extract data. Remove this if the SDK makes them public.
        // The light client uses `transaction` (full TransactionView) where the
        // indexer uses `tx_hash` (H256 only). Transform `transaction.hash` → `tx_hash`.
        let mut json_value =
            serde_json::to_value(&resp).map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

        if let Some(objects) = json_value.get_mut("objects").and_then(|v| v.as_array_mut()) {
            for obj in objects.iter_mut() {
                if let Some(map) = obj.as_object_mut() {
                    if let Some(tx_hash) =
                        map.get("transaction").and_then(|t| t.get("hash")).cloned()
                    {
                        map.remove("transaction");
                        map.insert("tx_hash".to_string(), tx_hash);
                    }
                }
            }
        }

        serde_json::from_value(json_value).map_err(|e| {
            NodeManagerError::RpcError(format!(
                "Failed to normalize light client transactions: {}",
                e
            ))
        })
    }

    fn get_rpc_url(&self) -> String {
        self.rpc_url.clone()
    }
}

/// Creates the appropriate RPC client based on the node configuration.
///
/// - `PublicRpc` and `FullNode` use `FullNodeRpc` (full CKB RPC interface).
/// - `LightClient` uses `LightClientRpc`.
///
/// Returns an `Arc` so the client can be shared across threads via
/// `NodeManager::clone()`. For light-client-specific operations (e.g.
/// `set_scripts`), use `connect_light_client` instead.
pub fn connect(config: &NodeConfig) -> Arc<dyn CkbRpc> {
    match config.node_type {
        NodeType::PublicRpc | NodeType::FullNode => Arc::new(FullNodeRpc::new(&config.rpc_url)),
        NodeType::LightClient => Arc::new(LightClientRpc::new(&config.rpc_url)),
    }
}

/// Creates a light-client-specific RPC connection for operations
/// like `set_scripts` and `get_scripts` that are not part of the
/// unified `CkbRpc` trait.
pub fn connect_light_client(config: &NodeConfig) -> Result<LightClientRpc, NodeManagerError> {
    if config.node_type != NodeType::LightClient {
        return Err(NodeManagerError::UnsupportedOperation {
            node_type: config.node_type.to_string(),
            reason: "Light client RPC is only available when node_type is LightClient.".to_string(),
        });
    }
    Ok(LightClientRpc::new(&config.rpc_url))
}
