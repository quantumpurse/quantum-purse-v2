//! DAO cell queries: classify an address's DAO cells as deposited vs prepared
//! and compute the maximum withdrawable capacity for prepared cells.

use crate::error::NodeManagerError;
use crate::rpc::CkbRpc;
use byteorder::{ByteOrder, LittleEndian};
use ckb_sdk::{
    constants::DAO_TYPE_HASH,
    traits::{CellCollector, CellQueryOptions, DefaultCellCollector, LiveCell, ValueRangeOption},
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
    /// Block number in which the deposit transaction was committed.
    pub deposit_block_number: u64,
    /// Block number in which the prepare transaction was committed.
    pub prepare_block_number: u64,
}

/// Queries all DAO cells for an address and partitions them into deposited and
/// prepared cells.
// TODO: rename to `categorize_dao_cells` (current name is a typo).
pub fn categozire_dao_cells(
    rpc: &dyn CkbRpc,
    address: &Address,
) -> Result<(Vec<DepositedCell>, Vec<PreparedCell>), NodeManagerError> {
    let rpc_url = rpc.get_rpc_url();
    let cells = collect_dao_cells(&rpc_url, address)?;

    let mut deposited = Vec::new();
    let mut prepared = Vec::new();
    for cell in cells {
        if cell.output_data.len() != 8 {
            continue;
        }

        // The DAO cell data tells us whether this is a deposit cell or a
        // withdrawn cell waiting to be unlocked. If the first 8 bytes are
        // all zero, it's a deposit cell; otherwise it's a prepared cell.
        let cell_data = LittleEndian::read_u64(&cell.output_data.as_ref()[0..8]);
        if cell_data == 0 {
            deposited.push(DepositedCell {
                out_point: cell.out_point,
                capacity: cell.output.capacity().unpack(),
                block_number: cell.block_number,
            });
        } else {
            let (max_withdraw, deposit_block_number, prepare_block_number) =
                calculate_max_withdraw(rpc, &cell)?;
            prepared.push(PreparedCell {
                out_point: cell.out_point,
                capacity: cell.output.capacity().unpack(),
                maximum_withdraw: max_withdraw,
                deposit_block_number,
                prepare_block_number,
            });
        }
    }

    Ok((deposited, prepared))
}

/// Collects all DAO cells for a given address from the indexer.
///
/// Returns the raw `LiveCell` list so callers can partition into
/// deposited vs prepared.
fn collect_dao_cells(
    rpc_url: &str,
    address: &Address,
) -> Result<Vec<LiveCell>, NodeManagerError> {
    let lock_script = Script::from(address.payload());
    let mut cell_collector = DefaultCellCollector::new(rpc_url);

    let dao_type_script = Script::new_builder()
        .code_hash(DAO_TYPE_HASH.pack())
        .hash_type(ScriptHashType::Type)
        .build();

    let mut query = CellQueryOptions::new_lock(lock_script);
    query.secondary_script = Some(dao_type_script);
    query.data_len_range = Some(ValueRangeOption::new_exact(8));
    query.min_total_capacity = u64::MAX;

    let (cells, _) = cell_collector
        .collect_live_cells(&query, false)
        .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

    Ok(cells)
}

/// Calculates the maximum withdrawable capacity for a prepared DAO cell.
///
/// Follows the same logic as ckb-cli's `calculate_dao_maximum_withdraw`:
/// 1. Fetch the prepare transaction to find the deposit out_point.
/// 2. Fetch the deposit transaction to get the original output.
/// 3. Fetch both block headers (deposit and prepare).
/// 4. Compute the DAO interest using `calculate_dao_maximum_withdraw4`.
///
/// Returns `(maximum_withdraw, deposit_block_number, prepare_block_number)`.
fn calculate_max_withdraw(
    rpc: &dyn CkbRpc,
    cell: &LiveCell,
) -> Result<(u64, u64, u64), NodeManagerError> {
    let prepare_tx_hash: H256 = cell.out_point.tx_hash().unpack();
    let prepare_output_index: u32 = cell.out_point.index().unpack();

    // 1. Get the prepare transaction and its block hash.
    let prepare_tx_status = rpc
        .get_transaction(prepare_tx_hash.clone())?
        .ok_or_else(|| {
            NodeManagerError::RpcError(format!(
                "Prepare transaction {} not found.",
                prepare_tx_hash
            ))
        })?;

    let prepare_block_hash = prepare_tx_status.block_hash.ok_or_else(|| {
        NodeManagerError::RpcError("Prepare transaction is not committed.".to_string())
    })?;

    let prepare_tx_view = prepare_tx_status.transaction.ok_or_else(|| {
        NodeManagerError::RpcError("Prepare transaction has no data.".to_string())
    })?;

    // 2. Extract the deposit out_point from the prepare tx input
    //    at the same index as the prepared cell output.
    let prepare_tx: ckb_types::packed::Transaction = prepare_tx_view.inner.into();
    let prepare_tx = prepare_tx.into_view();

    let deposit_out_point = prepare_tx
        .inputs()
        .get(prepare_output_index as usize)
        .ok_or_else(|| {
            NodeManagerError::RpcError(format!(
                "Input index {} not found in prepare transaction.",
                prepare_output_index
            ))
        })?
        .previous_output();

    // 3. Get the deposit transaction and its block hash.
    let deposit_tx_hash: H256 = deposit_out_point.tx_hash().unpack();
    let deposit_tx_status = rpc
        .get_transaction(deposit_tx_hash.clone())?
        .ok_or_else(|| {
            NodeManagerError::RpcError(format!(
                "Deposit transaction {} not found.",
                deposit_tx_hash
            ))
        })?;

    let deposit_block_hash = deposit_tx_status.block_hash.ok_or_else(|| {
        NodeManagerError::RpcError("Deposit transaction is not committed.".to_string())
    })?;

    let deposit_tx_view = deposit_tx_status.transaction.ok_or_else(|| {
        NodeManagerError::RpcError("Deposit transaction has no data.".to_string())
    })?;

    let deposit_tx: ckb_types::packed::Transaction = deposit_tx_view.inner.into();
    let deposit_tx = deposit_tx.into_view();

    // 4. Get the original deposit output and data.
    let deposit_index: u32 = deposit_out_point.index().unpack();
    let (output, output_data) = deposit_tx
        .output_with_data(deposit_index as usize)
        .ok_or_else(|| {
            NodeManagerError::RpcError(format!(
                "Output index {} not found in deposit transaction.",
                deposit_index
            ))
        })?;

    // 5. Fetch both headers.
    let deposit_header: HeaderView = rpc
        .get_header(deposit_block_hash)?
        .ok_or_else(|| NodeManagerError::RpcError("Deposit block header not found.".to_string()))?
        .into();

    let prepare_header: HeaderView = rpc
        .get_header(prepare_block_hash)?
        .ok_or_else(|| NodeManagerError::RpcError("Prepare block header not found.".to_string()))?
        .into();

    // 6. Calculate the maximum withdraw amount.
    let occupied_capacity = output
        .occupied_capacity(Capacity::bytes(output_data.len()).unwrap())
        .map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to calculate occupied capacity: {}", e))
        })?;

    let max_withdraw = ckb_sdk::util::calculate_dao_maximum_withdraw4(
        &deposit_header,
        &prepare_header,
        &output,
        occupied_capacity.as_u64(),
    );

    let deposit_block_number: u64 = deposit_header.number();
    let prepare_block_number: u64 = prepare_header.number();

    Ok((max_withdraw, deposit_block_number, prepare_block_number))
}
