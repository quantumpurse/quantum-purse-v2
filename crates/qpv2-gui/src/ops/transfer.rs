//! Transfer transaction building, signing, and sending.

use std::sync::mpsc;

use qpv2_core::KeyVault;

use crate::types::{spx_witness_lock_size, Status, TransferStatus, CKB_DECIMAL_PLACES};
use crate::App;

impl App {
    /// Kick off a transfer: validate inputs, then build the unsigned tx in a background thread.
    pub(crate) fn start_transfer(&mut self) {
        // Validate inputs
        if self.accounts.is_empty() {
            self.transfer_status = TransferStatus::Error("No accounts available.".to_string());
            return;
        }

        let from_idx = self.transfer_from_account.min(self.accounts.len() - 1);
        let lock_args = self.accounts[from_idx].clone();

        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet)
        {
            Ok(a) => a,
            Err(e) => {
                self.transfer_status =
                    TransferStatus::Error(format!("Invalid sender address: {}", e));
                return;
            }
        };

        let to_addr_str = self.transfer_recipient.trim().to_string();
        if to_addr_str.is_empty() {
            self.transfer_status = TransferStatus::Error("Recipient address is empty.".to_string());
            return;
        }

        // Parse amount (CKB with decimals -> shannons)
        let amount_ckb: f64 = match self.transfer_amount.trim().parse() {
            Ok(v) if v > 0.0 => v,
            _ => {
                self.transfer_status = TransferStatus::Error("Invalid amount.".to_string());
                return;
            }
        };
        let capacity_sh = (amount_ckb * CKB_DECIMAL_PLACES as f64) as u64;

