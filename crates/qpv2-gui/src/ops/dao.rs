//! DAO transaction building, signing, and sending.

use std::sync::mpsc;

use qpv2_core::KeyVault;

use crate::types::{spx_witness_lock_size, DaoQueryEvent, DaoStatus, Status, CKB_DECIMAL_PLACES};
use crate::App;

impl App {
    /// Kick off background queries for deposited + prepared DAO cells across all accounts.
    pub(crate) fn fetch_dao_cells(&mut self) {
        if self.accounts.is_empty() || self.dao_query_rx.is_some() {
            return;
        }
        self.dao_deposited_cells.clear();
        self.dao_prepared_cells.clear();

        let is_mainnet = self.is_mainnet();
        let node_config = self.node_config.clone();
        let all_lock_args: Vec<String> = self.accounts.clone();

        let (tx, rx) = mpsc::channel();
        self.dao_query_rx = Some(rx);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);

            for lock_args in &all_lock_args {
                let address_str = match qpv2_core::utilities::lock_args_to_address(lock_args, is_mainnet) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(Err(format!("Invalid address: {}", e)));
                        continue;
                    }
                };
                let address: ckb_sdk::Address = match address_str.parse() {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(Err(format!("Invalid address: {}", e)));
                        continue;
                    }
                };

                let deposited = match node_manager::query_deposited_cells(rpc.as_ref(), &address) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(Err(format!("Failed to query deposited cells: {}", e)));
                        continue;
                    }
                };

                for cell in deposited {
                    // If the receiver is dropped (e.g. wallet locked), stop.
                    if tx
                        .send(Ok(DaoQueryEvent::Deposited(lock_args.clone(), cell)))
                        .is_err()
                    {
                        return;
                    }
                }

                let prepared = match node_manager::query_prepared_cells(rpc.as_ref(), &address) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(Err(format!("Failed to query prepared cells: {}", e)));
                        continue;
                    }
                };
                for cell in prepared {
                    // If the receiver is dropped (e.g. wallet locked), stop.
                    if tx
                        .send(Ok(DaoQueryEvent::Prepared(lock_args.clone(), cell)))
                        .is_err()
                    {
                        return;
                    }
                }
            }

            let _ = tx.send(Ok(DaoQueryEvent::Done));
        });
    }

    /// Poll the DAO cell query channel.
    pub(crate) fn poll_dao_query(&mut self) {
        let rx = match &self.dao_query_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok(DaoQueryEvent::Deposited(lock_args, cell))) => {
                self.dao_deposited_cells.push((lock_args, cell));
            }
            Ok(Ok(DaoQueryEvent::Prepared(lock_args, cell))) => {
                self.dao_prepared_cells.push((lock_args, cell));
            }
            Ok(Ok(DaoQueryEvent::Done)) => {
                self.dao_query_rx = None;
            }
            Ok(Err(e)) => {
                self.dao_query_rx = None;
                self.status = Status::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.dao_query_rx = None;
            }
        }
    }

    /// Start building a DAO deposit transaction in a background thread.
    pub(crate) fn start_dao_deposit(&mut self) {
        if self.accounts.is_empty() {
            self.dao_status = DaoStatus::Error("No accounts available.".to_string());
            return;
        }

        let from_idx = self.dao_deposit_from_account.min(self.accounts.len() - 1);
        let lock_args = self.accounts[from_idx].clone();

        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let amount_ckb: f64 = match self.dao_deposit_amount.trim().parse() {
            Ok(v) if v > 0.0 => v,
            _ => {
                self.dao_status = DaoStatus::Error("Invalid amount.".to_string());
                return;
            }
        };
        let capacity_sh = (amount_ckb * CKB_DECIMAL_PLACES as f64) as u64;

        let fee_rate: u64 = match self.dao_deposit_fee_rate.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                self.dao_status = DaoStatus::Error("Invalid fee rate.".to_string());
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        self.dao_status = DaoStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.dao_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = node_manager::DaoDepositBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned(&from_address, capacity_sh, fee_rate)
                    .map_err(|e| format!("Failed to build DAO deposit: {}", e))?;

                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// Start building a DAO prepare transaction in a background thread.
    pub(crate) fn start_dao_prepare(
        &mut self,
        deposit_out_point: ckb_types::packed::OutPoint,
        lock_args: String,
    ) {
        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let fee_rate: u64 = self.dao_deposit_fee_rate.trim().parse().unwrap_or(1000);

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        self.dao_status = DaoStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.dao_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = node_manager::DaoPrepareBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned(&from_address, vec![deposit_out_point], fee_rate)
                    .map_err(|e| format!("Failed to build DAO prepare: {}", e))?;

                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// Start building a DAO withdraw transaction in a background thread.
    pub(crate) fn start_dao_withdraw(
        &mut self,
        prepared_out_point: ckb_types::packed::OutPoint,
        lock_args: String,
    ) {
        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let fee_rate: u64 = self.dao_deposit_fee_rate.trim().parse().unwrap_or(1000);

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        self.dao_status = DaoStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.dao_build_rx = Some(rx);

        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let rpc = node_manager::connect(&node_config);
                let from_address: ckb_sdk::Address = from_addr_str
                    .parse()
                    .map_err(|e| format!("Invalid sender address: {}", e))?;

                let unsigned_tx = node_manager::DaoWithdrawBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned(&from_address, vec![prepared_out_point], fee_rate)
                    .map_err(|e| format!("Failed to build DAO withdraw: {}", e))?;

                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// Poll the DAO build channel and trigger Touch ID on success.
    pub(crate) fn poll_dao_build(&mut self, frame: &eframe::Frame) {
        let rx = match &self.dao_build_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok((unsigned_tx, input_cells, lock_args))) => {
                self.dao_build_rx = None;
                #[cfg(target_os = "macos")]
                {
                    let window = match Self::get_ns_window(frame) {
                        Ok(w) => w,
                        Err(e) => {
                            self.dao_status =
                                DaoStatus::Error(format!("Failed to get window: {}", e));
                            return;
                        }
                    };
                    let credential_id = match self.get_credential_id() {
                        Some(id) => id,
                        None => {
                            self.dao_status =
                                DaoStatus::Error("Failed to read credential.".to_string());
                            return;
                        }
                    };

                    let rp_id = "quantumpurse.org";
                    let salt = b"quantumpurse-kv-seed-encryption\0";
                    match passkey_prf::assert_async(&window, rp_id, &credential_id, Some(salt)) {
                        Ok(op) => {
                            use crate::types::PasskeyOp;
                            self.passkey_op = Some(PasskeyOp::SignDaoAssert {
                                op,
                                unsigned_tx,
                                input_cells,
                                lock_args,
                            });
                            self.dao_status = DaoStatus::AwaitingSignature;
                        }
                        Err(passkey_prf::PrfError::Cancelled) => {
                            self.dao_status = DaoStatus::Idle;
                            self.status = Status::Info("DAO operation cancelled.".to_string());
                        }
                        Err(e) => {
                            self.dao_status =
                                DaoStatus::Error(format!("PRF assertion failed: {}", e));
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (frame, unsigned_tx, input_cells, lock_args);
                    self.dao_status =
                        DaoStatus::Error("Signing is only supported on macOS.".to_string());
                }
            }
            Ok(Err(e)) => {
                self.dao_build_rx = None;
                self.dao_status = DaoStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.dao_build_rx = None;
                if matches!(self.dao_status, DaoStatus::Building) {
                    self.dao_status =
                        DaoStatus::Error("Build thread terminated unexpectedly.".to_string());
                }
            }
        }
    }

    /// After Touch ID returns the PRF output for DAO, sign and send.
    pub(crate) fn finish_sign_dao(
        &mut self,
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
                self.dao_status = DaoStatus::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Failed to read variant: {}", e));
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
            self.dao_status = DaoStatus::Error(format!("Failed to compute tx message: {:?}", e));
            return;
        }
        let message = hasher.hash().to_vec();

        let vault = KeyVault::new(variant);
        let signature_bytes = match vault.ckb_sign(AuthKey::CryptoKey(key), lock_args, message) {
            Ok(sig) => sig,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Signing failed: {}", e));
                return;
            }
        };

        let signed_tx = match node_manager::fill_witness(unsigned_tx, 0, signature_bytes) {
            Ok(tx) => tx,
            Err(e) => {
                self.dao_status = DaoStatus::Error(format!("Failed to fill witness: {}", e));
                return;
            }
        };

        self.dao_status = DaoStatus::Sending;
        let node_config = self.node_config.clone();
        let (tx_send, rx_send) = mpsc::channel();
        self.dao_send_rx = Some(rx_send);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);
            let result = node_manager::send_transaction(rpc.as_ref(), &signed_tx)
                .map(|hash| format!("{:#x}", hash))
                .map_err(|e| format!("Failed to send transaction: {}", e));
            let _ = tx_send.send(result);
        });
    }

    /// Poll the DAO send channel for the final result.
    pub(crate) fn poll_dao_send(&mut self) {
        let rx = match &self.dao_send_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok(tx_hash)) => {
                self.dao_send_rx = None;
                let hash = tx_hash.trim_start_matches("0x").to_string();
                self.dao_status = DaoStatus::Success(hash);
                self.dao_deposit_amount.clear();
                // Refresh balances and DAO cells
                self.fetch_all_balances();
                self.fetch_dao_cells();
            }
            Ok(Err(e)) => {
                self.dao_send_rx = None;
                self.dao_status = DaoStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.dao_send_rx = None;
                if matches!(self.dao_status, DaoStatus::Sending) {
                    self.dao_status =
                        DaoStatus::Error("Send thread terminated unexpectedly.".to_string());
                }
            }
        }
    }
}
