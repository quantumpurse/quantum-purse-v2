//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

#[cfg(target_os = "macos")]
mod window_handle;

use eframe::egui;
use node_manager::{CkbRpc, NodeConfig, NodeType};
use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;
use std::collections::HashMap;
use std::sync::mpsc;

/// Result of a single account balance fetch from a background thread.
type BalanceResult = (String, Result<u64, String>);

/// Sidebar navigation tabs (only visible when unlocked).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Accounts,
    Settings,
}

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

/// CKB uses 8 decimal places: 1 CKB = 100,000,000 shannons.
const CKB_DECIMAL_PLACES: u64 = 100_000_000;

struct App {
    screen: Screen,
    status: Status,

    // Setup screen state.
    selected_variant: SpxVariant,

    // Unlocked screen state.
    active_tab: Tab,
    accounts: Vec<String>,
    confirm_remove: bool,

    // Balance cache: lock_args -> balance in shannons (None = not yet fetched).
    balances: HashMap<String, Option<u64>>,

    // Node configuration and RPC connection.
    node_config: NodeConfig,
    rpc_client: Option<Box<dyn CkbRpc>>,

    // Editable settings fields (buffered until saved).
    settings_rpc_url: String,
    settings_binary_path: String,
    settings_data_dir: String,

    // Receives balance results from background thread.
    // TODO: Consider migrating to tokio if the app needs more concurrent I/O
    // (e.g. transaction broadcasting, node health polling, WebSocket subscriptions).
    balance_receiver: Option<mpsc::Receiver<BalanceResult>>,

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

        let node_config = NodeConfig::load_or_default().unwrap_or_default();
        let settings_rpc_url = node_config.rpc_url.clone();
        let settings_binary_path = node_config
            .binary_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let settings_data_dir = node_config.data_dir.display().to_string();