        let fee_rate: u64 = match self.transfer_fee_rate.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                self.transfer_status = TransferStatus::Error("Invalid fee rate.".to_string());
                return;
            }
        };

        // Determine the SPHINCS+ variant to calculate placeholder witness size
        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.transfer_status =
                    TransferStatus::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };
        let witness_lock_size = spx_witness_lock_size(variant);

        self.transfer_status = TransferStatus::Building;
        let node_config = self.node_config.clone();

        let (tx, rx) = mpsc::channel();
        self.transfer_build_rx = Some(rx);

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

                // Build unsigned transaction with correct placeholder size
                let unsigned_tx = node_manager::TransferBuilder::new(rpc.as_ref(), is_mainnet)
                    .with_placeholder_lock_size(witness_lock_size)
                    .build_unsigned(&from_address, &to_address, capacity_sh, fee_rate, None)
                    .map_err(|e| format!("Failed to build transaction: {}", e))?;

                // Fetch input cells for CKB_TX_MESSAGE_ALL
                let input_cells = node_manager::fetch_input_cells(rpc.as_ref(), &unsigned_tx)
                    .map_err(|e| format!("Failed to fetch input cells: {}", e))?;

                Ok((unsigned_tx, input_cells, lock_args))
            })();

            let _ = tx.send(result);
        });
    }

    /// After Touch ID returns the PRF output, compute the CKB_TX_MESSAGE_ALL hash,
    /// sign with SPHINCS+, fill the witness, and send the transaction in a background thread.
    #[cfg(target_os = "macos")]
    pub(crate) fn finish_sign_transfer(
        &mut self,
        prf_output: &qpv2_core::SecureVec,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    ) {
        use ckb_types::prelude::*;
        use qpv2_core::types::AuthKey;

        // Derive AES key from PRF output
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.transfer_status =
                    TransferStatus::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        // Get the wallet variant
        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.transfer_status =
                    TransferStatus::Error(format!("Failed to read variant: {}", e));
                return;
            }
        };

        // Compute CKB_TX_MESSAGE_ALL hash
        //
        // The `generate_ckb_tx_message_all` function expects `ckb_gen_types::packed::Transaction`
        // and `ckb_gen_types::packed::CellOutput`. Since `ckb_types` re-exports `ckb_gen_types`,
        // `ckb_types::packed::Transaction` is the same type. We get the packed Transaction from
        // TransactionView via `.data()`.
        let packed_tx = unsigned_tx.data();
        let mut hasher = ckb_fips205_utils::Hasher::message_hasher();

        // Convert input cells from ckb_types to the format expected by generate_ckb_tx_message_all.
        // Both use ckb_gen_types::packed types under the hood, but we need to use
        // the ckb_gen_types re-export from ckb-fips205-utils.
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

        // The packed_tx from ckb_types::packed::Transaction needs converting to
        // ckb_gen_types::packed::Transaction too.
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
            self.transfer_status =
                TransferStatus::Error(format!("Failed to compute tx message: {:?}", e));
            return;
        }
        let message = hasher.hash().to_vec();

        // Sign with SPHINCS+
        let vault = KeyVault::new(variant);
        let signature_bytes = match vault.ckb_sign(AuthKey::CryptoKey(key), lock_args, message) {
            Ok(sig) => sig,
            Err(e) => {
                self.transfer_status = TransferStatus::Error(format!("Signing failed: {}", e));
                return;
            }
        };

        // Fill witness
        let signed_tx = match node_manager::fill_witness(unsigned_tx, 0, signature_bytes) {
            Ok(tx) => tx,
            Err(e) => {
                self.transfer_status =
                    TransferStatus::Error(format!("Failed to fill witness: {}", e));
                return;
            }
        };

        // Send in background thread
        self.transfer_status = TransferStatus::Sending;
        let node_config = self.node_config.clone();
        let (tx_send, rx_send) = mpsc::channel();
        self.transfer_send_rx = Some(rx_send);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);
            let result = node_manager::send_transaction(rpc.as_ref(), &signed_tx)
                .map(|hash| format!("{:#x}", hash))
                .map_err(|e| format!("Failed to send transaction: {}", e));
            let _ = tx_send.send(result);
        });
    }

    /// Poll the transfer build channel. When the unsigned tx is ready, trigger Touch ID.
    pub(crate) fn poll_transfer_build(&mut self, frame: &eframe::Frame) {
        let rx = match &self.transfer_build_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok((unsigned_tx, input_cells, lock_args))) => {
                self.transfer_build_rx = None;
                // Tx built successfully — now trigger Touch ID for signing
                #[cfg(target_os = "macos")]
                {
                    let window = match Self::get_ns_window(frame) {
                        Ok(w) => w,
                        Err(e) => {
                            self.transfer_status =
                                TransferStatus::Error(format!("Failed to get window: {}", e));
                            return;
                        }
                    };
                    let credential_id = match self.get_credential_id() {
                        Some(id) => id,
                        None => {
                            self.transfer_status =
                                TransferStatus::Error("Failed to read credential.".to_string());
                            return;
                        }
                    };

                    let rp_id = "quantumpurse.org";
                    let salt = b"quantumpurse-kv-seed-encryption\0";
                    match passkey_prf::assert_async(&window, rp_id, &credential_id, Some(salt)) {
                        Ok(op) => {
                            use crate::types::PasskeyOp;
                            self.passkey_op = Some(PasskeyOp::SignTransferAssert {
                                op,
                                unsigned_tx,
                                input_cells,
                                lock_args,
                            });
                            self.transfer_status = TransferStatus::AwaitingSignature;
                        }
                        Err(passkey_prf::PrfError::Cancelled) => {
                            self.transfer_status = TransferStatus::Idle;
                            self.status = Status::Info("Transfer cancelled.".to_string());
                        }
                        Err(e) => {
                            self.transfer_status =
                                TransferStatus::Error(format!("PRF assertion failed: {}", e));
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (frame, unsigned_tx, input_cells, lock_args);
                    self.transfer_status =
                        TransferStatus::Error("Signing is only supported on macOS.".to_string());
                }
            }
            Ok(Err(e)) => {
                self.transfer_build_rx = None;
                self.transfer_status = TransferStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still building
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.transfer_build_rx = None;
                if matches!(self.transfer_status, TransferStatus::Building) {
                    self.transfer_status =
                        TransferStatus::Error("Build thread terminated unexpectedly.".to_string());
                }
            }
        }
    }

    /// Poll the transfer send channel for the final result.
    pub(crate) fn poll_transfer_send(&mut self) {
        let rx = match &self.transfer_send_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok(tx_hash)) => {
                self.transfer_send_rx = None;
                // Strip the 0x prefix if present for consistent display
                let hash = tx_hash.trim_start_matches("0x").to_string();
                self.transfer_status = TransferStatus::Success(hash);
                // Clear form fields after successful send
                self.transfer_recipient.clear();
                self.transfer_amount.clear();
                // Refresh balances since they changed
                self.fetch_all_balances();
            }
            Ok(Err(e)) => {
                self.transfer_send_rx = None;
                self.transfer_status = TransferStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still sending
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.transfer_send_rx = None;
                if matches!(self.transfer_status, TransferStatus::Sending) {
                    self.transfer_status =
                        TransferStatus::Error("Send thread terminated unexpectedly.".to_string());
                }
            }
        }
    }
}
