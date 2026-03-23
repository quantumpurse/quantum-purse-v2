//! Passkey registration, assertion, and polling flows (macOS only).

use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use crate::types::{DaoStatus, PasskeyOp, Screen, Status, TransferStatus};
use crate::App;

impl App {
    /// Get the NSWindow handle, setting an error status on failure.
    pub(crate) fn get_ns_window_or_err(
        &mut self,
        frame: &eframe::Frame,
    ) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
        match Self::get_ns_window(frame) {
            Ok(w) => Some(w),
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                None
            }
        }
    }

    /// Read the stored credential ID for passkey-based wallets.
    pub(crate) fn get_credential_id(&mut self) -> Option<Vec<u8>> {
        let temp_vault = KeyVault::new(SpxVariant::Sha2128S);
        let wallet_info = match temp_vault.read_wallet_info() {
            Ok(info) => info,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet info: {}", e));
                return None;
            }
        };
        match wallet_info.auth_method {
            AuthMethod::PasskeyPrf { credential_id } => Some(credential_id),
            AuthMethod::Password => {
                self.status =
                    Status::Error("This wallet uses password auth, not Touch ID.".to_string());
                None
            }
        }
    }

    /// Kick off async passkey registration.
    pub(crate) fn start_registration(&mut self, frame: &mut eframe::Frame) {
        let window = match self.get_ns_window_or_err(frame) {
            Some(w) => w,
            None => return,
        };

        let rp_id = "quantumpurse.org";
        let user_id = b"qpv2-user";
        let user_name = "tea";

        match passkey_prf::register_passkey_async(&window, rp_id, user_id, user_name) {
            Ok(op) => {
                self.passkey_op = Some(PasskeyOp::Registration {
                    op,
                    variant: self.selected_variant,
                    window,
                });
            }
            Err(e) => {
                self.status = Status::Error(format!("Passkey registration failed: {}", e));
            }
        }
    }

    /// Kick off async credential-only assertion (no PRF) for unlock.
    pub(crate) fn start_unlock(&mut self, frame: &mut eframe::Frame) {
        let window = match self.get_ns_window_or_err(frame) {
            Some(w) => w,
            None => return,
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => return,
        };

        let rp_id = "quantumpurse.org";
        match passkey_prf::assert_async(&window, rp_id, &credential_id, None) {
            Ok(op) => {
                self.passkey_op = Some(PasskeyOp::UnlockAssert { op });
            }
            Err(passkey_prf::PrfError::Cancelled) => {
                self.status = Status::Info("Cancelled.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Credential assertion failed: {}", e));
            }
        }
    }

    /// Kick off async PRF assertion to create a new account (requires seed decryption).
    pub(crate) fn start_create_new_account(&mut self, frame: &mut eframe::Frame) {
        let window = match self.get_ns_window_or_err(frame) {
            Some(w) => w,
            None => return,
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => return,
        };

        let rp_id = "quantumpurse.org";
        let salt = b"quantumpurse-kv-seed-encryption\0";
        match passkey_prf::assert_async(&window, rp_id, &credential_id, Some(salt)) {
            Ok(op) => {
                self.passkey_op = Some(PasskeyOp::NewAccountAssert { op });
                self.status = Status::Info("Authenticate with Touch ID...".to_string());
            }
            Err(passkey_prf::PrfError::Cancelled) => {
                self.status = Status::Info("Cancelled.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("PRF assertion failed: {}", e));
            }
        }
    }

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
            PasskeyOp::SignTransferAssert {
                op,
                unsigned_tx,
                input_cells,
                lock_args,
            } => match op.poll() {
                None => {
                    self.passkey_op = Some(PasskeyOp::SignTransferAssert {
                        op,
                        unsigned_tx,
                        input_cells,
                        lock_args,
                    });
                }
                Some(Ok(Some(prf_output))) => {
                    self.finish_sign_transfer(&prf_output, unsigned_tx, input_cells, lock_args);
                }
                Some(Ok(None)) => {
                    self.transfer_status = TransferStatus::Error(
                        "Internal error: Expected encryption key from authentication.".to_string(),
                    );
                }
                Some(Err(passkey_prf::PrfError::Cancelled)) => {
                    self.transfer_status = TransferStatus::Idle;
                    self.status = Status::Info("Transfer cancelled.".to_string());
                }
                Some(Err(e)) => {
                    self.transfer_status =
                        TransferStatus::Error(format!("Authentication failed: {}", e));
                }
            },
            PasskeyOp::SignDaoAssert {
                op,
                unsigned_tx,
                input_cells,
                lock_args,
            } => match op.poll() {
                None => {
                    self.passkey_op = Some(PasskeyOp::SignDaoAssert {
                        op,
                        unsigned_tx,
                        input_cells,
                        lock_args,
                    });
                }
                Some(Ok(Some(prf_output))) => {
                    self.finish_sign_dao(&prf_output, unsigned_tx, input_cells, lock_args);
                }
                Some(Ok(None)) => {
                    self.dao_status = DaoStatus::Error(
                        "Internal error: Expected encryption key from authentication.".to_string(),
                    );
                }
                Some(Err(passkey_prf::PrfError::Cancelled)) => {
                    self.dao_status = DaoStatus::Idle;
                    self.status = Status::Info("DAO operation cancelled.".to_string());
                }
                Some(Err(e)) => {
                    self.dao_status = DaoStatus::Error(format!("Authentication failed: {}", e));
                }
            },
        }
    }

    /// Complete wallet creation after receiving the PRF output.
    pub(crate) fn finish_wallet_creation(
        &mut self,
        variant: SpxVariant,
        credential_id: &[u8],
        prf_output: &qpv2_core::SecureVec,
    ) {
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let vault = KeyVault::new(variant);
        let auth_method = AuthMethod::PasskeyPrf {
            credential_id: credential_id.to_vec(),
        };
        if let Err(e) = vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method) {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        // Re-derive key to generate the first account.
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                self.screen = Screen::Locked;
                return;
            }
        };
        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(key)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.screen = Screen::Locked;
            return;
        }

        // Read lock args from accounts.json (no decryption needed).
        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.screen = Screen::Unlocked;
                self.status = Status::Info("Wallet created successfully!".to_string());
                self.connect_and_fetch_balances();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.screen = Screen::Locked;
            }
        }
    }

    /// Complete wallet unlock after credential assertion succeeds.
    pub(crate) fn finish_unlock(&mut self) {
        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.screen = Screen::Unlocked;
                self.status = Status::None;
                self.connect_and_fetch_balances();
                self.fetch_dao_cells();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to unlock: {}", e));
            }
        }
    }

    /// Complete new account creation after receiving the PRF output.
    pub(crate) fn finish_create_new_account(&mut self, prf_output: &qpv2_core::SecureVec) {
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet variant: {}", e));
                return;
            }
        };

        let vault = KeyVault::new(variant);
        match vault.gen_new_account(AuthKey::CryptoKey(key)) {
            Ok(lock_args) => {
                // Mark as loading and fetch balance in the background.
                self.balances.insert(lock_args.clone(), None);
                if self.rpc_client.is_some() {
                    let node_config = self.node_config.clone();
                    let network = self.node_config.network;
                    let args = lock_args.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.balance_receiver = Some(rx);

                    std::thread::spawn(move || {
                        let rpc = node_manager::connect(&node_config);
                        let result =
                            node_manager::fetch_quantum_lock_balance(rpc.as_ref(), &args, network)
                                .map_err(|e| e.to_string());
                        let _ = tx.send((args, result));
                    });
                }
                self.accounts.push(lock_args);
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }
}
