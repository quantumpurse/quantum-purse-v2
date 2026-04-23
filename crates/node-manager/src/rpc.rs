use crate::config::NetworkType;
use crate::error::NodeManagerError;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::rpc::ckb_indexer::{Cell, CellsCapacity, Order, Pagination, ScriptType, SearchKey, Tx};
use ckb_sdk::rpc::ckb_light_client::{LightClientRpcClient, ScriptStatus, SetScriptsCommand};
use ckb_sdk::rpc::{CkbRpcClient, ResponseFormatGetter};
use ckb_types::H256;
use std::any::Any;

/// Unified RPC interface for wallet operations.
///
/// Abstracts over full node (`CkbRpcClient`) and light client (`LightClientRpcClient`)
/// so wallet code does not need to know which backend is active.
///
/// The `Send + Sync` supertraits allow a single client instance to be shared
/// across background threads via `Arc<dyn CkbRpc>` inside `NodeManager`.
///
/// `Any` enables downcasting from `Arc<dyn CkbRpc>` back to the concrete
/// type when callers need backend-specific methods (e.g.
/// `LightClientRpc::register_lock_script`) — avoids constructing a second
/// RPC client when one already exists.
pub trait CkbRpc: Send + Sync + Any {
    /// Concrete-type access for downcast. Each impl returns `self` so
    /// callers can do `self.rpc.as_any().downcast_ref::<LightClientRpc>()`.
    fn as_any(&self) -> &dyn Any;

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

    /// Number of peers the node is currently connected to.
    pub fn get_peer_count(&self) -> Result<usize, NodeManagerError> {
        self.client
            .get_peers()
            .map(|peers| peers.len())
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }
}

impl CkbRpc for FullNodeRpc {
    fn as_any(&self) -> &dyn Any {
        self
    }

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

    /// Number of peers the light client is currently connected to. Used by
    /// the Node Manager UI as a liveness / connectivity indicator.
    pub fn get_peer_count(&self) -> Result<usize, NodeManagerError> {
        self.client
            .get_peers()
            .map(|peers| peers.len())
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    /// Registers a QuantumPurse lock script with the light client so it
    /// begins indexing matching cells from `start_block`. Builds the
    /// `ScriptStatus` from network-specific code_hash/hash_type constants
    /// and calls `set_scripts` with `Partial` so existing registrations
    /// survive.
    pub fn register_lock_script(
        &self,
        lock_args_hex: &str,
        network: NetworkType,
        start_block: u64,
    ) -> Result<(), NodeManagerError> {
        let (code_hash_hex, hash_type_str) = match network {
            NetworkType::Mainnet => (
                qpv2_core::constants::CKB_MAINNET_CODE_HASH,
                qpv2_core::constants::CKB_MAINNET_HASH_TYPE,
            ),
            NetworkType::Testnet => (
                qpv2_core::constants::CKB_TESTNET_CODE_HASH,
                qpv2_core::constants::CKB_TESTNET_HASH_TYPE,
            ),
        };

        let script_hash_type = match hash_type_str {
            "type" => ckb_jsonrpc_types::ScriptHashType::Type,
            "data1" => ckb_jsonrpc_types::ScriptHashType::Data1,
            _ => ckb_jsonrpc_types::ScriptHashType::Data,
        };

        let code_hash_clean = code_hash_hex.strip_prefix("0x").unwrap_or(code_hash_hex);
        let mut code_hash_bytes = [0u8; 32];
        let decoded = hex::decode(code_hash_clean)
            .map_err(|e| NodeManagerError::RpcError(format!("Invalid code hash hex: {}", e)))?;
        if decoded.len() != 32 {
            return Err(NodeManagerError::RpcError(format!(
                "Code hash must be 32 bytes, got {}.",
                decoded.len()
            )));
        }
        code_hash_bytes.copy_from_slice(&decoded);

        let lock_args_clean = lock_args_hex.strip_prefix("0x").unwrap_or(lock_args_hex);
        let args_bytes = hex::decode(lock_args_clean)
            .map_err(|e| NodeManagerError::RpcError(format!("Invalid lock args hex: {}", e)))?;

        let script = ckb_jsonrpc_types::Script {
            code_hash: H256(code_hash_bytes),
            hash_type: script_hash_type,
            args: JsonBytes::from_bytes(args_bytes.into()),
        };

        let status = ScriptStatus {
            script,
            script_type: ScriptType::Lock,
            block_number: start_block.into(),
        };

        self.set_scripts(vec![status], Some(SetScriptsCommand::Partial))
    }
}

impl CkbRpc for LightClientRpc {
    fn as_any(&self) -> &dyn Any {
        self
    }

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
