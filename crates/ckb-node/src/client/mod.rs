//! UnifiedClient layer — protocol speakers + the App-facing handle.
//!
//! The crate's "talk-to-a-node" surface lives here, at three honest
//! levels of abstraction:
//!
//! - [`UnifiedClient`] — the trait describing what wallet code needs from any
//!   backend. Concrete impls are in submodules; consumers pass them
//!   around as `&dyn UnifiedClient` or `Arc<dyn UnifiedClient>`.
//! - [`FullNodeClient`] / [`LightClient`] — concrete backends. Wrap
//!   the upstream `ckb_sdk::CkbRpcClient` and
//!   `ckb_sdk::LightClientRpcClient` respectively, normalize their
//!   return shapes, and expose a few backend-specific methods (peer
//!   count, light-client filter scripts, etc.) that callers can reach
//!   via `as_any().downcast_ref::<…>()`.
//! - [`QpClient`] — the cloneable handle the App holds. Bundles
//!   `Arc<dyn UnifiedClient>` with the `NodeConfig` snapshot it was built
//!   from, so background threads carry one cheap clone instead of
//!   capturing the rpc and a fistful of config scalars separately.
//!
//! Plus free helpers — [`synced_block`], etc. — that consult a
//! `&dyn UnifiedClient` without caring which concrete impl is behind it.

use std::any::Any;
use std::sync::Arc;

use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::rpc::ckb_indexer::{CellsCapacity, Order, Pagination, SearchKey, Tx};
use ckb_sdk::traits::{
    CellCollector, CellQueryOptions, HeaderDepResolver, LiveCell, TransactionDependencyProvider,
};
use ckb_types::H256;

use crate::config::{NetworkType, NodeConfig, NodeType};
use crate::error::NodeManagerError;

mod full;
mod light;

pub use full::FullNodeClient;
pub(crate) use light::LightClient;

/// Unified RPC interface for wallet operations.
///
/// Abstracts over full node and light client so wallet code does not
/// need to know which backend is active.
///
/// The `Send + Sync` supertraits allow a single client instance to be
/// shared across background threads via `Arc<dyn UnifiedClient>` inside
/// `QpClient`.
///
/// `Any` enables downcasting from `Arc<dyn UnifiedClient>` back to the
/// concrete type when callers need backend-specific methods (e.g.
/// `LightClient::set_scripts`) — avoids constructing a second RPC
/// client when one already exists.
trait UnifiedClient: Send + Sync + Any {
    /// Concrete-type access for downcast. Each impl returns `self` so
    /// callers can do `client.as_any().downcast_ref::<LightClient>()`.
    fn as_any(&self) -> &dyn Any;

    /// Returns the tip (latest) block header.
    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError>;

    fn get_peers(&self) -> Result<Vec<ckb_jsonrpc_types::RemoteNode>, NodeManagerError>;

    /// Returns the genesis block. Full nodes serve this via
    /// `get_block_by_number(0)`; the light client has a dedicated
    /// `get_genesis_block` RPC. Used to bootstrap the system-script
    /// `CellDepResolver` for transaction building.
    fn get_genesis_block(&self) -> Result<ckb_jsonrpc_types::BlockView, NodeManagerError>;

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

    /// Queries transactions matching the given search key via the indexer.
    fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, NodeManagerError>;

    /// Collects live cells matching `query`. Each backend picks its best
    /// strategy: full node delegates to ckb-sdk's `DefaultCellCollector`;
    /// light client paginates its indexer (`get_cells`) directly.
    fn collect_cells(&self, query: &CellQueryOptions) -> Result<Vec<LiveCell>, NodeManagerError>;

    /// Returns a fresh ckb-sdk `CellCollector` bound to this backend.
    /// Used by tx builders that consume `&mut dyn CellCollector` (e.g.
    /// `build_balanced`). Full node returns `DefaultCellCollector`; light
    /// client returns `LightClientCellCollector`.
    fn cell_collector(&self) -> Box<dyn CellCollector>;

    /// Returns a fresh ckb-sdk `HeaderDepResolver` bound to this backend.
    /// Used by tx builders that consume `&dyn HeaderDepResolver`.
    fn header_dep_resolver(&self) -> Box<dyn HeaderDepResolver>;

    /// Returns a fresh ckb-sdk `TransactionDependencyProvider` bound to
    /// this backend. Used by tx builders that consume
    /// `&dyn TransactionDependencyProvider`.
    fn tx_dep_provider(&self) -> Box<dyn TransactionDependencyProvider>;

    fn local_node_info(&self) -> Result<ckb_jsonrpc_types::LocalNode, NodeManagerError>;
}

/// Simplified transaction status returned by `UnifiedClient::get_transaction`.
///
/// Normalizes the different response types from full node and light
/// client into a common representation.
#[derive(Debug, Clone)]
pub struct TransactionStatus {
    /// The transaction view, if available.
    pub transaction: Option<ckb_jsonrpc_types::TransactionView>,
    /// Transaction status string: "pending", "proposed", "committed", "rejected", or "unknown".
    pub status: String,
    /// Block hash of the block that committed this transaction, if committed.
    pub block_hash: Option<H256>,
}

