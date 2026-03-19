//! DAO transaction builders.

use crate::error::NodeManagerError;
use crate::rpc::CkbRpc;
use byteorder::{ByteOrder, LittleEndian};
use ckb_sdk::{
    constants::DAO_TYPE_HASH,
    traits::{
        CellCollector, CellQueryOptions, DefaultCellCollector, DefaultHeaderDepResolver,
        DefaultTransactionDependencyProvider, ValueRangeOption,
    },
    tx_builder::{
        dao::{
            DaoDepositBuilder as SdkDaoDepositBuilder, DaoDepositReceiver,
            DaoPrepareBuilder as SdkDaoPrepareBuilder, DaoPrepareItem,
            DaoWithdrawBuilder as SdkDaoWithdrawBuilder, DaoWithdrawItem, DaoWithdrawReceiver,
        },
        CapacityBalancer, CapacityProvider, TxBuilder,
    },
    Address,
};
use ckb_types::{
    bytes::Bytes,
    core::{FeeRate, ScriptHashType, TransactionView},
    packed::{CellInput, OutPoint, Script, WitnessArgs},
    prelude::*,
};

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
) -> Result<TransactionView, NodeManagerError> {
    let placeholder_witness = WitnessArgs::new_builder()
        .lock(Some(Bytes::from(vec![0u8; 65])).pack())
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
pub struct DaoDepositBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    is_mainnet: bool,
}

impl<'a> DaoDepositBuilder<'a> {
    /// Creates a new DAO deposit builder.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        DaoDepositBuilder { rpc, is_mainnet }
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
    pub fn build_unsigned(
        &self,
        from_address: &Address,
        capacity_sh: u64,
        fee_rate: u64,
    ) -> Result<TransactionView, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(from_address.payload());

        // Create DAO deposit receiver
        let deposit_receiver = DaoDepositReceiver::new(lock_script.clone(), capacity_sh);
        let deposit_builder = SdkDaoDepositBuilder::new(vec![deposit_receiver]);

        build_balanced_dao_tx(&deposit_builder, &lock_script, fee_rate, &rpc_url, self.is_mainnet)
    }
}

/// Builder for DAO prepare (withdraw phase 1) transactions.
pub struct DaoPrepareBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    is_mainnet: bool,
}

impl<'a> DaoPrepareBuilder<'a> {
    /// Creates a new DAO prepare builder.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        DaoPrepareBuilder { rpc, is_mainnet }
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
    pub fn build_unsigned(
        &self,
        from_address: &Address,
        deposit_out_points: Vec<OutPoint>,
        fee_rate: u64,
    ) -> Result<TransactionView, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(from_address.payload());

        // Create prepare items from deposit outpoints
        let items = deposit_out_points
            .into_iter()
            .map(|out_point| DaoPrepareItem::from(CellInput::new(out_point, 0)))
            .collect::<Vec<_>>();

        let prepare_builder = SdkDaoPrepareBuilder::new(items);

        build_balanced_dao_tx(&prepare_builder, &lock_script, fee_rate, &rpc_url, self.is_mainnet)
    }
}

/// Builder for DAO withdraw (phase 2) transactions.
pub struct DaoWithdrawBuilder<'a> {
    rpc: &'a dyn CkbRpc,
    is_mainnet: bool,
}

impl<'a> DaoWithdrawBuilder<'a> {
    /// Creates a new DAO withdraw builder.
    pub fn new(rpc: &'a dyn CkbRpc, is_mainnet: bool) -> Self {
        DaoWithdrawBuilder { rpc, is_mainnet }
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
    pub fn build_unsigned(
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
                .lock(Some(Bytes::from(vec![0u8; 65])).pack())
                .build(),
        );

        // Create withdraw receiver (where the funds go)
        let receiver = DaoWithdrawReceiver::LockScript {
            script: lock_script.clone(),
            fee_rate: Some(FeeRate::from_u64(fee_rate)),
        };

        let withdraw_builder = SdkDaoWithdrawBuilder::new(items, receiver);

        build_balanced_dao_tx(&withdraw_builder, &lock_script, fee_rate, &rpc_url, self.is_mainnet)
    }
}

/// Helper functions for querying DAO cells.
impl<'a> DaoDepositBuilder<'a> {
    /// Query deposited DAO cells for an address.
    pub fn query_deposited_cells(
        &self,
        address: &Address,
    ) -> Result<Vec<DepositedCell>, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(address.payload());
        let mut cell_collector = DefaultCellCollector::new(&rpc_url);

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

        // Filter for deposited cells (block number == 0 in cell data)
        Ok(cells
            .into_iter()
            .filter(|cell| {
                cell.output_data.len() == 8
                    && LittleEndian::read_u64(&cell.output_data.as_ref()[0..8]) == 0
            })
            .map(|cell| DepositedCell {
                out_point: cell.out_point,
                capacity: cell.output.capacity().unpack(),
            })
            .collect())
    }
}

/// Represents a deposited DAO cell.
#[derive(Debug, Clone)]
pub struct DepositedCell {
    pub out_point: OutPoint,
    pub capacity: u64,
}

/// Helper functions for querying prepared cells.
impl<'a> DaoPrepareBuilder<'a> {
    /// Query prepared DAO cells for an address.
    pub fn query_prepared_cells(
        &self,
        address: &Address,
    ) -> Result<Vec<PreparedCell>, NodeManagerError> {
        let rpc_url = self.rpc.get_rpc_url();
        let lock_script = Script::from(address.payload());
        let mut cell_collector = DefaultCellCollector::new(&rpc_url);

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

        // Filter for prepared cells (block number != 0 in cell data)
        let prepared_cells: Vec<_> = cells
            .into_iter()
            .filter(|cell| {
                cell.output_data.len() == 8
                    && LittleEndian::read_u64(&cell.output_data.as_ref()[0..8]) != 0
            })
            .collect();

        // Calculate maximum withdraw for each cell
        let mut result = Vec::new();
        for cell in prepared_cells {
            let max_withdraw = self.calculate_max_withdraw(&cell)?;
            result.push(PreparedCell {
                out_point: cell.out_point,
                capacity: cell.output.capacity().unpack(),
                maximum_withdraw: max_withdraw,
            });
        }

        Ok(result)
    }

    fn calculate_max_withdraw(
        &self,
        cell: &ckb_sdk::traits::LiveCell,
    ) -> Result<u64, NodeManagerError> {
        // TODO: Implement full calculation using get_transaction_with_status
        // to fetch deposit and prepare headers, then compute DAO interest.
        Ok(cell.output.capacity().unpack())
    }
}

/// Represents a prepared DAO cell ready for withdrawal.
#[derive(Debug, Clone)]
pub struct PreparedCell {
    pub out_point: OutPoint,
    pub capacity: u64,
    pub maximum_withdraw: u64,
}
