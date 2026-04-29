use crate::config::{NetworkType, NodeConfig, NodeType};
use crate::error::NodeManagerError;
use ckb_jsonrpc_types::{JsonBytes, Uint64};
use ckb_sdk::rpc::ckb_indexer::{
    Cell, CellsCapacity, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter, Tx,
};
use ckb_sdk::rpc::ckb_light_client::{
    FetchStatus, LightClientRpcClient, ScriptStatus, SetScriptsCommand,
};
use ckb_sdk::rpc::{CkbRpcClient, ResponseFormatGetter};
use ckb_sdk::traits::{
    CellCollector, CellQueryOptions, DefaultCellCollector, DefaultHeaderDepResolver,
    DefaultTransactionDependencyProvider, HeaderDepResolver, LightClientCellCollector,
    LightClientHeaderDepResolver, LightClientTransactionDependencyProvider, LiveCell,
    PrimaryScriptType, TransactionDependencyProvider, ValueRangeOption,
};
use ckb_types::prelude::*;
use ckb_types::H256;
use std::any::Any;
use std::sync::Arc;

/// Unified RPC interface for wallet operations.
///
/// Abstracts over full node (`CkbRpcClient`) and light client (`LightClientRpcClient`)
/// so wallet code does not need to know which backend is active.
///
/// The `Send + Sync` supertraits allow a single client instance to be shared
/// across background threads via `Arc<dyn Client>` inside `LocalNodeProcess`.
///
/// `Any` enables downcasting from `Arc<dyn Client>` back to the concrete
/// type when callers need backend-specific methods (e.g.
/// `LightClient::register_lock_script`) — avoids constructing a second
/// RPC client when one already exists.
pub trait Client: Send + Sync + Any {
    /// Concrete-type access for downcast. Each impl returns `self` so
    /// callers can do `self.rpc.as_any().downcast_ref::<LightClient>()`.
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
    ///
    /// Closes the cell-collection escape hatch — call sites used to
    /// construct `DefaultCellCollector::new(rpc_url)` directly, which only
    /// speaks full-node RPC and fails on a light client.
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
pub struct FullNodeClient {
    client: CkbRpcClient,
    rpc_url: String,
}

impl FullNodeClient {
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