/// Cloneable handle to the active CKB backend.
///
/// Bundles the protocol speaker (`Arc<dyn UnifiedClient>`) with the
/// `NodeConfig` snapshot it was built from. This is the unit that
/// background threads carry: one cheap clone gives them the rpc client
/// plus the backend-shape knowledge (`network`, `node_type`,
/// `is_mainnet`) they need to call `wallet_helpers::*` correctly without
/// having to consult any single-owner state.
///
/// Lifecycle: replaced wholesale on backend switch — `App` builds a new
/// `QpClient` from the new config, stores it, and lets the old one
/// drop when its last in-flight thread finishes its work. The trait
/// object behind the `Arc` keeps speaking to the old backend until it
/// does; this is intentional, and the only correct way to retire an
/// HTTP client that may be mid-request.
#[derive(Clone)]
pub struct QpClient {
    unified_client: Arc<dyn UnifiedClient>,
    config: NodeConfig,
}

impl QpClient {
    /// Builds a fresh handle bound to `config`. Constructs the
    /// concrete `UnifiedClient` impl appropriate for `config.node_type` via
    /// [`build`].
    pub fn new(config: NodeConfig) -> Self {
        let unified_client = build(&config);
        Self {
            unified_client,
            config,
        }
    }

    /// Returns a reference to the `NodeConfig` snapshot this handle was built from.
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Returns the network (Mainnet/Testnet) the active backend is bound to.
    pub fn network(&self) -> NetworkType {
        self.config.network
    }

    /// Returns the backend kind (`PublicRpc`, `FullNode`, or `LightClient`).
    pub fn node_type(&self) -> NodeType {
        self.config.node_type
    }

    /// True if this handle is bound to the mainnet network.
    pub fn is_mainnet(&self) -> bool {
        self.config.network == NetworkType::Mainnet
    }

    /// Returns the JSON-RPC endpoint URL this handle is bound to.
    fn rpc_url(&self) -> &str {
        &self.config.rpc_url
    }

