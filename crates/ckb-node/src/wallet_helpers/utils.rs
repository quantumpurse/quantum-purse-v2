//! Shared utilities for wallet-domain helpers.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ckb_jsonrpc_types::{OutPoint, Script};

use crate::client::{FullNodeClient, QpClient};
use crate::config::{NetworkType, NodeConfig, NodeType};
use crate::error::NodeManagerError;

/// How long a found mainnet code cell is reused before it is looked up
/// again. An upgrade of the lock is rare, so a wallet left open catches up
/// within this time without asking a node on every transaction.
const QR_LOCK_DEP_FOUND_TTL: Duration = Duration::from_secs(600);

/// How long to wait after a lookup that failed or found nothing before the
/// lookup is tried again. Meanwhile the last cell found is reused, or the
/// pinned one when none ever was. Short, so a node that was briefly
/// unreachable doesn't hide an upgrade for long.
const QR_LOCK_DEP_FALLBACK_TTL: Duration = Duration::from_secs(60);

/// The mainnet code cell chosen for one backend. Mainnet only: the testnet
/// out point is never looked up, so it is never stored here.
struct CachedQrLockDep {
    /// The backend this was chosen for. Another backend ignores the entry
    /// and looks the cell up itself, so a node that lags behind an upgrade
    /// can't hand its stale answer to a healthy one.
    rpc_url: String,
    at: Instant,
    out_point: OutPoint,
    /// Whether the last lookup found the cell. `false` after one that failed
    /// or found nothing: the out point is then the last one a lookup did
    /// find for this backend, or the pinned one when none ever did.
    found: bool,
}

/// Stores the outpoint for different backends.
/// One entry per backend, found by `rpc_url`, so the work still running for
/// a backend the app has left can't overwrite the entry of the one in use.
static QR_LOCK_DEP_CACHE: Mutex<Vec<CachedQrLockDep>> = Mutex::new(Vec::new());

fn lock_qr_lock_dep_cache() -> std::sync::MutexGuard<'static, Vec<CachedQrLockDep>> {
    QR_LOCK_DEP_CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The out point pinned in the constants for `network`: the testnet code
/// cell, or where the mainnet one sat when the constants were written.
fn fallback_qr_lock_dep(network: NetworkType) -> Result<OutPoint, NodeManagerError> {
    let (tx_hash_hex, index) = match network {
        NetworkType::Mainnet => (
            qpv2_core::constants::CKB_MAINNET_CELL_DEP_TX_HASH,
            qpv2_core::constants::CKB_MAINNET_CELL_DEP_INDEX,
        ),
        NetworkType::Testnet => (
            qpv2_core::constants::CKB_TESTNET_CELL_DEP_TX_HASH,
            qpv2_core::constants::CKB_TESTNET_CELL_DEP_INDEX,
        ),
    };
    let tx_hash: ckb_types::H256 = tx_hash_hex
        .trim_start_matches("0x")
        .parse()
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid QR lock dep tx hash: {}", e)))?;
    Ok(OutPoint {
        tx_hash,
        index: index.into(),
    })
}

/// The type script of the mainnet lock's code cell: its Type ID. Its hash is
/// `CKB_MAINNET_CODE_HASH`, which is how a mainnet address names the lock.
fn mainnet_qr_lock_type_id() -> Result<Script, NodeManagerError> {
    let args = hex::decode(qpv2_core::constants::CKB_MAINNET_TYPE_ID_ARGS.trim_start_matches("0x"))
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid QR lock Type ID args: {}", e)))?;
    Ok(Script {
        code_hash: ckb_sdk::constants::TYPE_ID_CODE_HASH,
        hash_type: ckb_jsonrpc_types::ScriptHashType::Type,
        args: ckb_jsonrpc_types::JsonBytes::from_bytes(args.into()),
    })
}

