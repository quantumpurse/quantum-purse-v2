//! DAO transaction builders.

use crate::error::NodeManagerError;
use crate::rpc::CkbRpc;
use byteorder::{ByteOrder, LittleEndian};
use ckb_sdk::{
    constants::DAO_TYPE_HASH,
    traits::{
        CellCollector, CellDepResolver, CellQueryOptions, DefaultCellCollector,
        DefaultHeaderDepResolver, DefaultTransactionDependencyProvider, ValueRangeOption,
    },
    tx_builder::{
        balance_tx_capacity,
        dao::{
            DaoDepositBuilder, DaoDepositReceiver, DaoPrepareBuilder, DaoPrepareItem,
            DaoWithdrawBuilder, DaoWithdrawItem, DaoWithdrawReceiver,
        },
        CapacityBalancer, CapacityProvider, TxBuilder,
    },
    Address,
};
use ckb_types::{
    bytes::Bytes,
    core::{Capacity, FeeRate, ScriptHashType, TransactionBuilder, TransactionView},
    packed::{CellInput, CellOutput, OutPoint, Script, WitnessArgs},
    prelude::*,
    H256,
};

/// Default placeholder lock size for secp256k1 witnesses.
const DEFAULT_PLACEHOLDER_LOCK_SIZE: usize = 65;

/// Builds a balanced DAO transaction from an SDK builder.
///
/// Shared by deposit, prepare, and withdraw builders since the balancing
/// and resolution logic is identical.
fn build_balanced_dao_tx(
    builder: &dyn TxBuilder,
    lock_script: &Script,
    fee_rate: u64,
    rpc_url: &str,
    is_mainnet: bool,
    placeholder_lock_size: usize,
) -> Result<TransactionView, NodeManagerError> {
    let placeholder_witness = WitnessArgs::new_builder()
        .lock(Some(Bytes::from(vec![0u8; placeholder_lock_size])).pack())
        .build();

    let balancer = CapacityBalancer {
        fee_rate: FeeRate::from_u64(fee_rate),
        change_lock_script: Some(lock_script.clone()),
        capacity_provider: CapacityProvider::new_simple(vec![(
            lock_script.clone(),
            placeholder_witness,
        )]),
        force_small_change_as_fee: None,
    };

    let mut cell_collector = DefaultCellCollector::new(rpc_url);
    let cell_dep_resolver = super::utils::cell_dep_resolver_from_rpc(rpc_url, is_mainnet)?;
    let header_dep_resolver = DefaultHeaderDepResolver::new(rpc_url);
    let tx_dep_provider = DefaultTransactionDependencyProvider::new(rpc_url, 10);

    let tx = builder
        .build_balanced(
            &mut cell_collector,
            &cell_dep_resolver,
            &header_dep_resolver,
            &tx_dep_provider,
            &balancer,
            &Default::default(),
        )
        .map_err(|e| NodeManagerError::RpcError(format!("Failed to build DAO tx: {:?}", e)))?;

    Ok(tx)
}

/// Builder for DAO deposit transactions.
pub struct QpDaoDepositBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    is_mainnet: bool,
    placeholder_lock_size: usize,
}

impl<'a> QpDaoDepositBuilder<'a> {
    /// Creates a new DAO deposit builder with default secp256k1 placeholder size.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        QpDaoDepositBuilder {
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

    /// Builds an unsigned DAO deposit transaction.
    ///
    /// # Parameters
    /// - `from_address`: The address providing the capacity
    /// - `capacity_sh`: Amount to deposit in shannons
    /// - `fee_rate`: Fee rate in shannons per 1000 bytes
    ///
    /// # Returns
    /// An unsigned transaction ready for signing
    pub fn build_unsigned_deposit(
        &self,
        from_address: &Address,
        capacity_sh: u64,
        fee_rate: u64,
    ) -> Result<TransactionView, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(from_address.payload());

        // Create DAO deposit receiver
        let deposit_receiver = DaoDepositReceiver::new(lock_script.clone(), capacity_sh);
        let deposit_builder = DaoDepositBuilder::new(vec![deposit_receiver]);

