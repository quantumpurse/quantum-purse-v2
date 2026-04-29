//! CkbClient layer — protocol speakers + the App-facing handle.
//!
//! The crate's "talk-to-a-node" surface lives here, at three honest
//! levels of abstraction:
//!
//! - [`CkbClient`] — the trait describing what wallet code needs from any
//!   backend. Concrete impls are in submodules; consumers pass them
//!   around as `&dyn CkbClient` or `Arc<dyn CkbClient>`.
//! - [`FullNodeClient`] / [`LightClient`] — concrete backends. Wrap
//!   the upstream `ckb_sdk::CkbRpcClient` and
//!   `ckb_sdk::LightClientRpcClient` respectively, normalize their
//!   return shapes, and expose a few backend-specific methods (peer
//!   count, light-client filter scripts, etc.) that callers can reach
//!   via `as_any().downcast_ref::<…>()`.
//! - [`QpClient`] — the cloneable handle the App holds. Bundles
//!   `Arc<dyn CkbClient>` with the `NodeConfig` snapshot it was built
//!   from, so background threads carry one cheap clone instead of
//!   capturing the rpc and a fistful of config scalars separately.
//!
//! Plus two free helpers — [`peer_count`] and [`synced_block`] — that
//! consult a `&dyn CkbClient` without caring which concrete impl is
//! behind it.

use std::any::Any;
use std::sync::Arc;

use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::rpc::ckb_indexer::{Cell, CellsCapacity, Order, Pagination, SearchKey, Tx};
use ckb_sdk::traits::{
    CellCollector, CellQueryOptions, HeaderDepResolver, LiveCell, TransactionDependencyProvider,
};
use ckb_types::H256;

use crate::config::{NetworkType, NodeConfig, NodeType};
use crate::error::NodeManagerError;

mod full;
mod light;

pub use full::FullNodeClient;
pub use light::LightClient;

/// Unified RPC interface for wallet operations.
///
/// Abstracts over full node and light client so wallet code does not
/// need to know which backend is active.
///
/// The `Send + Sync` supertraits allow a single client instance to be
/// shared across background threads via `Arc<dyn CkbClient>` inside
/// `QpClient`.
///
/// `Any` enables downcasting from `Arc<dyn CkbClient>` back to the
/// concrete type when callers need backend-specific methods (e.g.
/// `LightClient::set_scripts`) — avoids constructing a second RPC
/// client when one already exists.
pub trait CkbClient: Send + Sync + Any {
    /// Concrete-type access for downcast. Each impl returns `self` so
    /// callers can do `client.as_any().downcast_ref::<LightClient>()`.
    fn as_any(&self) -> &dyn Any;

    /// Returns the tip (latest) block header.
    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError>;

    /// Returns the genesis block. Full nodes serve this via
    /// `get_block_by_number(0)`; the light client has a dedicated
    /// `get_genesis_block` RPC. Used to bootstrap the system-script
    /// `CellDepResolver` for transaction building.
    fn get_genesis_block(&self) -> Result<ckb_jsonrpc_types::BlockView, NodeManagerError>;

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

    /// Gets the RPC URL (temporary method for SDK components).
    /// TODO: Remove when we implement custom collectors using the trait.
    fn get_rpc_url(&self) -> String;
}

/// Simplified transaction status returned by `CkbClient::get_transaction`.
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
/// Bundles the protocol speaker (`Arc<dyn CkbClient>`) with the
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
    ckb_client: Arc<dyn CkbClient>,
    config: NodeConfig,
}

impl QpClient {
    /// Builds a fresh handle bound to `config`. Constructs the
    /// concrete `CkbClient` impl appropriate for `config.node_type` via
    /// [`build`].
    pub fn new(config: NodeConfig) -> Self {
        let ckb_client = build(&config);
        Self { ckb_client, config }
    }

    /// Returns a cloned `Arc` handle to the rpc client. Use when moving
    /// the client into a background thread independent of `self`.
    pub fn ckb_client(&self) -> Arc<dyn CkbClient> {
        self.ckb_client.clone()
    }

    /// Returns a borrowed view of the rpc client. Use in synchronous
    /// code that does not need to outlive `self`.
    pub fn client_ref(&self) -> &dyn CkbClient {
        self.ckb_client.as_ref()
    }

    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    pub fn network(&self) -> NetworkType {
        self.config.network
    }

    pub fn node_type(&self) -> NodeType {
        self.config.node_type
    }

    pub fn is_mainnet(&self) -> bool {
        self.config.network == NetworkType::Mainnet
    }
}

/// Builds the right `Arc<dyn CkbClient>` for the given config. The active
/// backend determines the concrete RPC client; the App owns the single
/// shared instance and replaces it on every config change.
pub fn build(config: &NodeConfig) -> Arc<dyn CkbClient> {
    match config.node_type {
        NodeType::PublicRpc | NodeType::FullNode => Arc::new(FullNodeClient::new(&config.rpc_url)),
        NodeType::LightClient => Arc::new(LightClient::new(&config.rpc_url)),
    }
}

/// Number of peers for local-node backends. `Ok(None)` for `PublicRpc`
/// (peer count of a remote endpoint isn't meaningful wallet-side).
/// `Err` when the local node is unreachable.
pub fn peer_count(
    ckb_client: &dyn CkbClient,
    node_type: NodeType,
) -> Result<Option<usize>, NodeManagerError> {
    if let Some(light) = ckb_client.as_any().downcast_ref::<LightClient>() {
        return light.get_peer_count().map(Some);
    }
    if let Some(full) = ckb_client.as_any().downcast_ref::<FullNodeClient>() {
        // FullNodeClient is shared by FullNode + PublicRpc backends; only
        // the local FullNode case has a meaningful peer count.
        return match node_type {
            NodeType::FullNode => full.get_peer_count().map(Some),
            _ => Ok(None),
        };
    }
    Ok(None)
}

/// Min synced block across all scripts the LC is tracking. `Ok(None)`
/// outside `LightClient` and when no scripts are registered.
pub fn synced_block(client: &dyn CkbClient) -> Result<Option<u64>, NodeManagerError> {
    let Some(light) = client.as_any().downcast_ref::<LightClient>() else {
        return Ok(None);
    };
    let scripts = light.get_scripts()?;
    Ok(scripts.iter().map(|s| s.block_number.value()).min())
}