    /// Light-client utility: best-effort discovery of the earliest
    /// funding block across a set of QuantumPurse accounts.
    ///
    /// Although this method lives on `FullNodeClient`, its sole purpose is
    /// to support the **light-client backend's** manual "Auto-detect
    /// rescan block" flow. The light client only indexes scripts it has
    /// been told to track — it cannot itself answer "when was account X
    /// first funded?" The wallet therefore constructs an ad-hoc
    /// `FullNodeClient` against a richly-indexed public endpoint, calls
    /// this method, and uses the result to pre-fill the user's
    /// `set_scripts(start_block)` input on the LC.
    ///
    /// For each account, asks the indexer for the earliest tx
    /// (`Order::Asc`, limit 1) involving that lock script and returns
    /// the minimum block number across all accounts. `Ok(None)` when
    /// no account has any indexer history.
    ///
    /// Returns the **raw** earliest block as the indexer reports it.
    /// Callers wiring this into the LC's `set_scripts` MUST account
    /// for the upstream LC's "already-filtered up to and including N"
    /// semantics (subtract 1 — sync resumes at `block_number + 1`, so
    /// passing the raw earliest would skip the very tx we just
    /// discovered).
    pub fn find_earliest_funding_block(
        &self,
        lock_args_list: &[String],
        network: NetworkType,
    ) -> Result<Option<u64>, NodeManagerError> {
        if lock_args_list.is_empty() {
            return Ok(None);
        }

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

        let mut earliest: Option<u64> = None;
        for lock_args_hex in lock_args_list {
            let lock_args_clean = lock_args_hex.strip_prefix("0x").unwrap_or(lock_args_hex);
            let args_bytes = hex::decode(lock_args_clean)
                .map_err(|e| NodeManagerError::RpcError(format!("Invalid lock args hex: {}", e)))?;

            let script = ckb_jsonrpc_types::Script {
                code_hash: H256(code_hash_bytes),
                hash_type: script_hash_type,
                args: JsonBytes::from_bytes(args_bytes.into()),
            };
            let search_key = SearchKey {
                script,
                script_type: ScriptType::Lock,
                script_search_mode: None,
                filter: None,
                with_data: None,
                group_by_transaction: None,
            };

            let page = self
                .client
                .get_transactions(search_key, Order::Asc, 1u32.into(), None)
                .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

            if let Some(first) = page.objects.first() {
                let block_num = match first {
                    Tx::Ungrouped(t) => t.block_number.value(),
                    Tx::Grouped(t) => t.block_number.value(),
                };
                earliest = Some(earliest.map_or(block_num, |e| e.min(block_num)));
            }
        }
        Ok(earliest)
    }
}

impl Client for FullNodeClient {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError> {
        self.client
            .get_tip_header()
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_genesis_block(&self) -> Result<ckb_jsonrpc_types::BlockView, NodeManagerError> {
        self.client
            .get_block_by_number(0u64.into())
            .map_err(|e| {
                NodeManagerError::RpcError(format!("Failed to fetch genesis block: {}", e))
            })?
            .ok_or_else(|| NodeManagerError::RpcError("Genesis block not found.".to_string()))
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

    fn collect_cells(&self, query: &CellQueryOptions) -> Result<Vec<LiveCell>, NodeManagerError> {
        let mut collector = DefaultCellCollector::new(&self.rpc_url);
        let (cells, _) = collector
            .collect_live_cells(query, false)
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;
        Ok(cells)
    }

    fn cell_collector(&self) -> Box<dyn CellCollector> {
        Box::new(DefaultCellCollector::new(&self.rpc_url))
    }

    fn header_dep_resolver(&self) -> Box<dyn HeaderDepResolver> {
        Box::new(DefaultHeaderDepResolver::new(&self.rpc_url))
    }

    fn tx_dep_provider(&self) -> Box<dyn TransactionDependencyProvider> {
        Box::new(DefaultTransactionDependencyProvider::new(&self.rpc_url, 10))
    }

    fn get_rpc_url(&self) -> String {
        self.rpc_url.clone()
    }
}

/// Light client RPC implementation.
///
/// Provides the same `Client` interface plus light-client-specific methods
/// for script registration.
pub struct LightClient {
    client: LightClientRpcClient,
    rpc_url: String,
}

impl LightClient {
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

    /// Asks the light client to pull `tx_hash` (and its committing block
    /// header) into its local store. Returns `true` only when the tx is
    /// already in the store (`FetchStatus::Fetched`); any other status
    /// (`Added`, `Fetching`, `NotFound`) returns `false` — the caller
    /// shouldn't proceed with operations that depend on the tx.
    ///
    /// Idempotent: cached lookups return immediately without a network
    /// hit (verified in `vendor/ckb-light-client/.../rpc.rs:868`).
    pub fn fetch_transaction(&self, tx_hash: H256) -> Result<bool, NodeManagerError> {
        match self.client.fetch_transaction(tx_hash) {
            Ok(FetchStatus::Fetched { .. }) => Ok(true),
            Ok(_) => Ok(false),
            Err(e) => Err(NodeManagerError::RpcError(e.to_string())),
        }
    }

    /// Registers one or more QuantumPurse lock scripts with the light
    /// client so it indexes matching cells. Each entry is a
    /// `(lock_args_hex, start_block)` pair. Submits all in one
    /// `set_scripts(Partial)` call.
    ///
    /// Filters against `get_scripts` first so already-tracked scripts
    /// are skipped — their existing sync cursors stay put. This is the
    /// safe path used by the auto-flow (account creation, network
    /// switch). For deliberate cursor reset (manual UI) use
    /// [`Self::register_all_lock_scripts`].
    ///
    /// Why filter
    /// ----------
    /// `set_scripts(Partial)` overwrites the stored block_number for
    /// any script that's already registered (upstream LC's
    /// `update_filter_scripts` always `put`s the new value, never
    /// merges). LC data dirs are per-network and persistent, so when
    /// the user switches mainnet ↔ testnet the target network's
    /// RocksDB may already contain our scripts with valid sync cursors
    /// from a prior session. Naively re-registering at `tip` would
    /// yank those cursors forward and silently skip any blocks indexed
    /// since the last visit.
    pub fn register_lock_scripts(
        &self,
        scripts: &[(&str, u64)],
        network: NetworkType,
    ) -> Result<(), NodeManagerError> {
        if scripts.is_empty() {
            return Ok(());
        }

        // Set of lock_args bytes the LC already tracks. Only this wallet
        // talks to this LC, so every entry came from us — comparing by
        // lock_args alone is enough. One RPC call (`get_scripts`); the
        // rest is local.
        let existing: std::collections::HashSet<Vec<u8>> = self
            .client
            .get_scripts()
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))?
            .into_iter()
            .map(|ss| ss.script.args.as_bytes().to_vec())
            .collect();

