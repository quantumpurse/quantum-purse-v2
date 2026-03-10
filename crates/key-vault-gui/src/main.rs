//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

#[cfg(target_os = "macos")]
mod window_handle;

use eframe::egui;
use key_vault_core::types::{AuthKey, AuthMethod, SpxVariant};
use key_vault_core::KeyVault;

/// Application state machine.
#[derive(Debug, Clone, PartialEq)]
enum Screen {
    /// No wallet exists yet — user chooses variant and creates one.
    Setup,
    /// Wallet exists — waiting for Touch ID to unlock.
    Locked,
    /// Wallet unlocked — show wallet info.
    Unlocked,
}

/// Status messages shown to the user.
#[derive(Debug, Clone)]
enum Status {
    None,
    Info(String),
    Error(String),
}

/// Tracks in-flight passkey operations so the UI doesn't block.
#[cfg(target_os = "macos")]
enum PendingOp {
    /// Waiting for passkey registration to complete.
    Registration {
        pending: passkey_prf::PendingRegistration,
        variant: SpxVariant,
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    },
    /// Registration done; waiting for PRF assertion to get the encryption key.
    PostRegistrationAssert {
        pending: passkey_prf::AssertionRequest,
        variant: SpxVariant,
        credential_id: Vec<u8>,
    },
    /// Waiting for unlock credential assertion (no PRF).
    UnlockAssert {
        pending: passkey_prf::AssertionRequest,
    },
    /// Waiting for PRF assertion to create a new account.
    NewAccountAssert {
        pending: passkey_prf::AssertionRequest,
    },
}

struct App {
    screen: Screen,
    status: Status,

    // Setup screen state.
    selected_variant: SpxVariant,

    // Unlocked screen state.
    accounts: Vec<String>,
    is_mainnet: bool,
    confirm_remove: bool,

    // In-flight passkey operation (macOS only).
    #[cfg(target_os = "macos")]
    pending_op: Option<PendingOp>,
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Check if a wallet already exists by trying to read wallet info.
        let screen = if KeyVault::new(SpxVariant::Sha2128S).wallet_exists() {
            Screen::Locked
        } else {
            Screen::Setup
        };

