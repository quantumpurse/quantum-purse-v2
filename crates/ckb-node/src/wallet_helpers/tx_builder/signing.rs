//! Transaction signing utilities for quantum-resistant (SPHINCS+) locks.
//!
//! Provides helpers to:
//! 1. Fetch input cell data needed for CKB_TX_MESSAGE_ALL.
//! 2. Fill a signed witness into a transaction.
//! 3. Send a signed transaction via RPC.

use crate::client::{QpClient, TransactionStatus};
use crate::error::NodeManagerError;
use ckb_mock_tx_types::MockInput;
use ckb_types::{
    bytes::Bytes,
    core::{HeaderView, TransactionView},
    packed::{CellOutput, WitnessArgs},
    prelude::*,
    H256,
};

/// Chain data a hardware signer needs to verify transaction inputs and DAO
/// header dependencies without trusting host-supplied capacities.
pub struct HardwareSigningContext {
    /// Full transaction for every input's previous-output transaction hash.
    pub prev_txs: std::collections::HashMap<H256, TransactionView>,
    /// Block that committed each previous transaction, when available.
    pub prev_tx_block_hashes: std::collections::HashMap<H256, H256>,
    /// Full headers in exactly the same order as the transaction's header deps.
    pub headers: Vec<HeaderView>,
}

/// TODO optimize
/// `QpClient::get_transaction` with a short retry backoff. A lost RPC
/// request in the signing path aborts the whole flow (the user has to
/// rebuild and reconfirm the transaction), and public RPC endpoints drop
/// requests now and then — transient transport errors are worth absorbing.
fn get_transaction_with_retry(
    qp_client: &QpClient,
    tx_hash: &H256,
) -> Result<Option<TransactionStatus>, NodeManagerError> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match qp_client.get_transaction(tx_hash.clone()) {
            Ok(status) => return Ok(status),
            Err(_) if attempt < 4 => {
                std::thread::sleep(std::time::Duration::from_millis(500 * u64::from(attempt)))
            }
            Err(e) => return Err(e),
        }
    }
}

/// Fetches the cell every input in a transaction spends (CellOutput + data),
/// each paired with the input that spends it, in input order.
///
/// This data is required by `generate_ckb_tx_message_all` to compute the
/// transaction message hash for quantum-resistant signing.
pub fn fetch_input_cells(
    qp_client: &QpClient,
    tx: &TransactionView,
) -> Result<Vec<MockInput>, NodeManagerError> {
    let mut inputs = Vec::new();

    for input in tx.inputs().into_iter() {
        let out_point = input.previous_output();
        let tx_hash: H256 = out_point.tx_hash().unpack();
        let index: u32 = out_point.index().unpack();

        let tx_status = get_transaction_with_retry(qp_client, &tx_hash)?.ok_or_else(|| {
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

        inputs.push(MockInput {
            input,
            output: cell_output,
            data: cell_data,
            header: None,
        });
    }

    Ok(inputs)
}

/// Fetches the full previous transaction for every input, keyed by tx hash.
///
/// A hardware signer (Trezor) verifies input capacities trustlessly by
/// re-serializing each referenced previous transaction and matching its
/// recomputed hash against the spending input's `OutPoint`. Unlike
/// [`fetch_input_cells`], which keeps only the single spent `CellOutput`, this
/// returns the entire previous transaction (version, inputs, outputs, data,
/// cell deps, header deps) that the device streams and re-hashes. Each distinct
/// referenced tx hash is fetched once.
pub fn fetch_prev_txs(
    qp_client: &QpClient,
    tx: &TransactionView,
) -> Result<std::collections::HashMap<H256, TransactionView>, NodeManagerError> {
    let mut prev_txs: std::collections::HashMap<H256, TransactionView> =
        std::collections::HashMap::new();

    for input in tx.inputs().into_iter() {
        let tx_hash: H256 = input.previous_output().tx_hash().unpack();
        if prev_txs.contains_key(&tx_hash) {
            continue;
        }

        let tx_status = get_transaction_with_retry(qp_client, &tx_hash)?.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Input transaction {} not found.", tx_hash))
        })?;
        let prev_tx_view = tx_status.transaction.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Input transaction {} has no data.", tx_hash))
        })?;

        // jsonrpc Transaction -> packed -> core view, so the device can read
        // every RawTransaction field and recompute the hash.
        let packed: ckb_types::packed::Transaction = prev_tx_view.inner.into();
        prev_txs.insert(tx_hash, packed.into_view());
    }

    Ok(prev_txs)
}

/// Fetch all chain data needed by the Trezor streaming protocol.
///
/// Besides the full previous transactions, DAO phase-2 signing needs the block
/// that committed each spent phase-1 transaction and every full header named by
/// the transaction's `header_deps`.
pub fn fetch_hardware_signing_context(
    qp_client: &QpClient,
    tx: &TransactionView,
) -> Result<HardwareSigningContext, NodeManagerError> {
    let mut prev_txs = std::collections::HashMap::new();
    let mut prev_tx_block_hashes = std::collections::HashMap::new();

    for input in tx.inputs().into_iter() {
        let tx_hash: H256 = input.previous_output().tx_hash().unpack();
        if prev_txs.contains_key(&tx_hash) {
            continue;
        }

        let tx_status = get_transaction_with_retry(qp_client, &tx_hash)?.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Input transaction {} not found.", tx_hash))
        })?;
        let block_hash = tx_status.block_hash.clone();
        let prev_tx_view = tx_status.transaction.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Input transaction {} has no data.", tx_hash))
        })?;
        let packed: ckb_types::packed::Transaction = prev_tx_view.inner.into();
        prev_txs.insert(tx_hash.clone(), packed.into_view());
        if let Some(block_hash) = block_hash {
            prev_tx_block_hashes.insert(tx_hash, block_hash);
        }
    }

    let headers = fetch_header_deps(qp_client, tx)?;

    Ok(HardwareSigningContext {
        prev_txs,
        prev_tx_block_hashes,
        headers,
    })
}

/// Fetches the full header of every header dependency in a transaction, in
/// the order the transaction lists them. A DAO withdrawal's interest comes
/// from two of them: a multisig signing request carries them for its
/// co-signers, and a Trezor checks them when signing.
pub fn fetch_header_deps(
    qp_client: &QpClient,
    tx: &TransactionView,
) -> Result<Vec<HeaderView>, NodeManagerError> {
    let mut headers = Vec::with_capacity(tx.header_deps().len());
    for packed_hash in tx.header_deps().into_iter() {
        let hash: H256 = packed_hash.unpack();
        let json_header = qp_client.get_header(hash.clone())?.ok_or_else(|| {
            NodeManagerError::RpcError(format!("Header dependency {} not found.", hash))
        })?;
        let header: HeaderView = json_header.into();
        let actual: H256 = header.hash().unpack();
        if actual != hash {
            return Err(NodeManagerError::RpcError(format!(
                "Header dependency hash mismatch: requested {}, received {}.",
                hash, actual
            )));
        }
        headers.push(header);
    }
    Ok(headers)
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
