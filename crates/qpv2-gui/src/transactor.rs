//! Transaction building, signing, and sending.

use crate::types::{spx_witness_lock_size, TransactionKind, TransactionStatus, CKB_DECIMAL_PLACES};
use crate::App;
use ckb_node::{NodeType, QpClient};
use qpv2_core::KeyVault;
use std::sync::mpsc;

/// Pre-flight check before building any tx that uses the QR-lock-script
/// cell dep. The light client only stores cells whose lock matches a
/// registered filter script, so the dep cell isn't auto-indexed —
/// `rpc::fetch_qr_lock_dep` is what pulls it. This wrapper gates the
/// build path on whether the dep has finished fetching.
///
/// `Ok(())` for non-LightClient backends (full nodes / public RPC have
/// every cell); `Ok(())` for LightClient when the dep is already in the
/// store. Otherwise returns a user-facing message ready for `App.status`.
fn check_qr_lock_dep_ready(qp_client: &QpClient, node_type: NodeType) -> Result<(), String> {
    if node_type != NodeType::LightClient {
        return Ok(());
    }
    match ckb_node::wallet_helpers::lc::fetch_qr_lock_dep(qp_client) {
        Ok(true) => Ok(()),
        Ok(false) => Err(
            "Light client hasn't fetched the lock-script cell dep yet. Try again in a moment."
                .to_string(),
        ),
        Err(e) => Err(format!("Failed to check lock-script cell dep: {}", e)),
    }
}

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

        let is_mainnet = self.qp_client.is_mainnet();
        let from_addr_str = match crate::ckb::lock_args_to_address(&lock_args, is_mainnet) {
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
        let qp_client = self.qp_client.clone();
        let node_type = self.qp_client.config().node_type;
        let is_mainnet = self.qp_client.is_mainnet();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                check_qr_lock_dep_ready(&qp_client, node_type)?;

                // Parse addresses
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;
                let to_address: ckb_sdk::Address = to_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid recipient address: {}", e))?;

                let builder = ckb_node::QpTransferBuilder::new(&qp_client, is_mainnet)
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
                let input_cells = ckb_node::wallet_helpers::tx_builder::fetch_input_cells(
                    &qp_client,
                    &unsigned_tx,
                )
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

        let is_mainnet = self.qp_client.is_mainnet();
        let from_addr_str = match crate::ckb::lock_args_to_address(&lock_args, is_mainnet) {
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
        let qp_client = self.qp_client.clone();
        let node_type = self.qp_client.config().node_type;
        let is_mainnet = self.qp_client.is_mainnet();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                check_qr_lock_dep_ready(&qp_client, node_type)?;

                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let builder = ckb_node::QpDaoDepositBuilder::new(&qp_client, is_mainnet)
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

                let input_cells = ckb_node::wallet_helpers::tx_builder::fetch_input_cells(
                    &qp_client,
                    &unsigned_tx,
                )
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
        let is_mainnet = self.qp_client.is_mainnet();
        let from_addr_str = match crate::ckb::lock_args_to_address(&lock_args, is_mainnet) {
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
        let qp_client = self.qp_client.clone();
        let node_type = self.qp_client.config().node_type;
        let is_mainnet = self.qp_client.is_mainnet();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                check_qr_lock_dep_ready(&qp_client, node_type)?;

                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = ckb_node::QpDaoPrepareBuilder::new(&qp_client, is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned_dao_request_withdraw(
                        &from_address,
                        vec![deposit_out_point],
                        fee_rate,
                    )
                    .map_err(|e| format!("Failed to build DAO prepare: {}", e))?;

                let input_cells = ckb_node::wallet_helpers::tx_builder::fetch_input_cells(
                    &qp_client,
                    &unsigned_tx,
                )
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
        let is_mainnet = self.qp_client.is_mainnet();
        let from_addr_str = match crate::ckb::lock_args_to_address(&lock_args, is_mainnet) {
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
        let qp_client = self.qp_client.clone();
        let node_type = self.qp_client.config().node_type;
        let is_mainnet = self.qp_client.is_mainnet();

        let (tx, rx) = mpsc::channel();
        self.transaction_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                check_qr_lock_dep_ready(&qp_client, node_type)?;

                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = ckb_node::QpDaoWithdrawBuilder::new(&qp_client, is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned_dao_withdraw(&from_address, vec![prepared_out_point], fee_rate)
                    .map_err(|e| format!("Failed to build DAO withdraw: {}", e))?;

                let input_cells = ckb_node::wallet_helpers::tx_builder::fetch_input_cells(
                    &qp_client,
                    &unsigned_tx,
                )
                .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((TransactionKind::Dao, unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// After Touch ID returns the PRF output, sign and send the transaction.
    pub(crate) fn sign_and_send(
        &mut self,
        kind: TransactionKind,
        prf_output: &qpv2_core::SecureVec,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    ) {
        use ckb_types::prelude::*;
        use qpv2_core::types::AuthKey;

        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Failed to read variant: {}", e));
                return;
            }
        };

        let packed_tx = unsigned_tx.data();
        let mut hasher = ckb_fips205_utils::Hasher::message_hasher();

        let gen_inputs: Vec<(
            ckb_gen_types::packed::CellOutput,
            ckb_gen_types::bytes::Bytes,
        )> = input_cells
            .iter()
            .map(|(output, data)| {
                let raw = output.as_slice();
                let gen_output =
                    ckb_gen_types::packed::CellOutput::from_slice(raw).expect("valid CellOutput");
                (
                    gen_output,
                    ckb_gen_types::bytes::Bytes::copy_from_slice(data),
                )
            })
            .collect();

        let gen_tx = ckb_gen_types::packed::Transaction::from_slice(packed_tx.as_slice())
            .expect("valid Transaction");

        if let Err(e) =
            ckb_fips205_utils::ckb_tx_message_all_from_mock_tx::generate_ckb_tx_message_all(
                &gen_tx,
                &gen_inputs,
                ckb_fips205_utils::ckb_tx_message_all_from_mock_tx::ScriptOrIndex::Index(0),
                &mut hasher,
            )
        {
            self.tx_status =
                TransactionStatus::Error(format!("Failed to compute tx message: {:?}", e));
            return;
        }
        let message = hasher.hash().to_vec();

        let vault = KeyVault::new(variant);
        let signature_bytes = match vault.ckb_sign(AuthKey::CryptoKey(key), lock_args, message) {
            Ok(sig) => sig,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Signing failed: {}", e));
                return;
            }
        };

        let signed_tx = match ckb_node::fill_witness(unsigned_tx, 0, signature_bytes) {
            Ok(tx) => tx,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Failed to fill witness: {}", e));
                return;
            }
        };

        self.tx_status = TransactionStatus::Sending;
        let qp_client = self.qp_client.clone();
        let (tx_send, rx_send) = mpsc::channel();
        self.transaction_send_rx = Some(rx_send);

        // Spawn a thread to handle transaction submission.
        std::thread::spawn(move || {
            let result =
                ckb_node::wallet_helpers::tx_builder::send_transaction(&qp_client, &signed_tx)
                    .map(|hash| format!("{:#x}", hash))
                    .map_err(|e| format!("Failed to send transaction: {}", e));
            let _ = tx_send.send((kind, result));
        });
    }
}
