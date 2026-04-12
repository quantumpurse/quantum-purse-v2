//! DAO transaction building, signing, and sending.

use crate::types::{PasskeyOp, Status, TransactionKind, TransactionStatus};
use crate::App;
use std::sync::mpsc;

use crate::types::DaoQueryEvent;

impl App {
    /// Poll op passkey operations each frame.
    pub(crate) fn poll_passkey_ops(&mut self) {
        let op = match self.passkey_op.take() {
            Some(op) => op,
            None => return,
        };

        match op {
            PasskeyOp::Registration {
                op,
                variant,
                window,
            } => {
                match op.poll() {
                    None => {
                        // Still waiting — put it back.
                        self.passkey_op = Some(PasskeyOp::Registration {
                            op,
                            variant,
                            window,
                        });
                    }
                    Some(Ok(registration)) => {
                        if !registration.prf_supported {
                            self.status = Status::Error(
                                "PRF not supported by this authenticator.".to_string(),
                            );
                            return;
                        }

                        // Registration succeeded — now assert with PRF to get the encryption key.
                        let rp_id = "quantumpurse.org";
                        let salt = b"quantumpurse-kv-seed-encryption\0";
                        let credential_id = registration.credential_id.clone();
                        match passkey_prf::assert_async(&window, rp_id, &credential_id, Some(salt))
                        {
                            Ok(assert_pending) => {
                                self.passkey_op = Some(PasskeyOp::PostRegistrationAssert {
                                    op: assert_pending,
                                    variant,
                                    credential_id,
                                });
                                self.status = Status::Info(
                                    "Passkey registered. Now authenticate with Touch ID..."
                                        .to_string(),
                                );
                            }
                            Err(e) => {
                                self.status = Status::Error(format!("PRF assertion failed: {}", e));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        self.status = Status::Error(format!("Passkey registration failed: {}", e));
                    }
                }
            }
            PasskeyOp::PostRegistrationAssert {
                op,
                variant,
                credential_id,
            } => match op.poll() {
                None => {
                    self.passkey_op = Some(PasskeyOp::PostRegistrationAssert {
                        op,
                        variant,
                        credential_id,
                    });
                }
                Some(Ok(Some(prf_output))) => {
                    self.finish_wallet_creation(variant, &credential_id, &prf_output);
                }
                Some(Ok(None)) => {
                    self.status = Status::Error(
                        "Internal error: Expected encryption key from authentication.".to_string(),
                    );
                }
                Some(Err(passkey_prf::PrfError::Cancelled)) => {
                    self.status = Status::Info("Cancelled.".to_string());
                }
                Some(Err(e)) => {
                    self.status = Status::Error(format!("Authentication failed: {}", e));
                }
            },
            PasskeyOp::UnlockAssert { op } => match op.poll() {
                None => {
                    self.passkey_op = Some(PasskeyOp::UnlockAssert { op });
                }
                Some(Ok(_)) => {
                    self.finish_unlock();
                }
                Some(Err(passkey_prf::PrfError::Cancelled)) => {
                    self.status = Status::Info("Cancelled.".to_string());
                }
                Some(Err(e)) => {
                    self.status = Status::Error(format!("Authentication failed: {}", e));
                }
            },
            PasskeyOp::NewAccountAssert { op } => match op.poll() {
                None => {
                    self.passkey_op = Some(PasskeyOp::NewAccountAssert { op });
                }
                Some(Ok(Some(prf_output))) => {
                    self.finish_create_new_account(&prf_output);
                }
                Some(Ok(None)) => {
                    self.status = Status::Error(
                        "Internal error: Expected encryption key from authentication.".to_string(),
                    );
                }
                Some(Err(passkey_prf::PrfError::Cancelled)) => {
                    self.status = Status::Info("Cancelled.".to_string());
                }
                Some(Err(e)) => {
                    self.status = Status::Error(format!("Authentication failed: {}", e));
                }
            },
            PasskeyOp::SignTransactionAssert {
                op,
                kind,
                unsigned_tx,
                input_cells,
                lock_args,
            } => match op.poll() {
                None => {
                    self.passkey_op = Some(PasskeyOp::SignTransactionAssert {
                        op,
                        kind,
                        unsigned_tx,
                        input_cells,
                        lock_args,
                    });
                }
                Some(Ok(Some(prf_output))) => {
                    self.sign_and_send(kind, &prf_output, unsigned_tx, input_cells, lock_args);
                }
                Some(Ok(None)) => {
                    self.tx_status = TransactionStatus::Error(
                        "Internal error: Expected encryption key from authentication.".to_string(),
                    );
                }
                Some(Err(passkey_prf::PrfError::Cancelled)) => {
                    self.tx_status = TransactionStatus::Idle;
                    self.status = Status::Info("Signing cancelled.".to_string());
                }
                Some(Err(e)) => {
                    self.tx_status =
                        TransactionStatus::Error(format!("Authentication failed: {}", e));
                }
            },
        }
    }

    /// Poll the max transfer amount channel and update the amount field when ready.
    pub(crate) fn poll_spendable_capacity(&mut self) {
        let rx = match &self.spendable_capacity_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok(total_spendable_sh)) => {
                self.spendable_capacity_rx = None;
                self.transfer_all = true;
                let whole = total_spendable_sh / crate::types::CKB_DECIMAL_PLACES;
                let frac = total_spendable_sh % crate::types::CKB_DECIMAL_PLACES;
                if frac == 0 {
                    self.transfer_amount = format!("{}", whole);
                } else {
                    let frac_str = format!("{:08}", frac);
                    let trimmed = frac_str.trim_end_matches('0');
                    self.transfer_amount = format!("{}.{}", whole, trimmed);
                }
            }
            Ok(Err(e)) => {
                self.spendable_capacity_rx = None;
                self.tx_status = TransactionStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.spendable_capacity_rx = None;
            }
        }
    }