        build_balanced_dao_tx(
            &deposit_builder,
            &lock_script,
            fee_rate,
            &rpc_url,
            self.is_mainnet,
            self.placeholder_lock_size,
        )
    }

    /// Builds an unsigned DAO deposit transaction that deposits all spendable
    /// capacity, leaving no change cell. Fee is deducted from the deposit amount.
    ///
    /// Returns the built transaction together with the final deposit amount in shannons.
    pub fn build_unsigned_deposit_all(
        &self,
        from_address: &Address,
        fee_rate: u64,
    ) -> Result<(TransactionView, u64), NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(from_address.payload());

        let spendable_cells = super::utils::collect_spendable_cells(&rpc_url, &lock_script)?;

        let total_input_capacity: u64 = spendable_cells
            .iter()
            .map(|cell| {
                let capacity: u64 = cell.output.capacity().unpack();
                capacity
            })
            .sum();

        let cell_dep_resolver =
            super::utils::cell_dep_resolver_from_rpc(&rpc_url, self.is_mainnet)?;
        let sender_lock_dep = cell_dep_resolver.resolve(&lock_script).ok_or_else(|| {
            NodeManagerError::RpcError("Failed to resolve sender lock cell dep.".to_string())
        })?;

        // Resolve the DAO type script cell dep.
        let dao_type_script = Script::new_builder()
            .code_hash(DAO_TYPE_HASH.pack())
            .hash_type(ScriptHashType::Type)
            .build();
        let dao_cell_dep = cell_dep_resolver.resolve(&dao_type_script).ok_or_else(|| {
            NodeManagerError::RpcError("Failed to resolve DAO type script cell dep.".to_string())
        })?;

        let placeholder_witness = WitnessArgs::new_builder()
            .lock(Some(Bytes::from(vec![0u8; self.placeholder_lock_size])).pack())
            .build();

        let inputs: Vec<CellInput> = spendable_cells
            .iter()
            .map(|cell| CellInput::new(cell.out_point.clone(), 0))
            .collect();

        let witnesses: Vec<_> = std::iter::once(placeholder_witness.as_bytes().pack())
            .chain(
                std::iter::repeat_with(|| Bytes::new().pack()).take(inputs.len().saturating_sub(1)),
            )
            .collect();

        // DAO deposit output: lock script + DAO type script + 8 bytes of zero data.
        let dao_output = CellOutput::new_builder()
            .capacity(Capacity::shannons(total_input_capacity).pack())
            .lock(lock_script.clone())
            .type_(Some(dao_type_script.clone()).pack())
            .build();
        let dao_data = Bytes::from(vec![0u8; 8]);

        // Build provisional transaction to calculate exact fee.
        let provisional_tx = TransactionBuilder::default()
            .set_cell_deps(vec![sender_lock_dep.clone(), dao_cell_dep.clone()])
            .set_inputs(inputs.clone())
            .set_outputs(vec![dao_output])
            .set_outputs_data(vec![dao_data.clone().pack()])
            .set_witnesses(witnesses)
            .build();

        let tx_size = provisional_tx.data().as_reader().serialized_size_in_block() as u64;
        let required_fee = fee_rate.saturating_mul(tx_size).div_ceil(1000);
        let deposit_capacity = total_input_capacity
            .checked_sub(required_fee)
            .ok_or_else(|| {
                NodeManagerError::RpcError(
                    "Insufficient balance to pay transaction fee.".to_string(),
                )
            })?;

        let final_output = CellOutput::new_builder()
            .capacity(Capacity::shannons(deposit_capacity).pack())
            .lock(lock_script)
            .type_(Some(dao_type_script).pack())
            .build();

        // Validate the output cell has enough capacity (lock + type + 8 bytes data).
        let data_capacity = Capacity::bytes(dao_data.len()).map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to calculate output data capacity: {}", e))
        })?;
        if final_output
            .is_lack_of_capacity(data_capacity)
            .map_err(|e| {
                NodeManagerError::RpcError(format!(
                    "Failed to validate final output capacity: {}",
                    e
                ))
            })?
        {
            return Err(NodeManagerError::RpcError(
                "Insufficient balance to create a valid DAO deposit after fee deduction."
                    .to_string(),
            ));
        }

        let tx = TransactionBuilder::default()
            .set_cell_deps(vec![sender_lock_dep, dao_cell_dep])
            .set_inputs(inputs)
            .set_outputs(vec![final_output])
            .set_outputs_data(vec![dao_data.pack()])
            .set_witnesses(
                std::iter::once(placeholder_witness.as_bytes().pack())
                    .chain(
                        std::iter::repeat_with(|| Bytes::new().pack())
                            .take(spendable_cells.len().saturating_sub(1)),
                    )
                    .collect(),
            )
            .build();

        Ok((tx, deposit_capacity))
    }
}

