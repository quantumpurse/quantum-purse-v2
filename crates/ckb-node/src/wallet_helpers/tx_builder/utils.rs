//! Utilities for transaction building.

use crate::client::CkbClient;
use crate::error::NodeManagerError;
use ckb_sdk::traits::DefaultCellDepResolver;
use ckb_sdk::types::ScriptId;
use ckb_types::core::{BlockView, DepType, ScriptHashType};
use ckb_types::packed::{CellDep, OutPoint};
use ckb_types::prelude::*;
use ckb_types::H256;
use qpv2_core::constants;

/// Fetches the genesis block via the active backend's RPC and creates a
/// cell dep resolver with the quantum-resistant lock script registered.
///
/// Routes through `CkbClient::get_genesis_block` so it works on both the
/// full node (`get_block_by_number(0)`) and the light client
/// (`get_genesis_block`). The genesis block contains the system script
/// cell deps (sighash, multisig, DAO). The quantum-resistant lock
/// script is a custom deployment and must be registered explicitly
/// using the known deployment OutPoint.
pub fn cell_dep_resolver_from_rpc(
    ckb_client: &dyn CkbClient,
    is_mainnet: bool,
) -> Result<DefaultCellDepResolver, NodeManagerError> {
    let genesis_block = ckb_client.get_genesis_block()?;
    let block_view: BlockView = genesis_block.into();
    let mut resolver = DefaultCellDepResolver::from_genesis(&block_view).map_err(|e| {
        NodeManagerError::RpcError(format!("Failed to parse genesis info: {:?}", e))
    })?;

    // Register the quantum-resistant lock script cell dep.
    let (code_hash_hex, hash_type, dep_tx_hash_hex, dep_index) = if is_mainnet {
        (
            constants::CKB_MAINNET_CODE_HASH,
            ScriptHashType::Type,
            constants::CKB_MAINNET_CELL_DEP_TX_HASH,
            constants::CKB_MAINNET_CELL_DEP_INDEX,
        )
    } else {
        (
            constants::CKB_TESTNET_CODE_HASH,
            ScriptHashType::Data1,
            constants::CKB_TESTNET_CELL_DEP_TX_HASH,
            constants::CKB_TESTNET_CELL_DEP_INDEX,
        )
    };

    let code_hash: H256 = code_hash_hex
        .trim_start_matches("0x")
        .parse()
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid QR lock code_hash: {}", e)))?;
    let dep_tx_hash: H256 = dep_tx_hash_hex
        .trim_start_matches("0x")
        .parse()
        .map_err(|e| NodeManagerError::RpcError(format!("Invalid QR lock dep tx_hash: {}", e)))?;

    let script_id = ScriptId::new(code_hash.clone(), hash_type);
    let cell_dep = CellDep::new_builder()
        .out_point(
            OutPoint::new_builder()
                .tx_hash(dep_tx_hash.pack())
                .index(dep_index)
                .build(),
        )
        .dep_type(DepType::Code)
        .build();

    resolver.insert(script_id, cell_dep, "Quantum resistant lock".to_string());

    Ok(resolver)
}

/// Computes the minimum capacity (in shannons) for a cell with only a lock
/// script and the 8-byte capacity field — no type script, no output data.
pub(crate) fn minimal_cell_capacity(
    lock_script: &ckb_types::packed::Script,
) -> Result<u64, NodeManagerError> {
    use ckb_types::core::Capacity;
    use ckb_types::packed::CellOutput;

    let output = CellOutput::new_builder().lock(lock_script.clone()).build();
    output
        .occupied_capacity(Capacity::zero())
        .map(|capacity| capacity.as_u64())
        .map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to calculate minimal cell capacity: {}", e))
        })
}
