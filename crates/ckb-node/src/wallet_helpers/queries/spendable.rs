//! Spendable-capacity queries.
//!
//! "Spendable" means live cells with no type script and no output data —
//! the only cells that can be freely consumed as inputs for a transfer.

use crate::client::QpClient;
use crate::error::NodeManagerError;
use ckb_sdk::traits::{CellQueryOptions, LiveCell, ValueRangeOption};
use ckb_types::packed::Script;

/// Collects all spendable cells for a given lock script.
/// "Spendable" means live cells with no type script and no output data.
pub(crate) fn collect_spendable_cells(
    qp_client: &QpClient,
    lock_script: &Script,
) -> Result<Vec<LiveCell>, NodeManagerError> {
    let mut query = CellQueryOptions::new_lock(lock_script.clone());
    query.secondary_script_len_range = Some(ValueRangeOption::new_exact(0));
    query.data_len_range = Some(ValueRangeOption::new_exact(0));
    query.min_total_capacity = u64::MAX;

    let cells = qp_client.collect_cells(&query)?;

    if cells.is_empty() {
        return Err(NodeManagerError::RpcError(
            "No spendable cells available.".to_string(),
        ));
    }

    Ok(cells)
}

/// Returns the total spendable capacity (in shannons) via a single
/// `get_cells_capacity` RPC call — no cell collection or pagination.
pub fn spendable_capacity(
    qp_client: &QpClient,
    from_address: &ckb_sdk::Address,
) -> Result<u64, NodeManagerError> {
    use ckb_sdk::rpc::ckb_indexer::{ScriptType, SearchKey, SearchKeyFilter};

    let lock_script: ckb_jsonrpc_types::Script = Script::from(from_address.payload()).into();
    let search_key = SearchKey {
        script: lock_script,
        script_type: ScriptType::Lock,
        script_search_mode: None,
        filter: Some(SearchKeyFilter {
            script: None,
            script_len_range: Some([0u64.into(), 1u64.into()]),
            output_data: None,
            output_data_filter_mode: None,
            output_data_len_range: Some([0u64.into(), 1u64.into()]),
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
