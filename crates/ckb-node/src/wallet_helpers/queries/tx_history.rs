//! Transaction history queries for QuantumPurse lock scripts.

use crate::config::NetworkType;
use crate::error::NodeManagerError;
use crate::rpc::Client;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::rpc::ckb_indexer::{Order, ScriptType, SearchKey, SearchKeyFilter, Tx};

/// Queries all transactions for a QuantumPurse lock script via the indexer.
///
/// Paginates through the full result set using `last_cursor`. Returns grouped
/// `Tx` entries in descending order (newest first), one per unique transaction.
pub fn fetch_recent_transactions(
    client: &dyn Client,
    lock_args_hex: &str,
    network: NetworkType,
    after_block: Option<u64>,
    limit: Option<usize>,
) -> Result<Vec<Tx>, NodeManagerError> {
    let (code_hash_str, hash_type_str) = match network {
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

    let code_hash = code_hash_str.strip_prefix("0x").unwrap_or(code_hash_str);
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
            block_range: after_block.map(|b| {
                [
                    ckb_jsonrpc_types::Uint64::from(b + 1),
                    ckb_jsonrpc_types::Uint64::from(u64::MAX),
                ]
            }),
        }),
        with_data: None,
        group_by_transaction: Some(true),
    };

    // Paginate through results (newest first).
    let page_size = 100;
    let mut all_txs = Vec::new();
    let mut cursor: Option<JsonBytes> = None;

    loop {
        let page = client.get_transactions(search_key.clone(), Order::Desc, page_size, cursor)?;
        let is_last = page.objects.len() < page_size as usize;
        all_txs.extend(page.objects);

        if let Some(max) = limit {
            if all_txs.len() >= max {
                all_txs.truncate(max);
                break;
            }
        }

        if is_last {
            break;
        }
        cursor = Some(page.last_cursor);
    }

    Ok(all_txs)
}
