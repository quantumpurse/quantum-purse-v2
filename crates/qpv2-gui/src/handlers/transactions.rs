//! DAO transaction building, signing, and sending.

use std::sync::mpsc;

use qpv2_core::KeyVault;

use crate::types::{spx_witness_lock_size, TransactionKind, TransactionStatus, CKB_DECIMAL_PLACES};
use crate::App;

impl App {
    /// Kick off a transfer: validate inputs, then build the unsigned tx in a background thread.
    pub(crate) fn transfer_async(&mut self) {
        // Validate inputs
        if self.accounts.is_empty() {
            self.tx_status = TransactionStatus::Error("No accounts available.".to_string());
            return;
        }

        let from_idx = self.transfer_from_account.min(self.accounts.len() - 1);
        let lock_args = self.accounts[from_idx].clone();

        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let to_addr_str = self.transfer_recipient.trim().to_string();
        if to_addr_str.is_empty() {
            self.tx_status = TransactionStatus::Error("Recipient address is empty.".to_string());
            return;
        }

        let fee_rate: u64 = match self.transfer_fee_rate.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                self.tx_status = TransactionStatus::Error("Invalid fee rate.".to_string());
                return;
            }
        };

        // Determine the SPHINCS+ variant to calculate placeholder witness size
        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.tx_status =
                    TransactionStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        let send_all = self.transfer_all;

        // Parse amount only when not sending all.
        let capacity_sh = if send_all {
            0 // Unused; build_unsigned_transfer_all computes the amount internally.
        } else {
            let amount_ckb: f64 = match self.transfer_amount.trim().parse() {
                Ok(v) if v > 0.0 => v,
                _ => {
                    self.tx_status = TransactionStatus::Error("Invalid amount.".to_string());
                    return;
                }
            };
            (amount_ckb * CKB_DECIMAL_PLACES as f64) as u64
        };

        self.tx_status = TransactionStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);

                // Parse addresses
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;
                let to_address: ckb_sdk::Address = to_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid recipient address: {}", e))?;

                let builder = node_manager::QpTransferBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size);

                let unsigned_tx = if send_all {
                    let (tx, _) = builder
                        .build_unsigned_transfer_all(&from_address, &to_address, fee_rate, None)
                        .map_err(|e| format!("Failed to build transaction: {}", e))?;
                    tx
                } else {
                    builder
                        .build_unsigned_transfer(
                            &from_address,
                            &to_address,
                            capacity_sh,
                            fee_rate,
                            None,
                        )
                        .map_err(|e| format!("Failed to build transaction: {}", e))?
                };

                // Fetch input cells for CKB_TX_MESSAGE_ALL
                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((
                    TransactionKind::Transfer,
                    unsigned_tx,
                    input_cells,
                    lock_args,
                ))
            })();

            let _ = tx.send(result);
        });
    }

    /// Start building a DAO deposit transaction in a background thread.
    pub(crate) fn dao_deposit_async(&mut self) {
        if self.accounts.is_empty() {
            self.tx_status = TransactionStatus::Error("No accounts available.".to_string());
            return;
        }

        let from_idx = self.dao_deposit_from_account.min(self.accounts.len() - 1);
        let lock_args = self.accounts[from_idx].clone();

        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let fee_rate: u64 = match self.dao_deposit_fee_rate.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                self.tx_status = TransactionStatus::Error("Invalid fee rate.".to_string());
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.tx_status =
                    TransactionStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        let deposit_all = self.dao_deposit_all;

        // Parse amount only when not depositing all.
        let capacity_sh = if deposit_all {
            0 // Unused; build_unsigned_deposit_all computes the amount internally.
        } else {
            let amount_ckb: f64 = match self.dao_deposit_amount.trim().parse() {
                Ok(v) if v > 0.0 => v,
                _ => {
                    self.tx_status = TransactionStatus::Error("Invalid amount.".to_string());
                    return;
                }
            };
            (amount_ckb * CKB_DECIMAL_PLACES as f64) as u64
        };

        self.tx_status = TransactionStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let builder = node_manager::QpDaoDepositBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size);

                let unsigned_tx = if deposit_all {
                    let (tx, _) = builder
                        .build_unsigned_deposit_all(&from_address, fee_rate)
                        .map_err(|e| format!("Failed to build DAO deposit: {}", e))?;
                    tx
                } else {
                    builder
                        .build_unsigned_deposit(&from_address, capacity_sh, fee_rate)
                        .map_err(|e| format!("Failed to build DAO deposit: {}", e))?
                };

                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((TransactionKind::Dao, unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// Start building a DAO prepare transaction in a background thread.
    pub(crate) fn dao_withdraw_request_async(
        &mut self,
        deposit_out_point: ckb_types::packed::OutPoint,
        lock_args: String,
    ) {
        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let fee_rate: u64 = self.dao_deposit_fee_rate.trim().parse().unwrap_or(1000);

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.tx_status =
                    TransactionStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        self.tx_status = TransactionStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = node_manager::QpDaoPrepareBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned_dao_request_withdraw(
                        &from_address,
                        vec![deposit_out_point],
                        fee_rate,
                    )
                    .map_err(|e| format!("Failed to build DAO prepare: {}", e))?;

                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((TransactionKind::Dao, unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// Start building a DAO withdraw transaction in a background thread.
    pub(crate) fn dao_withdraw_async(
        &mut self,
        prepared_out_point: ckb_types::packed::OutPoint,
        lock_args: String,
    ) {
        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let fee_rate: u64 = self.dao_deposit_fee_rate.trim().parse().unwrap_or(1000);

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.tx_status =
                    TransactionStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        self.tx_status = TransactionStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = node_manager::QpDaoWithdrawBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned_dao_withdraw(&from_address, vec![prepared_out_point], fee_rate)
                    .map_err(|e| format!("Failed to build DAO withdraw: {}", e))?;

                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((TransactionKind::Dao, unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }
}