/// Builder for DAO prepare (withdraw phase 1) transactions.
pub struct QpDaoPrepareBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    is_mainnet: bool,
    placeholder_lock_size: usize,
}

impl<'a> QpDaoPrepareBuilder<'a> {
    /// Creates a new DAO prepare builder with default secp256k1 placeholder size.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        QpDaoPrepareBuilder {
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

    /// Builds an unsigned DAO prepare transaction.
    ///
    /// # Parameters
    /// - `from_address`: The address that owns the deposited cells
    /// - `deposit_out_points`: OutPoints of the deposited cells to prepare for withdrawal
    /// - `fee_rate`: Fee rate in shannons per 1000 bytes
    ///
    /// # Returns
    /// An unsigned transaction ready for signing
    pub fn build_unsigned_dao_request_withdraw(
        &self,
        from_address: &Address,
        deposit_out_points: Vec<OutPoint>,
        fee_rate: u64,
    ) -> Result<TransactionView, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(from_address.payload());

        // Create prepare items from deposit outpoints.
        let items = deposit_out_points
            .into_iter()
            .map(|out_point| DaoPrepareItem::from(CellInput::new(out_point, 0)))
            .collect::<Vec<_>>();

        let prepare_builder = DaoPrepareBuilder::new(items);

        // The SDK's DaoPrepareBuilder.build_base produces a transaction with
        // the deposited DAO cell as input 0 (using the user's lock script) but
        // sets no witnesses. The CapacityBalancer then detects the lock script
        // is already present in the inputs and skips placing a WitnessArgs
        // placeholder, leaving witness 0 empty. This causes two problems:
        //   1. generate_ckb_tx_message_all fails to parse witness 0 as WitnessArgs.
        //   2. Fee calculation doesn't account for the large SPHINCS+ lock field.
        //
        // Fix: build the base transaction, inject a WitnessArgs placeholder at
        // witness 0, then run the balancer on the patched transaction so fee
        // calculation includes the full witness size.
        let mut cell_collector = DefaultCellCollector::new(&rpc_url);
        let cell_dep_resolver =
            super::utils::cell_dep_resolver_from_rpc(&rpc_url, self.is_mainnet)?;
        let header_dep_resolver = DefaultHeaderDepResolver::new(&rpc_url);
        let tx_dep_provider = DefaultTransactionDependencyProvider::new(&rpc_url, 10);

        let base_tx = prepare_builder
            .build_base(
                &mut cell_collector,
                &cell_dep_resolver,
                &header_dep_resolver,
                &tx_dep_provider,
            )
            .map_err(|e| {
                NodeManagerError::RpcError(format!("Failed to build DAO prepare base: {:?}", e))
            })?;

        // Inject WitnessArgs with lock placeholder at witness 0.
        let placeholder_witness = WitnessArgs::new_builder()
            .lock(Some(Bytes::from(vec![0u8; self.placeholder_lock_size])).pack())
            .build();
        let mut witnesses: Vec<_> = base_tx.witnesses().into_iter().collect();
        // Pad witnesses to match input count (build_base sets none).
        while witnesses.len() < base_tx.inputs().len() {
            witnesses.push(Default::default());
        }
        witnesses[0] = placeholder_witness.as_bytes().pack();
        let patched_tx = base_tx
            .as_advanced_builder()
            .set_witnesses(witnesses)
            .build();

