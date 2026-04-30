//! Transaction signing utilities for quantum-resistant (SPHINCS+) locks.
//!
//! Provides helpers to:
//! 1. Fetch input cell data needed for CKB_TX_MESSAGE_ALL.
//! 2. Fill a signed witness into a transaction.
//! 3. Send a signed transaction via RPC.

use crate::client::QpClient;
use crate::error::NodeManagerError;
use ckb_types::{
    bytes::Bytes,
    core::TransactionView,
    packed::{CellOutput, WitnessArgs},
    prelude::*,
    H256,
};

/// Fetches the input cells (CellOutput + data) for every input in a transaction.
///
/// This data is required by `generate_ckb_tx_message_all` to compute the
/// transaction message hash for quantum-resistant signing.
pub fn fetch_input_cells(
    qp_client: &QpClient,
    tx: &TransactionView,
) -> Result<Vec<(CellOutput, Bytes)>, NodeManagerError> {
    let mut inputs = Vec::new();

    for input in tx.inputs().into_iter() {
        let out_point = input.previous_output();
        let tx_hash: H256 = out_point.tx_hash().unpack();
        let index: u32 = out_point.index().unpack();

        let tx_status = qp_client
            .get_transaction(tx_hash.clone())?
            .ok_or_else(|| {
                NodeManagerError::RpcError(format!("Input transaction {} not found.", tx_hash))
            })?;

        let prev_tx_view = tx_status.transaction.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Input transaction {} has no data.", tx_hash))
        })?;

        let output = prev_tx_view
            .inner
            .outputs
            .get(index as usize)
            .ok_or_else(|| {
                NodeManagerError::RpcError(format!(
                    "Output index {} not found in transaction {}.",
                    index, tx_hash
                ))
            })?;

        let data = prev_tx_view
            .inner
            .outputs_data
            .get(index as usize)
            .ok_or_else(|| {
                NodeManagerError::RpcError(format!(
                    "Output data index {} not found in transaction {}.",
                    index, tx_hash
                ))
            })?;

        // Convert from jsonrpc types to packed types
        let cell_output: CellOutput = output.clone().into();
        let cell_data: Bytes = data.clone().into_bytes();

        inputs.push((cell_output, cell_data));
    }

    Ok(inputs)
}

/// Replaces the placeholder witness at the given index with the signed witness data.
///
/// The `signature_bytes` should be the complete lock field content as returned
/// by `KeyVault::ckb_sign()`, which includes the prefix, public key, and signature.
pub fn fill_witness(
    tx: TransactionView,
    script_group_index: usize,
    signature_bytes: Vec<u8>,
) -> Result<TransactionView, NodeManagerError> {
    let mut witnesses: Vec<_> = tx.witnesses().into_iter().collect();

    let original_witness = witnesses.get(script_group_index).ok_or_else(|| {
        NodeManagerError::RpcError(format!(
            "Witness index {} out of range.",
            script_group_index
        ))
    })?;

    // Parse the existing WitnessArgs (which has a placeholder lock)
    let witness_args = if original_witness.raw_data().is_empty() {
        WitnessArgs::default()
    } else {
        WitnessArgs::from_slice(original_witness.raw_data().as_ref())
            .map_err(|e| NodeManagerError::RpcError(format!("Invalid witness: {}", e)))?
    };

    // Replace the lock field with the actual signature
    let updated_witness = witness_args
        .as_builder()
        .lock(Some(Bytes::from(signature_bytes)).pack())
        .build();

    witnesses[script_group_index] = updated_witness.as_bytes().pack();

    Ok(tx.as_advanced_builder().set_witnesses(witnesses).build())
}

/// Sends a signed transaction via RPC.
///
/// Returns the transaction hash on success.
pub fn send_transaction(
    qp_client: &QpClient,
    tx: &TransactionView,
) -> Result<H256, NodeManagerError> {
    let json_tx: ckb_jsonrpc_types::Transaction = tx.data().into();
    qp_client.send_transaction(json_tx)
}
