//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

#[cfg(target_os = "macos")]
mod window_handle;

use eframe::egui;
use key_vault_core::types::{AuthMethod, SpxVariant};
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

struct App {
    screen: Screen,
    status: Status,

    // Setup screen state.
    selected_variant: SpxVariant,
    seed_phrase_display: Option<String>,

    // Unlocked screen state.
    address: Option<String>,
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
            seed_phrase_display: None,
            address: None,
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
                    SpxVariant::Shake128S,
                    SpxVariant::Sha2128F,
                    SpxVariant::Shake128F,
                ] {
                    ui.selectable_value(
                        &mut self.selected_variant,
                        *variant,
                        format!("{}", variant),
                    );
                }
            });

        ui.add_space(12.0);

        if ui.button("Register Passkey & Create Wallet").clicked() {
            self.create_wallet_with_passkey(frame);
        }

        if let Some(ref phrase) = self.seed_phrase_display {
            ui.add_space(16.0);
            ui.separator();
            ui.heading("Backup Your Seed Phrase");
            ui.label("Write down these words and store them safely. You will NOT see them again.");
            ui.add_space(8.0);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut phrase.as_str())
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
            });

            ui.add_space(8.0);
            if ui.button("I have saved my seed phrase").clicked() {
                self.seed_phrase_display = None;
                self.screen = Screen::Locked;
                self.status = Status::Info("Wallet created. Touch ID to unlock.".to_string());
            }
        }

        self.show_status(ui);
    }

    fn show_locked(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("Wallet Locked");
        ui.add_space(12.0);

        if ui.button("Unlock with Touch ID").clicked() {
            self.unlock_with_passkey(frame);
        }

        self.show_status(ui);
    }

    fn show_unlocked(&mut self, ui: &mut egui::Ui) {
        ui.heading("Wallet Unlocked");
        ui.add_space(8.0);

        if let Some(ref addr) = self.address {
            ui.label("CKB Lock Args:");
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut addr.as_str())
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
            });
        }

        ui.add_space(12.0);
        if ui.button("Lock Wallet").clicked() {
            self.address = None;
            self.screen = Screen::Locked;
            self.status = Status::None;
        }

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

    fn create_wallet_with_passkey(&mut self, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let window = match Self::get_ns_window(frame) {
                Ok(w) => w,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to get window: {}", e));
                    return;
                }
            };

            // Step 1: Register a passkey.
            let rp_id = "quantumpurse.org";
            let user_id = b"qpkv-user";
            let user_name = "Key Vault User";

            let registration =
                match passkey_prf::register_passkey(&window, rp_id, user_id, user_name) {
                    Ok(r) => r,
                    Err(e) => {
                        self.status = Status::Error(format!("Passkey registration failed: {}", e));
                        return;
                    }
                };

            if !registration.prf_supported {
                self.status = Status::Error("PRF not supported by this authenticator.".to_string());
                return;
            }

            // Step 2: Assert PRF to get the encryption key.
            let salt = b"quantumpurse-kv-seed-encryption\0";
            let prf_output =
                match passkey_prf::assert_prf(&window, rp_id, &registration.credential_id, salt) {
                    Ok(o) => o,
                    Err(e) => {
                        self.status = Status::Error(format!("PRF assertion failed: {}", e));
                        return;
                    }
                };

            let key = match key_vault_core::Util::derive_key_from_prf(&prf_output) {
                Ok(k) => k,
                Err(e) => {
                    self.status = Status::Error(format!("Key derivation failed: {}", e));
                    return;
                }
            };

            // Step 3: Generate wallet seed with the PRF-derived key.
            let vault = KeyVault::new(self.selected_variant);
            let auth = AuthMethod::Prf {
                credential_id: registration.credential_id,
            };
            match vault.generate_master_seed_with_key(self.selected_variant, &key, auth) {
                Ok(phrase) => {
                    self.seed_phrase_display = Some(phrase);
                    self.status = Status::Info("Wallet created with Touch ID.".to_string());
                }
                Err(e) => {
                    self.status = Status::Error(format!("Failed to create wallet: {}", e));
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = frame;
            self.status = Status::Error("Passkey PRF is only supported on macOS.".to_string());
        }
    }

    fn unlock_with_passkey(&mut self, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let window = match Self::get_ns_window(frame) {
                Ok(w) => w,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to get window: {}", e));
                    return;
                }
            };

            // Read wallet info to get the credential_id and variant.
            // The variant used here is arbitrary — wallet_exists / read_wallet_info
            // do not depend on the variant stored in the KeyVault struct.
            let temp_vault = KeyVault::new(SpxVariant::Sha2128S);
            let wallet_info = match temp_vault.read_wallet_info() {
                Ok(info) => info,
                Err(e) => {
                    self.status = Status::Error(format!("Failed to read wallet info: {}", e));
                    return;
                }
            };

            let credential_id = match &wallet_info.auth_method {
                AuthMethod::Prf { credential_id } => credential_id.clone(),
                AuthMethod::Password => {
                    self.status =
                        Status::Error("This wallet uses password auth, not Touch ID.".to_string());
                    return;
                }
            };

            // Assert PRF to derive the encryption key.
            let rp_id = "quantumpurse.org";
            let salt = b"quantumpurse-kv-seed-encryption\0";
            let prf_output = match passkey_prf::assert_prf(&window, rp_id, &credential_id, salt) {
                Ok(o) => o,
                Err(passkey_prf::PrfError::Cancelled) => {
                    self.status = Status::Info("Cancelled.".to_string());
                    return;
                }
                Err(e) => {
                    self.status = Status::Error(format!("PRF assertion failed: {}", e));
                    return;
                }
            };

            let key = match key_vault_core::Util::derive_key_from_prf(&prf_output) {
                Ok(k) => k,
                Err(e) => {
                    self.status = Status::Error(format!("Key derivation failed: {}", e));
                    return;
                }
            };

            // Decrypt and derive the CKB lock args using the correct variant.
            let vault = KeyVault::new(wallet_info.spx_variant);
            match vault.get_address_with_key(&key) {
                Ok(addr) => {
                    self.address = Some(addr);
                    self.screen = Screen::Unlocked;
                    self.status = Status::None;
                }
                Err(e) => {
                    self.status = Status::Error(format!("Failed to unlock: {}", e));
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = frame;
            self.status = Status::Error("Passkey PRF is only supported on macOS.".to_string());
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            match self.screen.clone() {
                Screen::Setup => self.show_setup(ui, frame),
                Screen::Locked => self.show_locked(ui, frame),
                Screen::Unlocked => self.show_unlocked(ui),
            }
        });
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 480.0])
            .with_min_inner_size([360.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Key Vault",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
