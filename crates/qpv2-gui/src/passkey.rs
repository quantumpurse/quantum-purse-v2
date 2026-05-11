//! Passkey / Touch ID integration (macOS only).
//!
//! All `passkey_prf` crate usage is consolidated here so the rest of
//! the GUI can be compiled without the macOS-only dependency.

use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use crate::types::{PasskeyOp, Screen, Status, TransactionKind, TransactionStatus};
use crate::App;

pub(crate) const RP_ID: &str = "quantumpurse.org";
pub(crate) const PRF_SALT: &[u8] = b"quantumpurse-kv-seed-encryption\0";

/// Extracts the NSWindow from the eframe Frame via raw-window-handle.
fn get_ns_window(frame: &eframe::Frame) -> Result<Retained<NSWindow>, String> {
    let handle = frame
        .window_handle()
        .map_err(|e| format!("Failed to get window handle: {}", e))?;

    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit_handle) => {
            let ns_view_ptr = appkit_handle.ns_view.as_ptr();
            // SAFETY: The pointer came from eframe's WindowHandle, which guarantees
            // it points to a valid NSView. We are on the main thread because eframe's
            // update() runs on the main thread.
            let ns_view: Retained<NSView> = unsafe { Retained::retain(ns_view_ptr.cast()) }
                .ok_or("Failed to retain NSView from raw pointer")?;
            ns_view
                .window()
                .ok_or_else(|| "NSView is not installed in a window".to_string())
        }
        other => Err(format!("Unexpected window handle type: {:?}", other)),
    }
}

impl App {
    /// Read the stored credential ID for passkey-based wallets.
    pub(crate) fn get_credential_id(&mut self) -> Option<Vec<u8>> {
        let wallet_info = match KeyVault::read_wallet_info() {
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
    pub(crate) fn create_wallet_with_passkey_start(&mut self, frame: &mut eframe::Frame) {
        let window = match get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                return;
            }
        };

        // TODO users must specify these info.
        let user_id = b"qpv2-user";
        let user_name = "tea";

        match passkey_prf::register_passkey_async(&window, RP_ID, user_id, user_name) {
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

    /// Complete wallet creation after receiving the PRF output.
    pub(crate) fn create_wallet_with_passkey_finish(
        &mut self,
        variant: SpxVariant,
        credential_id: &[u8],
        prf_output: &qpv2_core::SecureVec,
    ) {
        let key = match qpv2_core::utilities::derive_vault_enc_key(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };
        let cloned_key = key.clone();

        let vault = KeyVault::new(variant);
        let auth_method = AuthMethod::PasskeyPrf {
            credential_id: credential_id.to_vec(),
        };
        if let Err(e) = vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method) {
            self.status = Status::Error(format!("Failed to create wallet: {}", e));
            return;
        }

        if let Err(e) = vault.gen_new_account(AuthKey::CryptoKey(cloned_key)) {
            self.status = Status::Error(format!("Failed to create first account: {}", e));
            self.screen = Screen::Locked;
            return;
        }

        // Read lock args from accounts.json (no decryption needed).
        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                // First account of a brand-new wallet — if a light
                // client is running, start indexing it from the tip.
                self.register_lock_scripts_with_light_client(&self.accounts.clone());
                self.screen = Screen::Unlocked;
                self.status = Status::Info("Wallet created successfully!".to_string());
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.screen = Screen::Locked;
            }
        }
    }

    /// Kick off async credential-only assertion (no PRF) for unlock.
    pub(crate) fn unlock_with_passkey_start(&mut self, frame: &mut eframe::Frame) {
        let window = match get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                return;
            }
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => return,
        };

        match passkey_prf::assert_async(&window, RP_ID, &credential_id, None) {
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

    /// Complete wallet unlock after credential assertion succeeds.
    pub(crate) fn unlock_with_passkey_finish(&mut self) {
        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.screen = Screen::Unlocked;
                self.status = Status::None;
                self.last_poll_time = std::time::Instant::now();
                self.load_tx_history_from_disk();
                self.fetch_all_balances();
                self.fetch_tx_history(true);
                self.fetch_dao_cells();
                self.fetch_node_status();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to unlock: {}", e));
            }
        }
    }

    /// Kick off async PRF assertion to create a new account (Touch ID).
    pub(crate) fn create_new_account_with_passkey_start(&mut self, frame: &mut eframe::Frame) {
        let window = match get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.status = Status::Error(format!("Failed to get window: {}", e));
                return;
            }
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => return,
        };

        match passkey_prf::assert_async(&window, RP_ID, &credential_id, Some(PRF_SALT)) {
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

    /// Complete new account creation after receiving the PRF output.
    pub(crate) fn create_new_account_with_passkey_finish(
        &mut self,
        prf_output: &qpv2_core::SecureVec,
    ) {
        let key = match qpv2_core::utilities::derive_vault_enc_key(prf_output) {
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
                let qp_client = self.qp_client.clone();
                let args = lock_args.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.balance_receiver = Some(rx);

                std::thread::spawn(move || {
                    let result = ckb_node::wallet_helpers::queries::fetch_quantum_lock_balance(
                        &qp_client, &args,
                    )
                    .map_err(|e| e.to_string());
                    let _ = tx.send((args, result));
                });
                self.accounts.push(lock_args.clone());
                // Register only the new account with the light client
                // (no-op on other backends). Don't pass all accounts
                // here — `set_scripts(Partial)` would overwrite the
                // sync cursors of the existing ones.
                self.register_lock_scripts_with_light_client(std::slice::from_ref(&lock_args));
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }

    pub(crate) fn sign_with_passkey_start(
        &mut self,
        frame: &eframe::Frame,
        kind: TransactionKind,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    ) {
        let window = match get_ns_window(frame) {
            Ok(w) => w,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Failed to get window: {}", e));
                return;
            }
        };
        let credential_id = match self.get_credential_id() {
            Some(id) => id,
            None => {
                self.tx_status = TransactionStatus::Error("Failed to read credential.".to_string());
                return;
            }
        };

        match passkey_prf::assert_async(&window, RP_ID, &credential_id, Some(PRF_SALT)) {
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
                self.status = Status::Info("Transaction building cancelled.".to_string());
            }
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("PRF assertion failed: {}", e));
            }
        }
    }

    /// After Touch ID returns the PRF output, derive the encryption
    /// key and hand it to the auth-agnostic signing core.
    pub(crate) fn sign_with_passkey_finish(
        &mut self,
        kind: TransactionKind,
        prf_output: &qpv2_core::SecureVec,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    ) {
        use qpv2_core::types::AuthKey;

        let key = match qpv2_core::utilities::derive_vault_enc_key(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.tx_status = TransactionStatus::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };
        self.sign_and_send(
            kind,
            AuthKey::CryptoKey(key),
            unsigned_tx,
            input_cells,
            lock_args,
        );
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
                    self.create_wallet_with_passkey_finish(variant, &credential_id, &prf_output);
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
                    self.unlock_with_passkey_finish();
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
                    self.create_new_account_with_passkey_finish(&prf_output);
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
                    self.sign_with_passkey_finish(
                        kind,
                        &prf_output,
                        unsigned_tx,
                        input_cells,
                        lock_args,
                    );
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
}
