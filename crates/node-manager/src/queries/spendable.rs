//! Spendable-capacity queries.
//!
//! "Spendable" means live cells with no type script and no output data —
//! the only cells that can be freely consumed as inputs for a transfer.

use crate::error::NodeManagerError;
use crate::rpc::CkbRpc;
use ckb_sdk::traits::{CellCollector, CellQueryOptions, DefaultCellCollector, LiveCell, ValueRangeOption};
use ckb_types::packed::Script;
use ckb_types::prelude::*;

/// Collects all spendable cells for a given lock script.
/// "Spendable" means live cells with no type script and no output data.
pub(crate) fn collect_spendable_cells(
    rpc_url: &str,
    lock_script: &Script,
) -> Result<Vec<LiveCell>, NodeManagerError> {
    let mut cell_collector = DefaultCellCollector::new(rpc_url);
    let mut query = CellQueryOptions::new_lock(lock_script.clone());
    query.secondary_script_len_range = Some(ValueRangeOption::new_exact(0));
    query.data_len_range = Some(ValueRangeOption::new_exact(0));
    query.min_total_capacity = u64::MAX;

    let (cells, _) = cell_collector
        .collect_live_cells(&query, false)
        .map_err(|e| NodeManagerError::RpcError(e.to_string()))?;

    if cells.is_empty() {
        return Err(NodeManagerError::RpcError(
            "No spendable cells available.".to_string(),
        ));
    }

    Ok(cells)
}

/// Returns the total spendable capacity (in shannons) for the given address.
/// "Spendable" means live cells with no type script and no output data.
pub fn spendable_capacity(
    rpc: &dyn CkbRpc,
    from_address: &ckb_sdk::Address,
) -> Result<u64, NodeManagerError> {
    let rpc_url = rpc.get_rpc_url();
    let lock_script = Script::from(from_address.payload());
    let cells = collect_spendable_cells(&rpc_url, &lock_script)?;
    Ok(cells
        .iter()
        .map(|c| {
            let cap: u64 = c.output.capacity().unpack();
            cap
        })
        .sum())
}