        Self {
            screen,
            status: Status::None,
            selected_variant: SpxVariant::Sha2128S,
            active_tab: Tab::Accounts,
            accounts: Vec::new(),
            confirm_remove: false,
            balances: HashMap::new(),
            node_config,
            rpc_client: None,
            settings_rpc_url,
            settings_binary_path,
            settings_data_dir,
            balance_receiver: None,
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

    fn show_unlocked(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Left sidebar.
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(140.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("QPV2");
                ui.add_space(16.0);

                // Tab buttons.
                if ui
                    .selectable_label(self.active_tab == Tab::Accounts, "Accounts")
                    .clicked()
                {
                    self.active_tab = Tab::Accounts;
                }
                if ui
                    .selectable_label(self.active_tab == Tab::Settings, "Settings")
                    .clicked()
                {
                    self.active_tab = Tab::Settings;
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // Node status indicator.
                let node_label = match self.node_config.node_type {
                    NodeType::PublicRpc => "Public RPC",
                    NodeType::LightClient => "Light Client",
                    NodeType::FullNode => "Full Node",
                };
                let connected = self.rpc_client.is_some();
                let status_icon = if connected { "●" } else { "○" };
                let status_color = if connected {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::GRAY
                };
                ui.horizontal(|ui| {
                    ui.colored_label(status_color, status_icon);
                    ui.label(node_label);
                });

                ui.add_space(8.0);

                // Lock button at the bottom of the sidebar.
                if ui.button("Lock Wallet").clicked() {
                    self.lock_wallet();
                }
            });

        // Right content area.
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            match self.active_tab {
                Tab::Accounts => self.show_accounts_tab(ui, frame),
                Tab::Settings => self.show_settings_tab(ui),
            }
        });
    }

    fn show_accounts_tab(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("Accounts");
        ui.add_space(8.0);

        // Action buttons (above the list so they stay visible).
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

            if ui.button("Refresh Balances").clicked() {
                self.fetch_all_balances();
            }
        });

        self.show_status(ui);
        ui.add_space(8.0);

        // Account list with balances.
        if self.accounts.is_empty() {
            ui.label("No accounts yet.");
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, lock_args) in self.accounts.clone().iter().enumerate() {
                    let address_text = match qpv2_core::utilities::lock_args_to_address(
                        lock_args,
                        self.is_mainnet(),
                    ) {
                        Ok(addr) => addr,
                        Err(_) => format!("0x{}", lock_args),
                    };

                    let balance_text = match self.balances.get(lock_args) {
                        Some(Some(shannons)) => format_ckb_balance(*shannons),
                        Some(None) => "Loading...".to_string(),
                        None => "--".to_string(),
                    };

                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.strong(format!("Account #{}", i));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(&balance_text);
                                },
                            );
                        });
                        ui.add(
                            egui::TextEdit::singleline(&mut address_text.as_str())
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
                    ui.add_space(4.0);
                }
            });
        }
    }

    fn show_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(12.0);

        // ── Node Configuration ──
        ui.strong("Node Configuration");
        ui.add_space(4.0);

        // Node type dropdown.
        ui.horizontal(|ui| {
            ui.label("Node Type:");
            egui::ComboBox::from_id_salt("node_type")
                .selected_text(format!("{}", self.node_config.node_type))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(
                            &mut self.node_config.node_type,
                            NodeType::PublicRpc,
                            "Public RPC",
                        )
                        .changed()
                    {
                        self.on_node_type_changed();
                    }
                    if ui
                        .selectable_value(
                            &mut self.node_config.node_type,
                            NodeType::LightClient,
                            "Light Client",
                        )
                        .changed()
                    {
                        self.on_node_type_changed();
                    }
                    if ui
                        .selectable_value(
                            &mut self.node_config.node_type,
                            NodeType::FullNode,
                            "Full Node",
                        )
                        .changed()
                    {
                        self.on_node_type_changed();
                    }
                });
        });

        // Network dropdown.
        let prev_network = self.node_config.network;
        ui.horizontal(|ui| {
            ui.label("Network:");
            egui::ComboBox::from_id_salt("network")
                .selected_text(format!("{}", self.node_config.network))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.node_config.network,
                        node_manager::NetworkType::Testnet,
                        "Testnet",
                    );
                    ui.selectable_value(
                        &mut self.node_config.network,
                        node_manager::NetworkType::Mainnet,
                        "Mainnet",
                    );
                });
        });
        if self.node_config.network != prev_network {
            // Update RPC URL to default for new network when using public RPC.
            if self.node_config.node_type == NodeType::PublicRpc {
                let default_url = self.node_config.default_rpc_url().to_string();
                self.node_config.rpc_url = default_url.clone();
                self.settings_rpc_url = default_url;
            }
            // Reconnect and refresh balances for the new network.
            self.save_node_config();
        }

        ui.add_space(4.0);

        // RPC URL.
        ui.horizontal(|ui| {
            ui.label("RPC URL:");
            ui.add(egui::TextEdit::singleline(&mut self.settings_rpc_url).desired_width(300.0));
        });

        // Binary path (only relevant for local nodes).
        if self.node_config.requires_binary() {
            ui.horizontal(|ui| {
                ui.label("Binary Path:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_binary_path).desired_width(300.0),
                );
            });
        }

        // Data directory.
        ui.horizontal(|ui| {
            ui.label("Data Directory:");
            ui.add(egui::TextEdit::singleline(&mut self.settings_data_dir).desired_width(300.0));
        });

        ui.add_space(8.0);

        // Save and reconnect buttons.
        ui.horizontal(|ui| {
            if ui.button("Save & Reconnect").clicked() {
                self.save_node_config();
            }
        });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // ── Wallet Info ──
        ui.strong("Wallet Info");
        ui.add_space(4.0);

        let temp_vault = KeyVault::new(SpxVariant::Sha2128S);
        if let Ok(info) = temp_vault.read_wallet_info() {
            ui.label(format!("SPHINCS+ Variant: {}", info.spx_variant));
            ui.label(format!(
                "Auth Method: {}",
                match info.auth_method {
                    AuthMethod::PasskeyPrf { .. } => "Passkey PRF (Touch ID)",
                    AuthMethod::Password => "Password",
                }
            ));
            ui.label(format!("Accounts: {}", self.accounts.len()));
        }

        ui.add_space(12.0);

        // Remove wallet (with confirmation).
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
                        self.lock_wallet();
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

        self.show_status(ui);
    }

    // ── Shared helpers ──────────────────────────────────────────────────

    /// Whether the app is configured for CKB mainnet (derived from node config).
    fn is_mainnet(&self) -> bool {
        self.node_config.network == node_manager::NetworkType::Mainnet
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

    /// Lock the wallet: clear sensitive state and return to the Locked screen.
    fn lock_wallet(&mut self) {
        self.accounts.clear();
        self.balances.clear();
        self.confirm_remove = false;
        self.rpc_client = None;
        self.active_tab = Tab::Accounts;
        self.screen = Screen::Locked;
        self.status = Status::None;
    }

    /// Called when the node type dropdown changes in settings.
    fn on_node_type_changed(&mut self) {
        let default_url = self.node_config.default_rpc_url().to_string();
        self.node_config.rpc_url = default_url.clone();
        self.settings_rpc_url = default_url;
    }

    /// Apply settings edits, save config to disk, and reconnect the RPC client.
    fn save_node_config(&mut self) {
        self.node_config.rpc_url = self.settings_rpc_url.clone();

        if self.node_config.requires_binary() && !self.settings_binary_path.is_empty() {
            self.node_config.binary_path = Some(self.settings_binary_path.clone().into());
        } else if !self.node_config.requires_binary() {
            self.node_config.binary_path = None;
        }

        if !self.settings_data_dir.is_empty() {
            self.node_config.data_dir = self.settings_data_dir.clone().into();
        }

        if let Err(e) = self.node_config.save() {
            self.status = Status::Error(format!("Failed to save config: {}", e));
            return;
        }

        // Reconnect RPC client.
        self.rpc_client = Some(node_manager::connect(&self.node_config));
        self.status = Status::Info("Configuration saved. RPC reconnected.".to_string());

        // Refresh balances with new connection.
        self.fetch_all_balances();
    }

    /// Connect to the RPC endpoint and fetch balances for all accounts.
    fn connect_and_fetch_balances(&mut self) {
        self.rpc_client = Some(node_manager::connect(&self.node_config));
        self.fetch_all_balances();
    }

    /// Fetch balances for all accounts in a background thread.
    ///
    /// Spawns a thread that creates its own RPC client (CkbRpcClient is not
    /// Send) and sends each result back via `mpsc` channel. The UI thread
    /// polls `balance_receiver` every frame in `poll_balance_results()`.
    ///
    /// ```text
    /// UI thread                Background thread
    /// ─────────                ─────────────────
    /// mark all "Loading…"
    /// spawn thread ──────────► create RPC client
    /// keep rendering           ├─ fetch balance #0
    /// │                        ├─ send(result_0) ──►
    /// ├─ try_recv → update #0  ├─ fetch balance #1
    /// │                        ├─ send(result_1) ──►
    /// ├─ try_recv → update #1  └─ thread exits
    /// └─ try_recv → empty
    /// ```
    fn fetch_all_balances(&mut self) {
        if self.rpc_client.is_none() {
            return;
        }

        // Mark all accounts as loading.
        for lock_args in &self.accounts {
            self.balances.insert(lock_args.clone(), None);
        }

        let accounts = self.accounts.clone();
        if accounts.is_empty() {
            return;
        }

        let node_config = self.node_config.clone();
        let is_mainnet = self.is_mainnet();
        let (tx, rx) = mpsc::channel();
        self.balance_receiver = Some(rx);

        std::thread::spawn(move || {
            let rpc = node_manager::connect(&node_config);
            for lock_args in accounts {
                let result = fetch_account_balance(rpc.as_ref(), &lock_args, is_mainnet)
                    .map_err(|e| e.to_string());
                // If the receiver is dropped (e.g. wallet locked), stop.
                if tx.send((lock_args, result)).is_err() {
                    break;
                }
            }
        });
    }

    // ── Passkey flows ────────────────────────────────────────────────────

    /// Get the NSWindow handle, setting an error status on failure.
    #[cfg(target_os = "macos")]
    fn get_ns_window_or_err(
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
    /// Returns `None` (and sets an error status) if the wallet uses password auth
    /// or if the wallet info cannot be read.
    #[cfg(target_os = "macos")]
    fn get_credential_id(&mut self) -> Option<Vec<u8>> {
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
    fn start_registration(&mut self, frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let window = match self.get_ns_window_or_err(frame) {
                Some(w) => w,
                None => return,
            };

            let rp_id = "quantumpurse.org";
            let user_id = b"qpv2-user";
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
                self.status = Status::Info("Wallet created successfully.".to_string());
                self.connect_and_fetch_balances();
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
                self.connect_and_fetch_balances();
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to unlock: {}", e));
            }
        }
    }

    /// Complete new account creation after receiving the PRF output.
    fn finish_create_new_account(&mut self, prf_output: &qpv2_core::SecureVec) {
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
                    let is_mainnet = self.is_mainnet();
                    let args = lock_args.clone();
                    let (tx, rx) = mpsc::channel();
                    self.balance_receiver = Some(rx);

                    std::thread::spawn(move || {
                        let rpc = node_manager::connect(&node_config);
                        let result = fetch_account_balance(rpc.as_ref(), &args, is_mainnet)
                            .map_err(|e| e.to_string());
                        let _ = tx.send((args, result));
                    });
                }
                self.accounts.push(lock_args);
                self.status = Status::Info("New account created.".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }

    /// Drain available balance results from the background thread.
    /// Called every frame from `update()`.
    fn poll_balance_results(&mut self) {
        let rx = match &self.balance_receiver {
            Some(rx) => rx,
            None => return,
        };

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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Poll pending passkey operations each frame.
        #[cfg(target_os = "macos")]
        self.poll_pending();

        // Drain balance results from the background thread.
        self.poll_balance_results();

        match self.screen.clone() {
            Screen::Setup => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(8.0);
                    self.show_setup(ui, frame);
                });
            }
            Screen::Locked => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(8.0);
                    self.show_locked(ui, frame);
                });
            }
            Screen::Unlocked => {
                // Sidebar + content layout handled by show_unlocked.
                self.show_unlocked(ctx, frame);
            }
        }

        // Request repaint while an async operation is pending so we poll promptly.
        let balance_pending = self.balance_receiver.is_some();
        #[cfg(target_os = "macos")]
        let has_pending_op = self.pending_op.is_some();
        #[cfg(not(target_os = "macos"))]
        let has_pending_op = false;

        if has_pending_op || balance_pending {
            ctx.request_repaint();
        }
    }
}

