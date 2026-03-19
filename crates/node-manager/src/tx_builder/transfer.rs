//! Transfer transaction builder.

use crate::error::NodeManagerError;
use crate::rpc::CkbRpc;
use ckb_sdk::{
    traits::{
        DefaultCellCollector, DefaultHeaderDepResolver, DefaultTransactionDependencyProvider,
    },
    tx_builder::{
        transfer::CapacityTransferBuilder, CapacityBalancer, CapacityProvider, TxBuilder,
    },
    Address,
};
use ckb_types::{
    bytes::Bytes,
    core::{Capacity, FeeRate, TransactionView},
    packed::{CellOutput, Script, WitnessArgs},
    prelude::*,
};

/// Default placeholder lock size for secp256k1 witnesses.
const DEFAULT_PLACEHOLDER_LOCK_SIZE: usize = 65;

/// Builder for transfer transactions.
pub struct TransferBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    /// Whether the target network is mainnet (affects cell dep resolution).
    is_mainnet: bool,
    /// Size of the placeholder lock field in the witness.
    /// Must match the final signed witness lock length for correct fee estimation.
    placeholder_lock_size: usize,
}

impl<'a> TransferBuilder<'a> {
    /// Creates a new transfer builder with default secp256k1 placeholder size.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        TransferBuilder {
            rpc,
            is_mainnet,
            placeholder_lock_size: DEFAULT_PLACEHOLDER_LOCK_SIZE,
        }
    }

    /// Sets the placeholder lock size for the witness.
    ///
    /// For SPHINCS+ transactions, this should be set to
    /// `5 + public_key_length + signature_length` to match the final
    /// signed witness format.
    pub fn with_placeholder_lock_size(mut self, size: usize) -> Self {
        self.placeholder_lock_size = size;
        self
    }

    /// Builds an unsigned transfer transaction.
    ///
    /// # Parameters
    /// - `from_address`: The sender's address
    /// - `to_address`: The recipient's address
    /// - `capacity_sh`: Amount to transfer in shannons (1 CKB = 10^8 shannons)
    /// - `fee_rate`: Fee rate in shannons per 1000 bytes
    /// - `data`: Optional data to include in the output cell
    ///
    /// # Returns
    /// An unsigned transaction ready for signing
    pub fn build_unsigned(
        &self,
        from_address: &Address,
        to_address: &Address,
        capacity_sh: u64,
        fee_rate: u64,
        data: Option<Vec<u8>>,
    ) -> Result<TransactionView, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let from_lock_script = Script::from(from_address.payload());
        let to_lock_script = Script::from(to_address.payload());

        // Create output cell
        let to_output = CellOutput::new_builder()
            .capacity(Capacity::shannons(capacity_sh).pack())
            .lock(to_lock_script)
            .build();

        let output_data = data.unwrap_or_default();

        // Create transfer builder with output
        let transfer_builder =
            CapacityTransferBuilder::new(vec![(to_output, Bytes::from(output_data))]);

        // Setup balance parameters.
        // The placeholder lock size must match the final signed witness lock
        // field length so the fee estimator reserves enough capacity.
        // Default 65 bytes is for secp256k1; SPHINCS+ signatures are much larger.
        let placeholder_witness = WitnessArgs::new_builder()
            .lock(Some(Bytes::from(vec![0u8; self.placeholder_lock_size])).pack())
            .build();

        let balancer = CapacityBalancer {
            fee_rate: FeeRate::from_u64(fee_rate),
            change_lock_script: Some(from_lock_script.clone()),
            capacity_provider: CapacityProvider::new_simple(vec![(
                from_lock_script,
                placeholder_witness,
            )]),
            force_small_change_as_fee: None,
        };

        // Create collectors and resolvers
        let mut cell_collector = DefaultCellCollector::new(&rpc_url);
        let cell_dep_resolver =
            super::utils::cell_dep_resolver_from_rpc(&rpc_url, self.is_mainnet)?;
        let header_dep_resolver = DefaultHeaderDepResolver::new(&rpc_url);
        let tx_dep_provider = DefaultTransactionDependencyProvider::new(&rpc_url, 10);

        // Build the transaction
        let tx = transfer_builder
            .build_balanced(
                &mut cell_collector,
                &cell_dep_resolver,
                &header_dep_resolver,
                &tx_dep_provider,
                &balancer,
                &Default::default(),
            )
            .map_err(|e| {
                NodeManagerError::RpcError(format!("Failed to build transfer: {:?}", e))
            })?;

        Ok(tx)
    }
}