/// Where the lock script's code cell is believed to sit now. May ask a node,
/// so it blocks for up to the RPC timeout: call it off the UI thread.
///
/// On mainnet the live cell is looked up by the lock's Type ID, because an
/// upgrade moves the code to a new cell, while testnet always uses the out
/// point in the constants.
///
/// A full node or the public RPC runs the lookup itself, and a light client
/// asks the public RPC, since it can't search by Type ID.
///
/// A lookup that fails or finds nothing keeps the last cell found for this
/// backend, or falls back to the pinned out point when none ever was, so a
/// brief outage doesn't stop sending while the lock hasn't been upgraded.
///
/// The choice is kept per backend for a while, so a node is not asked on
/// every transaction.
pub(crate) fn locate_qr_lock_dep(qp_client: &QpClient) -> Result<OutPoint, NodeManagerError> {
    let network = qp_client.config().network;
    let pinned = fallback_qr_lock_dep(network)?;

    // QR lock testnet deployment uses data2, not type id, so no need to look up.
    if network != NetworkType::Mainnet {
        return Ok(pinned);
    }
    let rpc_url = &qp_client.config().rpc_url;

    // Scoped so the cache lock is released before the lookup below, which
    // can take seconds and would otherwise block every other caller.
    {
        let cache = lock_qr_lock_dep_cache();
        if let Some(entry) = cache.iter().find(|e| e.rpc_url == *rpc_url) {
            let ttl = if entry.found {
                QR_LOCK_DEP_FOUND_TTL
            } else {
                QR_LOCK_DEP_FALLBACK_TTL
            };
            if entry.at.elapsed() < ttl {
                return Ok(entry.out_point.clone());
            }
        }
    }

    // use full node rpc client instance to make the lookup call.
    // if the client is already full node, use the existing instance.
    let type_id = mainnet_qr_lock_type_id()?;
    let looked_up = match qp_client.as_any().downcast_ref::<FullNodeClient>() {
        Some(full) => full.find_live_cell_by_type_script(type_id),
        None => {
            let url = NodeConfig::default_rpc_url_for(NodeType::PublicRpc, network);
            FullNodeClient::new(url).find_live_cell_by_type_script(type_id)
        }
    };

    // Save the search result, keeping the earlier cell when the search failed.
    let mut cache = lock_qr_lock_dep_cache();
    let previous = cache.iter().find(|e| e.rpc_url == *rpc_url);
    let (out_point, found) = match (looked_up, previous) {
        (Ok(Some(out_point)), _) => (out_point, true),
        // A lookup that failed or found nothing says nothing about where the
        // cell is, so the last answer for this backend stands, or the pinned
        // cell when there is none yet.
        (Ok(None) | Err(_), previous) => (previous.map_or(pinned, |e| e.out_point.clone()), false),
    };

    // Replace this backend's entry.
    cache.retain(|e| e.rpc_url != *rpc_url);
    cache.push(CachedQrLockDep {
        rpc_url: rpc_url.clone(),
        at: Instant::now(),
        out_point: out_point.clone(),
        found,
    });
    Ok(out_point)
}

/// Checks, on a light client that has fetched `out_point`'s transaction,
/// that the cell there really carries the lock's Type ID. This catches a
/// public RPC that pointed at some other cell. It does not prove the cell is
/// still live, which only the network decides when the transaction is sent.
/// On a mismatch the cell is forgotten, so the next call looks it up again,
/// and this is an error. A testnet cell needs no check.
pub(crate) fn confirm_qr_lock_dep(
    qp_client: &QpClient,
    out_point: &OutPoint,
) -> Result<(), NodeManagerError> {
    if qp_client.config().network != NetworkType::Mainnet {
        return Ok(());
    }
    let type_id = mainnet_qr_lock_type_id()?;
    let carried = qp_client
        .get_transaction(out_point.tx_hash.clone())?
        .and_then(|status| status.transaction)
        .and_then(|tx| {
            tx.inner
                .outputs
                .get(out_point.index.value() as usize)
                .cloned()
        })
        .and_then(|output| output.type_);
    if carried == Some(type_id) {
        return Ok(());
    }

    // Forget only the entry that names this cell: another worker's lookup
    // may have replaced it meanwhile, and that one is left alone.
    let rpc_url = &qp_client.config().rpc_url;
    lock_qr_lock_dep_cache().retain(|e| !(e.rpc_url == *rpc_url && e.out_point == *out_point));
    Err(NodeManagerError::RpcError(
        "The cell the public RPC named doesn't carry the lock script's Type ID.".to_string(),
    ))
}

/// Builds the Quantum Resistant lock script for `network` with the given
/// lock args (empty args + the indexer's default prefix mode match all
/// cells under the contract).
pub(crate) fn build_qr_lock_script(
    network: NetworkType,
    lock_args_hex: &str,
) -> Result<ckb_jsonrpc_types::Script, NodeManagerError> {
    let (code_hash_hex, hash_type) = match network {
        NetworkType::Mainnet => (
            qpv2_core::constants::CKB_MAINNET_CODE_HASH,
            qpv2_core::constants::CKB_MAINNET_HASH_TYPE,
        ),
        NetworkType::Testnet => (
            qpv2_core::constants::CKB_TESTNET_CODE_HASH,
            qpv2_core::constants::CKB_TESTNET_HASH_TYPE,
        ),
    };
    let hash_type: ckb_jsonrpc_types::ScriptHashType = hash_type.into();

    // H256: FromStr does the hex-decode and 32-byte length check in one
    // step (same idiom as tx_builder/utils.rs).
    let code_hash: ckb_types::H256 = code_hash_hex
        .trim_start_matches("0x")
        .parse()
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid QR lock code hash: {}", e)))?;

    let args_clean = lock_args_hex.strip_prefix("0x").unwrap_or(lock_args_hex);
    let args = hex::decode(args_clean)
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid lock args hex: {}", e)))?;

    Ok(ckb_jsonrpc_types::Script {
        code_hash,
        hash_type,
        args: ckb_jsonrpc_types::JsonBytes::from_bytes(args.into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lookup only finds the lock's code cell if the Type ID pinned here
    /// is the one mainnet addresses name, so the two constants must agree.
    #[test]
    fn mainnet_type_id_hashes_to_the_mainnet_code_hash() {
        let type_id: ckb_types::packed::Script = mainnet_qr_lock_type_id().unwrap().into();
        assert_eq!(
            format!("{:#x}", type_id.calc_script_hash()),
            qpv2_core::constants::CKB_MAINNET_CODE_HASH
        );
    }
}