        // Drop already-tracked entries; only newly-seen scripts go through.
        let filtered: Vec<(&str, u64)> = scripts
            .iter()
            .copied()
            .filter(|(args_hex, _)| {
                let clean = args_hex.strip_prefix("0x").unwrap_or(args_hex);
                match hex::decode(clean) {
                    Ok(bytes) => !existing.contains(&bytes),
                    // Invalid hex: let the helper surface a precise error
                    // when it tries to decode the same value.
                    Err(_) => true,
                }
            })
            .collect();

        let statuses = build_lock_script_statuses(&filtered, network)?;
        if statuses.is_empty() {
            return Ok(());
        }
        self.set_scripts(statuses, Some(SetScriptsCommand::Partial))
    }

    /// Force-applies a `set_scripts(Partial)` call for every entry,
    /// **without** filtering. The user's intent is to deliberately
    /// reset (rewind or advance) the LC's stored sync cursor for these
    /// scripts — typically wired to a manual "set scan from block N"
    /// UI control.
    ///
    /// Same construction logic as [`Self::register_lock_scripts`]; only
    /// the filter is skipped. Empty input is a no-op.
    pub fn register_all_lock_scripts(
        &self,
        scripts: &[(&str, u64)],
        network: NetworkType,
    ) -> Result<(), NodeManagerError> {
        if scripts.is_empty() {
            return Ok(());
        }
        let statuses = build_lock_script_statuses(scripts, network)?;
        self.set_scripts(statuses, Some(SetScriptsCommand::Partial))
    }
}

/// Builds `ScriptStatus`es for a list of `(lock_args_hex, start_block)`
/// pairs against the active network's code_hash/hash_type. Pure
/// construction — no RPC. Shared between `register_lock_scripts` (auto,
/// filtered) and `set_all_lock_scripts` (manual, force).
fn build_lock_script_statuses(
    scripts: &[(&str, u64)],
    network: NetworkType,
) -> Result<Vec<ScriptStatus>, NodeManagerError> {
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

    let mut statuses: Vec<ScriptStatus> = Vec::with_capacity(scripts.len());
    for (lock_args_hex, start_block) in scripts {
        let lock_args_clean = lock_args_hex.strip_prefix("0x").unwrap_or(lock_args_hex);
        let args_bytes = hex::decode(lock_args_clean)
            .map_err(|e| NodeManagerError::RpcError(format!("Invalid lock args hex: {}", e)))?;

        let script = ckb_jsonrpc_types::Script {
            code_hash: H256(code_hash_bytes),
            hash_type: script_hash_type,
            args: JsonBytes::from_bytes(args_bytes.into()),
        };

        statuses.push(ScriptStatus {
            script,
            script_type: ScriptType::Lock,
            block_number: (*start_block).into(),
        });
    }

    Ok(statuses)
}

impl Client for LightClient {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_tip_header(&self) -> Result<ckb_jsonrpc_types::HeaderView, NodeManagerError> {
        self.client
            .get_tip_header()
            .map_err(|e| NodeManagerError::RpcError(e.to_string()))
    }

    fn get_genesis_block(&self) -> Result<ckb_jsonrpc_types::BlockView, NodeManagerError> {
        self.client.get_genesis_block().map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to fetch genesis block: {}", e))
        })
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