    /// Poll the transaction from channel and trigger Touch ID on success.
    pub(crate) fn poll_transaction_build(&mut self, frame: &eframe::Frame) {
        let rx = match &self.transaction_build_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok((kind, unsigned_tx, input_cells, lock_args))) => {
                self.transaction_build_rx = None;
                #[cfg(target_os = "macos")]
                {
                    let window = match Self::get_ns_window(frame) {
                        Ok(w) => w,
                        Err(e) => {
                            self.tx_status =
                                TransactionStatus::Error(format!("Failed to get window: {}", e));
                            return;
                        }
                    };
                    let credential_id = match self.get_credential_id() {
                        Some(id) => id,
                        None => {
                            self.tx_status =
                                TransactionStatus::Error("Failed to read credential.".to_string());
                            return;
                        }
                    };

                    let rp_id = "quantumpurse.org";
                    let salt = b"quantumpurse-kv-seed-encryption\0";
                    match passkey_prf::assert_async(&window, rp_id, &credential_id, Some(salt)) {
                        Ok(op) => {
                            self.passkey_op = Some(PasskeyOp::SignTransactionAssert {
                                op,
                                kind,
                                unsigned_tx,
                                input_cells,
                                lock_args,
                            });
                            self.tx_status = TransactionStatus::AwaitingSignature;
                        }
                        Err(passkey_prf::PrfError::Cancelled) => {
                            self.tx_status = TransactionStatus::Idle;
                            self.status =
                                Status::Info("Transaction building cancelled.".to_string());
                        }
                        Err(e) => {
                            self.tx_status =
                                TransactionStatus::Error(format!("PRF assertion failed: {}", e));
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (frame, unsigned_tx, input_cells, lock_args);
                    self.tx_status =
                        TransactionStatus::Error("Signing is only supported on macOS.".to_string());
                }
            }
            Ok(Err(e)) => {
                self.transaction_build_rx = None;
                self.tx_status = TransactionStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.transaction_build_rx = None;
                if matches!(self.tx_status, TransactionStatus::Building) {
                    self.tx_status = TransactionStatus::Error(
                        "Build thread terminated unexpectedly.".to_string(),
                    );
                }
            }
        }
    }

    /// Poll the shared transaction send channel and dispatch follow-up work by kind.
    pub(crate) fn poll_transaction_send(&mut self) {
        let rx = match &self.transaction_send_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok((kind, Ok(tx_hash))) => {
                self.transaction_send_rx = None;
                let hash = tx_hash.trim_start_matches("0x").to_string();
                self.tx_status = TransactionStatus::Success(hash);

                match kind {
                    TransactionKind::Transfer => {
                        self.transfer_recipient.clear();
                        self.transfer_amount.clear();
                        self.fetch_all_balances();
                    }
                    TransactionKind::Dao => {
                        self.dao_deposit_amount.clear();
                        self.fetch_all_balances();
                        self.fetch_dao_cells();
                    }
                }
            }
            Ok((_, Err(e))) => {
                self.transaction_send_rx = None;
                self.tx_status = TransactionStatus::Error(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.transaction_send_rx = None;
                if matches!(self.tx_status, TransactionStatus::Sending) {
                    self.tx_status = TransactionStatus::Error(
                        "Send thread terminated unexpectedly.".to_string(),
                    );
                }
            }
        }
    }

    /// Drain available balance results from the background thread.
    pub(crate) fn poll_all_balances(&mut self) {
        let rx = match &self.balance_receiver {
            Some(rx) => rx,
            None => return,
        };

        // fetching all available results from the mpsc::channel's buffer.
        loop {
            match rx.try_recv() {
                Ok((lock_args, Ok(balance))) => {
                    self.balances.insert(lock_args, Some(balance));
                }
                Ok((lock_args, Err(e))) => {
                    self.balances.insert(lock_args, None);
                    self.status = Status::Error(format!("Failed to fetch balance: {}", e));
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Background thread finished; drop the receiver.
                    self.balance_receiver = None;
                    break;
                }
            }
        }
    }

    /// Poll the DAO cell query channel.
    pub(crate) fn poll_dao_cells(&mut self) {
        let rx = match &self.dao_cells_query_rx {
            Some(rx) => rx,
            None => return,
        };

        // Drain all available DAO query events from the channel buffer.
        loop {
            match rx.try_recv() {
                Ok(Ok(DaoQueryEvent::Deposited(lock_args, cell))) => {
                    self.dao_deposited_cells.push((lock_args, cell));
                }
                Ok(Ok(DaoQueryEvent::Prepared(lock_args, cell))) => {
                    self.dao_prepared_cells.push((lock_args, cell));
                }
                Ok(Ok(DaoQueryEvent::Done)) => {
                    self.dao_cells_query_rx = None;
                    break;
                }
                Ok(Err(e)) => {
                    self.dao_cells_query_rx = None;
                    self.status = Status::Error(e);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.dao_cells_query_rx = None;
                    break;
                }
            }
        }
    }
}