    /// Sends a batch of JSON-RPC calls in a single HTTP POST and returns
    /// the results in request order. Each element of `calls` is a
    /// `(method, params)` pair. Returns one `serde_json::Value` per call
    /// (the `"result"` field); callers deserialize themselves.
    pub fn batch_rpc(
        &self,
        calls: &[(&str, serde_json::Value)],
    ) -> Result<Vec<serde_json::Value>, NodeManagerError> {
        if calls.is_empty() {
            return Ok(vec![]);
        }

        let batch: Vec<serde_json::Value> = calls
            .iter()
            .enumerate()
            .map(|(id, (method, params))| {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                })
            })
            .collect();

        let resp = reqwest::blocking::Client::new()
            .post(self.rpc_url())
            .json(&batch)
            .send()
            .map_err(|e| NodeManagerError::RpcError(format!("Batch RPC HTTP error: {}", e)))?
            .error_for_status()
            .map_err(|e| NodeManagerError::RpcError(format!("Batch RPC HTTP {}", e)))?;

        let mut results: Vec<serde_json::Value> = resp
            .json()
            .map_err(|e| NodeManagerError::RpcError(format!("Batch RPC parse error: {}", e)))?;

        if results.len() != calls.len() {
            return Err(NodeManagerError::RpcError(format!(
                "Batch RPC: expected {} results, got {}",
                calls.len(),
                results.len(),
            )));
        }

        results.sort_by_key(|r| r.get("id").and_then(|v| v.as_u64()).unwrap_or(0));

        results
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                if let Some(err) = r.get("error") {
                    let method = calls[i].0;
                    return Err(NodeManagerError::RpcError(format!(
                        "Batch RPC '{}': {}",
                        method, err
                    )));
                }
                Ok(r.get("result").cloned().unwrap_or(serde_json::Value::Null))
            })
            .collect()
    }

    /// Returns the tip (latest) block header from the active backend.
    pub fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError> {
        self.unified_client.get_tip_header()
    }

    /// Retrieves a transaction by hash, returning its normalized status.
    pub fn get_transaction(
        &self,
        hash: H256,
    ) -> Result<Option<TransactionStatus>, NodeManagerError> {
        self.unified_client.get_transaction(hash)
    }

    /// Collects live cells matching `options`, using whichever strategy the
    /// active backend prefers (see [`UnifiedClient::collect_cells`]).
    pub fn collect_cells(
        &self,
        options: &CellQueryOptions,
    ) -> Result<Vec<LiveCell>, NodeManagerError> {
        self.unified_client.collect_cells(options)
    }

    /// Returns the total capacity of live cells matching `search_key`.
    pub fn get_cells_capacity(
        &self,
        search_key: SearchKey,
    ) -> Result<Option<CellsCapacity>, NodeManagerError> {
        self.unified_client.get_cells_capacity(search_key)
    }

    /// Gets a header by its block hash.
    pub fn get_header(
        &self,
        hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::HeaderView>, NodeManagerError> {
        self.unified_client.get_header(hash)
    }

    /// Queries indexer-side transactions matching `search_key`, paginated.
    pub fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: u32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, NodeManagerError> {
        self.unified_client
            .get_transactions(search_key, order, limit, after)
    }

    /// Returns a fresh ckb-sdk `CellCollector` bound to the active backend,
    /// for tx builders that consume `&mut dyn CellCollector`.
    pub fn cell_collector(&self) -> Box<dyn CellCollector> {
        self.unified_client.cell_collector()
    }

    /// Returns a fresh ckb-sdk `HeaderDepResolver` bound to the active backend,
    /// for tx builders that consume `&dyn HeaderDepResolver`.
    pub fn header_dep_resolver(&self) -> Box<dyn HeaderDepResolver> {
        self.unified_client.header_dep_resolver()
    }

    /// Returns a fresh ckb-sdk `TransactionDependencyProvider` bound to the
    /// active backend, for tx builders that consume
    /// `&dyn TransactionDependencyProvider`.
    pub fn tx_dep_provider(&self) -> Box<dyn TransactionDependencyProvider> {
        self.unified_client.tx_dep_provider()
    }

    /// Returns the genesis block. Used to bootstrap the system-script
    /// `CellDepResolver` for transaction building.
    pub fn get_genesis_block(&self) -> Result<ckb_jsonrpc_types::BlockView, NodeManagerError> {
        self.unified_client.get_genesis_block()
    }

    /// Submits a transaction to the network and returns its hash.
    pub fn send_transaction(
        &self,
        tx: ckb_jsonrpc_types::Transaction,
    ) -> Result<H256, NodeManagerError> {
        self.unified_client.send_transaction(tx)
    }

    /// Concrete-type access for downcast — e.g.
    /// `client.as_any().downcast_ref::<LightClient>()` to reach
    /// backend-specific methods like `LightClient::set_scripts`.
    pub fn as_any(&self) -> &dyn Any {
        self.unified_client.as_any()
    }

    pub fn get_peers(
        &self,
    ) -> Result<Vec<ckb_jsonrpc_types::RemoteNode>, NodeManagerError> {
        self.unified_client.get_peers()
    }

    /// Min synced block across all scripts the LC is tracking. `Ok(None)`
    /// outside `LightClient` and when no scripts are registered.
    pub fn synced_block(&self) -> Result<Option<u64>, NodeManagerError> {
        let Some(light) = self.unified_client.as_any().downcast_ref::<LightClient>() else {
            return Ok(None);
        };
        let scripts = light.get_scripts()?;
        Ok(scripts.iter().map(|s| s.block_number.value()).min())
    }

    /// Full node IBD progress and phase (header sync / block download /
    /// verifying / synced). `Ok(None)` for `LightClient` (no analogue)
    /// and `PublicRpc` (remote endpoint's sync isn't *our* sync, same
    /// policy as `peer_count`). `Some(_)` only for `FullNode`.
    pub fn sync_state(&self) -> Result<Option<ckb_jsonrpc_types::SyncState>, NodeManagerError> {
        if self.config.node_type != NodeType::FullNode {
            return Ok(None);
        }
        let Some(full) = self
            .unified_client
            .as_any()
            .downcast_ref::<FullNodeClient>()
        else {
            return Ok(None);
        };
        full.sync_state().map(Some)
    }

    pub fn blockchain_info(
        &self,
    ) -> Result<Option<ckb_jsonrpc_types::ChainInfo>, NodeManagerError> {
        let Some(full) = self
            .unified_client
            .as_any()
            .downcast_ref::<FullNodeClient>()
        else {
            return Ok(None);
        };
        full.get_blockchain_info().map(Some)
    }

    pub fn tx_pool_info(
        &self,
    ) -> Result<Option<ckb_jsonrpc_types::TxPoolInfo>, NodeManagerError> {
        let Some(full) = self
            .unified_client
            .as_any()
            .downcast_ref::<FullNodeClient>()
        else {
            return Ok(None);
        };
        full.tx_pool_info().map(Some)
    }

    pub fn local_node_info(
        &self,
    ) -> Result<ckb_jsonrpc_types::LocalNode, NodeManagerError> {
        self.unified_client.local_node_info()
    }
}

/// Builds the right `Arc<dyn UnifiedClient>` for the given config. The active
/// backend determines the concrete RPC client; the App owns the single
/// shared instance and replaces it on every config change.
fn build(config: &NodeConfig) -> Arc<dyn UnifiedClient> {
    match config.node_type {
        NodeType::PublicRpc | NodeType::FullNode => Arc::new(FullNodeClient::new(&config.rpc_url)),
        NodeType::LightClient => Arc::new(LightClient::new(&config.rpc_url)),
    }
}
