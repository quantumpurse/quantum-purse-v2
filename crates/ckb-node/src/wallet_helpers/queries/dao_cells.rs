//! DAO cell queries: classify an address's DAO cells as deposited vs prepared
//! and compute the maximum withdrawable capacity for prepared cells.

use std::collections::{HashMap, HashSet};

use crate::client::QpClient;
use crate::error::NodeManagerError;
use byteorder::{ByteOrder, LittleEndian};
use ckb_sdk::{
    constants::DAO_TYPE_HASH,
    traits::{CellQueryOptions, LiveCell, ValueRangeOption},
    Address,
};
use ckb_types::{
    core::{Capacity, HeaderView, ScriptHashType},
    packed::{OutPoint, Script},
    prelude::*,
    H256,
};

/// Represents a deposited DAO cell.
#[derive(Debug, Clone)]
pub struct DepositedCell {
    /// On-chain out_point identifying this cell.
    pub out_point: OutPoint,
    /// Deposited capacity in shannons.
    pub capacity: u64,
    /// Block number in which the deposit transaction was committed.
    pub block_number: u64,
}

/// Represents a prepared DAO cell ready for withdrawal.
#[derive(Debug, Clone)]
pub struct PreparedCell {
    /// On-chain out_point identifying this cell.
    pub out_point: OutPoint,
    /// Original deposited capacity in shannons.
    pub capacity: u64,
    /// Maximum withdrawable capacity (principal + interest) in shannons.
    pub maximum_withdraw: u64,
    /// Header of the block in which the deposit transaction was committed.
    pub deposit_header: HeaderView,
    /// Header of the block in which the prepare transaction was committed.
    pub prepare_header: HeaderView,
}

/// Helper for deserializing `get_transaction` batch results.
#[derive(serde::Deserialize)]
struct TxResponse {
    transaction: Option<ckb_jsonrpc_types::TransactionView>,
    tx_status: TxStatusResponse,
}

#[derive(serde::Deserialize)]
struct TxStatusResponse {
    block_hash: Option<H256>,
}