/// Fetch the balance (in shannons) for a single account by its lock_args.
fn fetch_account_balance(
    rpc: &dyn CkbRpc,
    lock_args: &str,
    is_mainnet: bool,
) -> Result<u64, node_manager::NodeManagerError> {
    let (code_hash, hash_type) = if is_mainnet {
        (
            qpv2_core::constants::CKB_MAINNET_CODE_HASH,
            qpv2_core::constants::CKB_MAINNET_HASH_TYPE,
        )
    } else {
        (
            qpv2_core::constants::CKB_TESTNET_CODE_HASH,
            qpv2_core::constants::CKB_TESTNET_HASH_TYPE,
        )
    };

    node_manager::fetch_lock_script_balance(rpc, code_hash, hash_type, lock_args)
}

/// Format a balance in shannons to a human-readable CKB string.
/// 1 CKB = 100,000,000 shannons.
fn format_ckb_balance(shannons: u64) -> String {
    let whole = shannons / CKB_DECIMAL_PLACES;
    let frac = shannons % CKB_DECIMAL_PLACES;
    if frac == 0 {
        format!("{} CKB", whole)
    } else {
        // Show fractional part, trimming trailing zeros.
        let frac_str = format!("{:08}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{}.{} CKB", whole, trimmed)
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 520.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "qpv2",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
