//! Async polling for background operations (passkeys, balances, transactions).

use crate::passkey::{PRF_SALT, RP_ID};
use crate::types::{
    DaoQueryEvent, PasskeyOp, SpendableCapacityTarget, Status, TransactionKind, TransactionStatus,
    TxHistoryEvent,
};
use crate::App;
use std::sync::mpsc;

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
                        let credential_id = registration.credential_id.clone();
                        match passkey_prf::assert_async(
                            &window,
                            RP_ID,
                            &credential_id,
                            Some(PRF_SALT),
                        ) {
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
                    self.create_wallet_finish(variant, &credential_id, &prf_output);
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
                    self.unlock_finish();
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
                    self.create_new_account_finish(&prf_output);
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

    /// Poll the spendable capacity channel and route the result by target.
    pub(crate) fn poll_spendable_capacity(&mut self) {
        let (target, rx) = match &self.spendable_capacity_rx {
            Some((t, rx)) => (*t, rx),
            None => return,
        };

        match rx.try_recv() {
            Ok(Ok(total_spendable_sh)) => {
                self.spendable_capacity_rx = None;
                let formatted = crate::types::format_ckb(total_spendable_sh);
                match target {
                    SpendableCapacityTarget::Transfer => {
                        self.transfer_all = true;
                        self.transfer_amount = formatted;
                    }
                    SpendableCapacityTarget::DaoDeposit => {
                        self.dao_deposit_all = true;
                        self.dao_deposit_amount = formatted;
                    }
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
                    let window = match crate::window_handle::get_ns_window(frame) {
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

                    match passkey_prf::assert_async(&window, RP_ID, &credential_id, Some(PRF_SALT))
                    {
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
                        self.transfer_all = false;
                    }
                    TransactionKind::Dao => {
                        self.dao_deposit_amount.clear();
                        self.dao_deposit_all = false;
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
                    self.dao_deposited_staging.push((lock_args, cell));
                }
                Ok(Ok(DaoQueryEvent::Prepared(lock_args, cell))) => {
                    self.dao_prepared_staging.push((lock_args, cell));
                }
                Ok(Ok(DaoQueryEvent::Done)) => {
                    // Atomic swap: replace display vectors with complete staging data.
                    std::mem::swap(
                        &mut self.dao_deposited_cells,
                        &mut self.dao_deposited_staging,
                    );
                    std::mem::swap(&mut self.dao_prepared_cells, &mut self.dao_prepared_staging);
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

    /// Poll the transaction history channel.
    pub(crate) fn poll_tx_history(&mut self) {
        let rx = match &self.tx_history_rx {
            Some(rx) => rx,
            None => return,
        };

        loop {
            match rx.try_recv() {
                Ok(Ok(TxHistoryEvent::Record(record))) => {
                    self.tx_history.push(record);
                }
                Ok(Ok(TxHistoryEvent::Done)) => {
                    // Sort by block number descending and deduplicate by tx hash.
                    self.tx_history
                        .sort_by(|a, b| b.block_number.cmp(&a.block_number));
                    self.tx_history.dedup_by(|a, b| a.tx_hash == b.tx_hash);
                    self.tx_history_rx = None;
                    break;
                }
                Ok(Err(_e)) => {
                    // Silently ignore individual fetch errors; partial results are fine.
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.tx_history_rx = None;
                    break;
                }
            }
        }
    }
}