        Self {
            screen,
            status: Status::None,
            selected_variant: SpxVariant::Sha2128S,
            accounts: Vec::new(),
            is_mainnet: false,
            confirm_remove: false,
            #[cfg(target_os = "macos")]
            pending_op: None,
        }
    }

    /// Extract the NSWindow from the eframe Frame (macOS only).
    #[cfg(target_os = "macos")]
    fn get_ns_window(
        frame: &eframe::Frame,
    ) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, String> {
        window_handle::get_ns_window(frame)
    }

    fn show_setup(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("Create New Wallet");
        ui.add_space(8.0);

        ui.label("Select SPHINCS+ variant:");
        egui::ComboBox::from_id_salt("variant")
            .selected_text(format!("{}", self.selected_variant))
            .show_ui(ui, |ui| {
                for variant in &[
                    SpxVariant::Sha2128S,
                    SpxVariant::Sha2128F,
                    SpxVariant::Shake128S,
                    SpxVariant::Shake128F,
                    SpxVariant::Sha2192S,
                    SpxVariant::Sha2192F,
                    SpxVariant::Shake192S,
                    SpxVariant::Shake192F,
                    SpxVariant::Sha2256S,
                    SpxVariant::Sha2256F,
                    SpxVariant::Shake256S,
                    SpxVariant::Shake256F,
                ] {
                    ui.selectable_value(
                        &mut self.selected_variant,
                        *variant,
                        format!("{}", variant),
                    );
                }
            });

        ui.add_space(12.0);

        #[cfg(target_os = "macos")]
        let is_busy = self.pending_op.is_some();
        #[cfg(not(target_os = "macos"))]
        let is_busy = false;

        let button = ui.add_enabled(
            !is_busy,
            egui::Button::new(if is_busy {
                "Creating wallet..."
            } else {
                "Create Wallet"
            }),
        );
        if button.clicked() {
            self.start_registration(frame);
        }

        self.show_status(ui);
    }

    fn show_locked(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("Wallet Locked");
        ui.add_space(12.0);

        #[cfg(target_os = "macos")]
        let is_busy = self.pending_op.is_some();
        #[cfg(not(target_os = "macos"))]
        let is_busy = false;

        let button = ui.add_enabled(
            !is_busy,
            egui::Button::new(if is_busy {
                "Waiting for Touch ID..."
            } else {
                "Unlock with Touch ID"
            }),
        );
        if button.clicked() {
            self.start_unlock(frame);
        }

        self.show_status(ui);
    }

    fn show_unlocked(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("Wallet Unlocked");
        ui.add_space(8.0);

        // Network toggle.
        ui.horizontal(|ui| {
            ui.label("Network:");
            ui.selectable_value(&mut self.is_mainnet, false, "Testnet");
            ui.selectable_value(&mut self.is_mainnet, true, "Mainnet");
        });
        ui.add_space(8.0);

        // Account list.
        if self.accounts.is_empty() {
            ui.label("No accounts yet.");
        } else {
            ui.label(format!("Accounts ({}):", self.accounts.len()));
            ui.add_space(4.0);
            for (i, lock_args) in self.accounts.iter().enumerate() {
                let address_text = match key_vault_core::utilities::lock_args_to_address(
                    lock_args,
                    self.is_mainnet,
                ) {
                    Ok(addr) => addr,
                    Err(_) => format!("0x{}", lock_args),
                };
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(format!("Account #{}", i));
                    ui.add(
                        egui::TextEdit::singleline(&mut address_text.as_str())
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                });
                ui.add_space(4.0);
            }
        }

        ui.add_space(8.0);

        #[cfg(target_os = "macos")]
        let is_busy = self.pending_op.is_some();
        #[cfg(not(target_os = "macos"))]
        let is_busy = false;

        ui.horizontal(|ui| {
            let new_acct_button = ui.add_enabled(
                !is_busy,
                egui::Button::new(if is_busy {
                    "Creating account..."
                } else {
                    "New Account"
                }),
            );
            if new_acct_button.clicked() {
                self.start_create_new_account(frame);
            }

            if ui.button("Lock Wallet").clicked() {
                self.accounts.clear();
                self.confirm_remove = false;
                self.screen = Screen::Locked;
                self.status = Status::None;
            }

            let remove_label = if self.confirm_remove {
                "Confirm Remove?"
            } else {
                "Remove Wallet"
            };
            let remove_button =
                egui::Button::new(egui::RichText::new(remove_label).color(egui::Color32::RED));
            if ui.add(remove_button).clicked() {
                if self.confirm_remove {
                    match KeyVault::clear_database() {
                        Ok(()) => {
                            self.accounts.clear();
                            self.confirm_remove = false;
                            self.screen = Screen::Setup;
                            self.status = Status::Info("Wallet removed successfully.".to_string());
                        }
                        Err(e) => {
                            self.status = Status::Error(format!("Failed to remove wallet: {}", e));
                        }
                    }
                } else {
                    self.confirm_remove = true;
                }
            }
        });

        self.show_status(ui);
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        match &self.status {
            Status::None => {}
            Status::Info(msg) => {
                ui.label(egui::RichText::new(msg).color(egui::Color32::GREEN));
            }
            Status::Error(msg) => {
                ui.label(egui::RichText::new(msg).color(egui::Color32::RED));
            }
        }
    }

    /// Kick off async passkey registration.
    fn start_registration(&mut self, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let window = match Self::get_ns_window(frame) {
                Ok(w) => w,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to get window: {}", e));
                    return;
                }
            };

            let rp_id = "quantumpurse.org";
            let user_id = b"qpkv-user";
            let user_name = "tea";

            match passkey_prf::register_passkey_async(&window, rp_id, user_id, user_name) {
                Ok(pending) => {
                    self.pending_op = Some(PendingOp::Registration {
                        pending,
                        variant: self.selected_variant,
                        window,
                    });
                    self.status = Status::Info("Touch ID prompt should appear...".to_string());
                }
                Err(e) => {
                    self.status = Status::Error(format!("Passkey registration failed: {}", e));
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = frame;
            self.status = Status::Error("Passkey PRF is only supported on macOS.".to_string());
        }
    }

    /// Kick off async credential-only assertion (no PRF) for unlock.
    fn start_unlock(&mut self, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let window = match Self::get_ns_window(frame) {
                Ok(w) => w,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to get window: {}", e));
                    return;
                }
            };

            let temp_vault = KeyVault::new(SpxVariant::Sha2128S);
            let wallet_info = match temp_vault.read_wallet_info() {
                Ok(info) => info,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to read wallet info: {}", e));
                    return;
                }
            };

            let credential_id = match &wallet_info.auth_method {
                AuthMethod::PasskeyPrf { credential_id } => credential_id.clone(),
                AuthMethod::Password => {
                    self.status =
                        Status::Error("This wallet uses password auth, not Touch ID.".to_string());
                    return;
                }
            };

            let rp_id = "quantumpurse.org";
            match passkey_prf::assert_async(&window, rp_id, &credential_id, None) {
                Ok(pending) => {
                    self.pending_op = Some(PendingOp::UnlockAssert { pending });
                    self.status = Status::Info("Touch ID prompt should appear...".to_string());
                }
                Err(passkey_prf::PrfError::Cancelled) => {
                    self.status = Status::Info("Cancelled.".to_string());
                }
                Err(e) => {
                    self.status = Status::Error(format!("Credential assertion failed: {}", e));
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = frame;
            self.status = Status::Error("Passkey PRF is only supported on macOS.".to_string());
        }
    }

    /// Kick off async PRF assertion to create a new account (requires seed decryption).
    fn start_create_new_account(&mut self, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let window = match Self::get_ns_window(frame) {
                Ok(w) => w,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to get window: {}", e));
                    return;
                }
            };

            let temp_vault = KeyVault::new(SpxVariant::Sha2128S);
            let wallet_info = match temp_vault.read_wallet_info() {
                Ok(info) => info,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to read wallet info: {}", e));
                    return;
                }
            };

            let credential_id = match &wallet_info.auth_method {
                AuthMethod::PasskeyPrf { credential_id } => credential_id.clone(),
                AuthMethod::Password => {
                    self.status =
                        Status::Error("This wallet uses password auth, not Touch ID.".to_string());
                    return;
                }
            };

            let rp_id = "quantumpurse.org";
            let salt = b"quantumpurse-kv-seed-encryption\0";
            match passkey_prf::assert_async(&window, rp_id, &credential_id, Some(salt)) {
                Ok(pending) => {
                    self.pending_op = Some(PendingOp::NewAccountAssert { pending });
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

        #[cfg(not(target_os = "macos"))]
        {
            let _ = frame;
            self.status = Status::Error("Passkey PRF is only supported on macOS.".to_string());
        }
    }

    /// Poll pending passkey operations each frame (macOS only).
    #[cfg(target_os = "macos")]
    fn poll_pending(&mut self) {
        let op = match self.pending_op.take() {
            Some(op) => op,
            None => return,
        };

        match op {
            PendingOp::Registration {
                pending,
                variant,
                window,
            } => {
                match pending.poll() {
                    None => {
                        // Still waiting — put it back.
                        self.pending_op = Some(PendingOp::Registration {
                            pending,
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
                                self.pending_op = Some(PendingOp::PostRegistrationAssert {
                                    pending: assert_pending,
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
            PendingOp::PostRegistrationAssert {
                pending,
                variant,
                credential_id,
            } => match pending.poll() {
                None => {
                    self.pending_op = Some(PendingOp::PostRegistrationAssert {
                        pending,
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
            PendingOp::UnlockAssert { pending } => match pending.poll() {
                None => {
                    self.pending_op = Some(PendingOp::UnlockAssert { pending });
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
            PendingOp::NewAccountAssert { pending } => match pending.poll() {
                None => {
                    self.pending_op = Some(PendingOp::NewAccountAssert { pending });
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
        }
    }

    /// Complete wallet creation after receiving the PRF output.
    /// Generates the master seed, creates the first account, and goes straight to Unlocked.
    fn finish_wallet_creation(
        &mut self,
        variant: SpxVariant,
        credential_id: &[u8],
        prf_output: &key_vault_core::SecureVec,
    ) {
        let key = match key_vault_core::utilities::derive_key_from_prf(prf_output) {
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
        let key = match key_vault_core::utilities::derive_key_from_prf(prf_output) {
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
                self.status = Status::Info("Wallet created successfully.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to read accounts: {}", e));
                self.screen = Screen::Locked;
            }
        }
    }

    /// Complete wallet unlock after credential assertion succeeds.
    /// Reads all account lock args from accounts.json (no decryption needed).
    fn finish_unlock(&mut self) {
        match KeyVault::get_all_sphincs_lock_args() {
            Ok(lock_args) => {
                self.accounts = lock_args;
                self.screen = Screen::Unlocked;
                self.status = Status::None;
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to unlock: {}", e));
            }
        }
    }

    /// Complete new account creation after receiving the PRF output.
    fn finish_create_new_account(&mut self, prf_output: &key_vault_core::SecureVec) {
        let key = match key_vault_core::utilities::derive_key_from_prf(prf_output) {
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
                self.accounts.push(lock_args);
                self.status = Status::Info("New account created.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Poll pending passkey operations each frame.
        #[cfg(target_os = "macos")]
        self.poll_pending();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            match self.screen.clone() {
                Screen::Setup => self.show_setup(ui, frame),
                Screen::Locked => self.show_locked(ui, frame),
                Screen::Unlocked => self.show_unlocked(ui, frame),
            }
        });

        // Request repaint while an operation is pending so we poll promptly.
        #[cfg(target_os = "macos")]
        if self.pending_op.is_some() {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 480.0])
            .with_min_inner_size([360.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "qpkv",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