/// Queries all DAO cells for an address and partitions them into deposited and
/// prepared cells. Prepared cells use batched RPC to compute max-withdraw in
/// 3 round-trips instead of 4*N sequential calls.
pub fn categorize_dao_cells(
    qp_client: &QpClient,
    address: &Address,
) -> Result<(Vec<DepositedCell>, Vec<PreparedCell>), NodeManagerError> {
    let cells = collect_dao_cells(qp_client, address)?;

    // Phase 1: Classify cells — deposited vs prepared.
    let mut deposited = Vec::new();
    let mut prepared_cells: Vec<LiveCell> = Vec::new();
    for cell in cells {
        if cell.output_data.len() != 8 {
            continue;
        }
        let cell_data = LittleEndian::read_u64(&cell.output_data.as_ref()[0..8]);
        if cell_data == 0 {
            deposited.push(DepositedCell {
                out_point: cell.out_point.clone(),
                capacity: cell.output.capacity().unpack(),
                block_number: cell.block_number,
            });
        } else {
            prepared_cells.push(cell);
        }
    }

    if prepared_cells.is_empty() {
        return Ok((deposited, vec![]));
    }

    // Phase 2: Batch-fetch all prepare transactions (1 round-trip).
    let prepare_tx_hashes: Vec<H256> = prepared_cells
        .iter()
        .map(|c| c.out_point.tx_hash().unpack())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let calls: Vec<(&str, serde_json::Value)> = prepare_tx_hashes
        .iter()
        .map(|h| ("get_transaction", serde_json::json!([format!("{:#x}", h)])))
        .collect();
    let results = qp_client.batch_rpc(&calls)?;

    let mut prepare_tx_map: HashMap<H256, (ckb_jsonrpc_types::TransactionView, H256)> =
        HashMap::new();
    for (hash, result) in prepare_tx_hashes.iter().zip(results) {
        let resp: TxResponse = serde_json::from_value(result).map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to parse prepare tx {}: {}", hash, e))
        })?;
        let tx_view = resp.transaction.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Prepare transaction {} has no data.", hash))
        })?;
        let block_hash = resp.tx_status.block_hash.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Prepare transaction {} is not committed.", hash))
        })?;
        prepare_tx_map.insert(hash.clone(), (tx_view, block_hash));
    }

    // Phase 3: Extract deposit out_points, batch-fetch deposit transactions (1 round-trip).
    // Build a per-cell mapping: prepared cell -> (prepare_tx, prepare_block_hash, deposit_out_point).
    struct CellContext {
        prepare_block_hash: H256,
        deposit_out_point: OutPoint,
    }

    let mut cell_contexts: Vec<CellContext> = Vec::with_capacity(prepared_cells.len());
    let mut deposit_tx_hashes_set: HashSet<H256> = HashSet::new();

    for cell in &prepared_cells {
        let prepare_tx_hash: H256 = cell.out_point.tx_hash().unpack();
        let output_index: u32 = cell.out_point.index().unpack();

        let (tx_view, prepare_block_hash) =
            prepare_tx_map.get(&prepare_tx_hash).ok_or_else(|| {
                NodeManagerError::RpcError(format!(
                    "Prepare transaction {} missing from batch results.",
                    prepare_tx_hash
                ))
            })?;
        let prepare_tx: ckb_types::packed::Transaction = tx_view.inner.clone().into();
        let prepare_tx = prepare_tx.into_view();

        let deposit_out_point = prepare_tx
            .inputs()
            .get(output_index as usize)
            .ok_or_else(|| {
                NodeManagerError::RpcError(format!(
                    "Input index {} not found in prepare transaction {}.",
                    output_index, prepare_tx_hash
                ))
            })?
            .previous_output();

        let deposit_tx_hash: H256 = deposit_out_point.tx_hash().unpack();
        deposit_tx_hashes_set.insert(deposit_tx_hash);

        cell_contexts.push(CellContext {
            prepare_block_hash: prepare_block_hash.clone(),
            deposit_out_point,
        });
    }

    let deposit_tx_hashes: Vec<H256> = deposit_tx_hashes_set.into_iter().collect();
    let calls: Vec<(&str, serde_json::Value)> = deposit_tx_hashes
        .iter()
        .map(|h| ("get_transaction", serde_json::json!([format!("{:#x}", h)])))
        .collect();
    let results = qp_client.batch_rpc(&calls)?;

    let mut deposit_tx_map: HashMap<H256, (ckb_jsonrpc_types::TransactionView, H256)> =
        HashMap::new();
    for (hash, result) in deposit_tx_hashes.iter().zip(results) {
        let resp: TxResponse = serde_json::from_value(result).map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to parse deposit tx {}: {}", hash, e))
        })?;
        let tx_view = resp.transaction.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Deposit transaction {} has no data.", hash))
        })?;
        let block_hash = resp.tx_status.block_hash.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Deposit transaction {} is not committed.", hash))
        })?;
        deposit_tx_map.insert(hash.clone(), (tx_view, block_hash));
    }

    // Phase 4: Collect all unique block hashes, batch-fetch headers (1 round-trip).
    let mut block_hashes_set: HashSet<H256> = HashSet::new();
    for (_, block_hash) in prepare_tx_map.values() {
        block_hashes_set.insert(block_hash.clone());
    }
    for (_, block_hash) in deposit_tx_map.values() {
        block_hashes_set.insert(block_hash.clone());
    }
    let block_hashes: Vec<H256> = block_hashes_set.into_iter().collect();

    let calls: Vec<(&str, serde_json::Value)> = block_hashes
        .iter()
        .map(|h| ("get_header", serde_json::json!([format!("{:#x}", h)])))
        .collect();
    let results = qp_client.batch_rpc(&calls)?;

    let mut header_map: HashMap<H256, HeaderView> = HashMap::new();
    for (hash, result) in block_hashes.iter().zip(results) {
        let header: ckb_jsonrpc_types::HeaderView =
            serde_json::from_value(result).map_err(|e| {
                NodeManagerError::RpcError(format!("Failed to parse header {}: {}", hash, e))
            })?;
        header_map.insert(hash.clone(), header.into());
    }

    // Phase 5: Compute max-withdraw for each prepared cell (local only).
    let mut prepared = Vec::with_capacity(prepared_cells.len());
    for (cell, ctx) in prepared_cells.into_iter().zip(cell_contexts) {
        let deposit_tx_hash: H256 = ctx.deposit_out_point.tx_hash().unpack();
        let deposit_index: u32 = ctx.deposit_out_point.index().unpack();

        let (deposit_tx_view, deposit_block_hash) =
            deposit_tx_map.get(&deposit_tx_hash).ok_or_else(|| {
                NodeManagerError::RpcError(format!(
                    "Deposit transaction {} missing from batch results.",
                    deposit_tx_hash
                ))
            })?;
        let deposit_tx: ckb_types::packed::Transaction = deposit_tx_view.inner.clone().into();
        let deposit_tx = deposit_tx.into_view();

        let (output, output_data) = deposit_tx
            .output_with_data(deposit_index as usize)
            .ok_or_else(|| {
                NodeManagerError::RpcError(format!(
                    "Output index {} not found in deposit transaction {}.",
                    deposit_index, deposit_tx_hash
                ))
            })?;

        let deposit_header = header_map.get(deposit_block_hash).ok_or_else(|| {
            NodeManagerError::RpcError(format!("Deposit header {} not found.", deposit_block_hash))
        })?;
        let prepare_header = header_map.get(&ctx.prepare_block_hash).ok_or_else(|| {
            NodeManagerError::RpcError(format!(
                "Prepare header {} not found.",
                ctx.prepare_block_hash
            ))
        })?;

        let occupied_capacity = output
            .occupied_capacity(Capacity::bytes(output_data.len()).unwrap())
            .map_err(|e| {
                NodeManagerError::RpcError(format!("Failed to calculate occupied capacity: {}", e))
            })?;

        let max_withdraw = ckb_sdk::util::calculate_dao_maximum_withdraw4(
            deposit_header,
            prepare_header,
            &output,
            occupied_capacity.as_u64(),
        );

        prepared.push(PreparedCell {
            out_point: cell.out_point,
            capacity: cell.output.capacity().unpack(),
            maximum_withdraw: max_withdraw,
            deposit_header: deposit_header.clone(),
            prepare_header: prepare_header.clone(),
        });
    }

    Ok((deposited, prepared))
}

/// Collects all DAO cells for a given address from the indexer.
fn collect_dao_cells(
    qp_client: &QpClient,
    address: &Address,
) -> Result<Vec<LiveCell>, NodeManagerError> {
    let lock_script = Script::from(address.payload());

    let dao_type_script = Script::new_builder()
        .code_hash(DAO_TYPE_HASH.pack())
        .hash_type(ScriptHashType::Type)
        .build();

    let mut query = CellQueryOptions::new_lock(lock_script);
    query.secondary_script = Some(dao_type_script);
    query.data_len_range = Some(ValueRangeOption::new_exact(8));
    query.min_total_capacity = u64::MAX;

    qp_client.collect_cells(&query)
}