    fn collect_cells(&self, query: &CellQueryOptions) -> Result<Vec<LiveCell>, NodeManagerError> {
        // Translate ckb-sdk's `CellQueryOptions` into a light-client indexer
        // `SearchKey`. Then page through `get_cells` until either the total
        // capacity reaches `query.min_total_capacity` or the indexer runs out.
        //
        // `CellQueryOptions::maturity` and `query.limit` are not honored —
        // neither current call site (spendable, dao_cells) sets them, and
        // light-client indexer responses don't carry the cellbase metadata
        // needed for proper maturity filtering.
        let primary_type = match query.primary_type {
            PrimaryScriptType::Lock => ScriptType::Lock,
            PrimaryScriptType::Type => ScriptType::Type,
        };

        let to_range = |opt: Option<ValueRangeOption>| -> Option<[Uint64; 2]> {
            opt.map(|r| [Uint64::from(r.start), Uint64::from(r.end)])
        };

        let filter = SearchKeyFilter {
            script: query.secondary_script.clone().map(|s| s.into()),
            script_len_range: to_range(query.secondary_script_len_range),
            output_data: None,
            output_data_filter_mode: None,
            output_data_len_range: to_range(query.data_len_range),
            output_capacity_range: to_range(query.capacity_range),
            block_range: to_range(query.block_range),
        };

        let search_key = SearchKey {
            script: query.primary_script.clone().into(),
            script_type: primary_type,
            script_search_mode: None,
            filter: Some(filter),
            with_data: Some(true),
            group_by_transaction: None,
        };

        let page_size: u32 = 100;
        let mut after: Option<JsonBytes> = None;
        let mut collected: Vec<LiveCell> = Vec::new();
        let mut total_capacity: u64 = 0;

        loop {
            let page = self
                .client
                .get_cells(
                    search_key.clone(),
                    Order::Asc,
                    page_size.into(),
                    after.clone(),
                )
                .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

            if page.objects.is_empty() {
                break;
            }

            for cell in page.objects {
                let output: ckb_types::packed::CellOutput = cell.output.into();
                let output_data = cell.output_data.map(|b| b.into_bytes()).unwrap_or_default();
                let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
                let block_number: u64 = cell.block_number.value();
                let tx_index: u32 = cell.tx_index.value();
                let capacity: u64 = output.capacity().unpack();

                collected.push(LiveCell {
                    output,
                    output_data,
                    out_point,
                    block_number,
                    tx_index,
                });
                total_capacity = total_capacity.saturating_add(capacity);

                if query.min_total_capacity > 0 && total_capacity >= query.min_total_capacity {
                    return Ok(collected);
                }
            }

            if page.last_cursor.is_empty() {
                break;
            }
            after = Some(page.last_cursor);
        }

        Ok(collected)
    }

    fn cell_collector(&self) -> Box<dyn CellCollector> {
        Box::new(LightClientCellCollector::new(&self.rpc_url))
    }

    fn header_dep_resolver(&self) -> Box<dyn HeaderDepResolver> {
        Box::new(LightClientHeaderDepResolver::new(&self.rpc_url))
    }

    fn tx_dep_provider(&self) -> Box<dyn TransactionDependencyProvider> {
        Box::new(LightClientTransactionDependencyProvider::new(&self.rpc_url))
    }

    fn get_rpc_url(&self) -> String {
        self.rpc_url.clone()
    }
}

// ── Top-level helpers (no LocalNodeProcess required) ──────────────────────────

/// Builds the right `Arc<dyn Client>` for the given config. The active
/// backend determines the concrete RPC client; the App owns the single
/// shared instance and replaces it on every config change.
pub fn build(config: &NodeConfig) -> Arc<dyn Client> {
    match config.node_type {
        NodeType::PublicRpc | NodeType::FullNode => Arc::new(FullNodeClient::new(&config.rpc_url)),
        NodeType::LightClient => Arc::new(LightClient::new(&config.rpc_url)),
    }
}

/// Number of peers for local-node backends. `Ok(None)` for `PublicRpc`
/// (peer count of a remote endpoint isn't meaningful wallet-side).
/// `Err` when the local node is unreachable.
pub fn peer_count(
    client: &dyn Client,
    node_type: NodeType,
) -> Result<Option<usize>, NodeManagerError> {
    if let Some(light) = client.as_any().downcast_ref::<LightClient>() {
        return light.get_peer_count().map(Some);
    }
    if let Some(full) = client.as_any().downcast_ref::<FullNodeClient>() {
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
pub fn synced_block(rpc: &dyn Client) -> Result<Option<u64>, NodeManagerError> {
    let Some(light) = rpc.as_any().downcast_ref::<LightClient>() else {
        return Ok(None);
    };
    let scripts = light.get_scripts()?;
    Ok(scripts.iter().map(|s| s.block_number.value()).min())
}
