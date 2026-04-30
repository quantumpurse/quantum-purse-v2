//! Balance queries for arbitrary and QuantumPurse-specific lock scripts.

use crate::client::QpClient;
use crate::config::NetworkType;
use crate::error::NodeManagerError;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::rpc::ckb_indexer::{ScriptType, SearchKey, SearchKeyFilter};

/// Queries the total balance (in shannons) for a lock script identified by
/// its code hash, hash type, and lock args.
///
/// This is a convenience wrapper that builds the CKB `SearchKey` internally
/// so callers don't need to depend on `ckb-sdk` or `ckb-jsonrpc-types`.
///
/// - `code_hash_hex`: hex-encoded code hash (with or without `0x` prefix).
/// - `hash_type_str`: one of `"type"`, `"data1"`, or `"data"`.
/// - `lock_args_hex`: hex-encoded lock args (with or without `0x` prefix).
pub fn fetch_lock_script_balance(
    qp_client: &QpClient,
    code_hash_hex: &str,
    hash_type_str: &str,
    lock_args_hex: &str,
) -> Result<u64, NodeManagerError> {
    let script_hash_type = match hash_type_str {
        "type" => ckb_jsonrpc_types::ScriptHashType::Type,
        "data1" => ckb_jsonrpc_types::ScriptHashType::Data1,
        _ => ckb_jsonrpc_types::ScriptHashType::Data,
    };

    let code_hash = code_hash_hex.strip_prefix("0x").unwrap_or(code_hash_hex);
    let code_hash_bytes: [u8; 32] = {
        let bytes = hex::decode(code_hash)
            .map_err(|e| NodeManagerError::RpcError(format!("Invalid code hash hex: {}", e)))?;
        if bytes.len() != 32 {
            return Err(NodeManagerError::RpcError(format!(
                "Code hash must be 32 bytes, got {}.",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        arr
    };

    let lock_args_clean = lock_args_hex.strip_prefix("0x").unwrap_or(lock_args_hex);
    let args_bytes = hex::decode(lock_args_clean)
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid lock args hex: {}", e)))?;

    let script = ckb_jsonrpc_types::Script {
        code_hash: ckb_types::H256(code_hash_bytes),
        hash_type: script_hash_type,
        args: JsonBytes::from_bytes(args_bytes.into()),
    };

    let search_key = SearchKey {
        script,
        script_type: ScriptType::Lock,
        script_search_mode: None,
        filter: Some(SearchKeyFilter {
            script: None,
            script_len_range: None,
            output_data: None,
            output_data_filter_mode: None,
            output_data_len_range: None,
            output_capacity_range: None,
            block_range: None,
        }),
        with_data: None,
        group_by_transaction: None,
    };

    match qp_client.get_cells_capacity(search_key)? {
        Some(capacity) => Ok(capacity.capacity.value()),
        None => Ok(0),
    }
}

/// Queries the total balance (in shannons) for a QuantumPurse lock script.
///
/// Selects the correct lock script deployment (code hash + hash type) for the
/// requested network, then delegates to `fetch_lock_script_balance`.
pub fn fetch_quantum_lock_balance(
    qp_client: &QpClient,
    lock_args_hex: &str,
    network: NetworkType,
) -> Result<u64, NodeManagerError> {
    let (code_hash, hash_type) = match network {
        NetworkType::Mainnet => (
            qpv2_core::constants::CKB_MAINNET_CODE_HASH,
            qpv2_core::constants::CKB_MAINNET_HASH_TYPE,
        ),
        NetworkType::Testnet => (
            qpv2_core::constants::CKB_TESTNET_CODE_HASH,
            qpv2_core::constants::CKB_TESTNET_HASH_TYPE,
        ),
    };

    fetch_lock_script_balance(qp_client, code_hash, hash_type, lock_args_hex)
}