        // Balance the patched transaction (fee now accounts for full witness).
        let capacity_placeholder = WitnessArgs::new_builder()
            .lock(Some(Bytes::from(vec![0u8; self.placeholder_lock_size])).pack())
            .build();
        let balancer = CapacityBalancer {
            fee_rate: FeeRate::from_u64(fee_rate),
            change_lock_script: Some(lock_script.clone()),
            capacity_provider: CapacityProvider::new_simple(vec![(
                lock_script.clone(),
                capacity_placeholder,
            )]),
            force_small_change_as_fee: None,
        };

        let tx = balance_tx_capacity(
            &patched_tx,
            &balancer,
            &mut cell_collector,
            &tx_dep_provider,
            &cell_dep_resolver,
            &header_dep_resolver,
        )
        .map_err(|e| {
            NodeManagerError::RpcError(format!("Failed to balance DAO prepare: {:?}", e))
        })?;

        Ok(tx)
    }
}

/// Builder for DAO withdraw (phase 2) transactions.
pub struct QpDaoWithdrawBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    is_mainnet: bool,
    placeholder_lock_size: usize,
}

impl<'a> QpDaoWithdrawBuilder<'a> {
    /// Creates a new DAO withdraw builder with default secp256k1 placeholder size.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        QpDaoWithdrawBuilder {
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

    /// Builds an unsigned DAO withdraw transaction.
    ///
    /// # Parameters
    /// - `from_address`: The address that owns the prepared cells
    /// - `prepared_out_points`: OutPoints of the prepared cells to withdraw
    /// - `fee_rate`: Fee rate in shannons per 1000 bytes
    ///
    /// # Returns
    /// An unsigned transaction ready for signing
    pub fn build_unsigned_dao_withdraw(
        &self,
        from_address: &Address,
        prepared_out_points: Vec<OutPoint>,
        fee_rate: u64,
    ) -> Result<TransactionView, NodeManagerError> {
        if prepared_out_points.is_empty() {
            return Err(NodeManagerError::RpcError(
                "No cells to withdraw.".to_string(),
            ));
        }

        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(from_address.payload());

        // Create withdraw items from prepared outpoints
        let mut items = prepared_out_points
            .into_iter()
            .map(|out_point| DaoWithdrawItem::new(out_point, None))
            .collect::<Vec<_>>();

        // Set witness for first input
        items[0].init_witness = Some(
            WitnessArgs::new_builder()
                .lock(Some(Bytes::from(vec![0u8; self.placeholder_lock_size])).pack())
                .build(),
        );

        // Create withdraw receiver (where the funds go)
        let receiver = DaoWithdrawReceiver::LockScript {
            script: lock_script.clone(),
            fee_rate: Some(FeeRate::from_u64(fee_rate)),
        };

        let withdraw_builder = DaoWithdrawBuilder::new(items, receiver);

        build_balanced_dao_tx(
            &withdraw_builder,
            &lock_script,
            fee_rate,
            &rpc_url,
            self.is_mainnet,
            self.placeholder_lock_size,
        )
    }
}

// ── DAO Cell Query Functions ──────────────────────────────────────────

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

/// Collects all DAO cells for a given address from the indexer.
///
/// Returns the raw `LiveCell` list so callers can partition into
/// deposited vs prepared.
fn collect_dao_cells(
    rpc_url: &str,
    address: &Address,
) -> Result<Vec<ckb_sdk::traits::LiveCell>, NodeManagerError> {
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

/// Queries all DAO cells for an address and partitions them into deposited and
/// prepared cells.
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

        // the DAO cell data can tell if this is a deposit cell or a withdrawn cell - waiting to be unlocked.
        // if the first 8 bytes of the data is all 0, the it is a deposit cell. otherwise, it is a prepared cell.
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
    cell: &ckb_sdk::traits::LiveCell,
) -> Result<(u64, u64, u64), NodeManagerError> {
    use ckb_types::core::{Capacity, HeaderView};

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

    let max_withdraw = super::utils::calculate_dao_maximum_withdraw(
        &deposit_header,
        &prepare_header,
        &output,
        occupied_capacity.as_u64(),
    );

    let deposit_block_number: u64 = deposit_header.number();
    let prepare_block_number: u64 = prepare_header.number();

    Ok((max_withdraw, deposit_block_number, prepare_block_number))
}
