//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

#[cfg(target_os = "macos")]
mod window_handle;

use eframe::egui;
use node_manager::{CkbRpc, NodeConfig, NodeType};
use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;
use std::collections::HashMap;
use std::sync::mpsc;

/// Computes the SPHINCS+ witness lock size for a given variant.
///
/// The lock field format is: `[4-byte config] + [1-byte flag] + [pubkey] + [signature]`.
fn spx_witness_lock_size(variant: SpxVariant) -> usize {
    let param_id: ckb_fips205_utils::ParamId = (variant as u8)
        .try_into()
        .expect("SpxVariant and ParamId use the same discriminants");
    let (pk_len, sig_len) = ckb_fips205_utils::verifying::lengths(param_id);
    5 + pk_len + sig_len
}

/// Result of a single account balance fetch from a background thread.
type BalanceResult = (String, Result<u64, String>);

/// Sidebar navigation tabs matching the mockup layout.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tab {
    Dashboard,
    Transfer,
    DaoOperations,
    Accounts,
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
    /// Waiting for PRF assertion to sign a transfer transaction.
    SignTransferAssert {
        pending: passkey_prf::AssertionRequest,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    },
}

/// Tracks the state of an in-progress transfer transaction.
#[derive(Debug, Clone)]
enum TransferStatus {
	/// No transfer in progress.
	Idle,
	/// Building the unsigned transaction.
	Building,
	/// Waiting for Touch ID to sign.
	AwaitingSignature,
	/// Sending the signed transaction.
	Sending,
	/// Transaction sent successfully.
	Success(String),
	/// An error occurred.
	Error(String),
}

/// CKB uses 8 decimal places: 1 CKB = 100,000,000 shannons.
const CKB_DECIMAL_PLACES: u64 = 100_000_000;

/// Custom color scheme matching the quantum aesthetic mockup.
struct AppColors {
    bg: egui::Color32,
    surface: egui::Color32,
    surface2: egui::Color32,
    border: egui::Color32,
    border2: egui::Color32,
    accent: egui::Color32,
    accent2: egui::Color32,
    accent3: egui::Color32,
    danger: egui::Color32,
    warn: egui::Color32,
    text: egui::Color32,
    text_muted: egui::Color32,
}

impl Default for AppColors {
    fn default() -> Self {
        Self {
            bg: egui::Color32::from_rgb(8, 12, 16),         // #080c10
            surface: egui::Color32::from_rgb(13, 19, 24),   // #0d1318
            surface2: egui::Color32::from_rgb(17, 25, 32),  // #111920
            border: egui::Color32::from_rgba_unmultiplied(0, 255, 180, 26),  // rgba(0,255,180,0.10)
            border2: egui::Color32::from_rgba_unmultiplied(0, 255, 180, 56), // rgba(0,255,180,0.22)
            accent: egui::Color32::from_rgb(0, 255, 180),   // #00ffb4
            accent2: egui::Color32::from_rgb(0, 200, 255),  // #00c8ff
            accent3: egui::Color32::from_rgb(155, 127, 212), // #9b7fd4
            danger: egui::Color32::from_rgb(255, 77, 109),  // #ff4d6d
            warn: egui::Color32::from_rgb(255, 209, 102),   // #ffd166
            text: egui::Color32::from_rgb(232, 244, 240),   // #e8f4f0
            text_muted: egui::Color32::from_rgb(90, 122, 112), // #5a7a70
        }
    }
}

struct App {
    screen: Screen,
    status: Status,
    colors: AppColors,

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
    balance_receiver: Option<mpsc::Receiver<BalanceResult>>,

    // In-flight passkey operation (macOS only).
    #[cfg(target_os = "macos")]
    pending_op: Option<PendingOp>,

    // Node selector popup state.
    node_selector_open: bool,
    node_selector_rect: Option<egui::Rect>,
    // Temporary values for node selector popup.
    temp_network: node_manager::NetworkType,
    temp_node_type: NodeType,

    // Transfer form state.
    transfer_recipient: String,
    transfer_amount: String,
    transfer_fee_rate: String,
    transfer_from_account: usize,
    transfer_status: TransferStatus,
    // Channel for receiving the built unsigned transaction from the background thread.
    transfer_build_rx: Option<
        mpsc::Receiver<
            Result<
                (
                    ckb_types::core::TransactionView,
                    Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
                    String, // lock_args for signing
                ),
                String,
            >,
        >,
    >,
    // Channel for receiving the final send result (tx hash or error).
    transfer_send_rx: Option<mpsc::Receiver<Result<String, String>>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {

        // Configure visuals for dark theme
        let mut visuals = egui::Visuals::dark();
        let colors = AppColors::default();

        visuals.override_text_color = Some(colors.text);
        visuals.panel_fill = colors.surface;
        visuals.window_fill = colors.surface2;
        visuals.faint_bg_color = colors.surface2;
        visuals.extreme_bg_color = colors.bg;
        visuals.widgets.noninteractive.bg_fill = colors.surface2;
        visuals.widgets.inactive.bg_fill = colors.surface2;
        visuals.widgets.hovered.bg_fill = colors.surface2;
        visuals.widgets.active.bg_fill = colors.surface2;
        visuals.widgets.open.bg_fill = colors.surface2;

        cc.egui_ctx.set_visuals(visuals);

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

        // Store temp values before moving node_config
        let temp_network = node_config.network;
        let temp_node_type = node_config.node_type;

        Self {
            screen,
            status: Status::None,
            colors,
            selected_variant: SpxVariant::Sha2128S,
            active_tab: Tab::Dashboard,
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
            node_selector_open: false,
            node_selector_rect: None,
            temp_network,
            temp_node_type,
            transfer_recipient: String::new(),
            transfer_amount: String::new(),
            transfer_fee_rate: "1000".to_string(),
            transfer_from_account: 0,
            transfer_status: TransferStatus::Idle,
            transfer_build_rx: None,
            transfer_send_rx: None,
        }
    }

    /// Extract the NSWindow from the eframe Frame (macOS only).
    #[cfg(target_os = "macos")]
    fn get_ns_window(
        frame: &eframe::Frame,
    ) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, String> {
        window_handle::get_ns_window(frame)
    }

    /// Draw a gradient background effect
    fn draw_gradient_bg(&self, ui: &mut egui::Ui) {
        let rect = ui.clip_rect();
        let painter = ui.painter();

        // Subtle gradient overlay
        painter.rect_filled(
            rect,
            0.0,
            self.colors.bg,
        );

        // Add subtle glow effects at corners
        let glow1_center = rect.left_top() + egui::vec2(rect.width() * 0.12, rect.height() * 0.18);
        let glow2_center = rect.right_bottom() - egui::vec2(rect.width() * 0.12, rect.height() * 0.22);

        // Draw gradient circles
        for i in (0..30).rev() {
            let alpha = (1.0 - (i as f32 / 30.0)).powi(2) * 0.05;
            let radius = rect.width().min(rect.height()) * 0.4 * (i as f32 / 30.0);

            painter.circle_filled(
                glow1_center,
                radius,
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, (alpha * 255.0) as u8),
            );

            painter.circle_filled(
                glow2_center,
                radius,
                egui::Color32::from_rgba_unmultiplied(0, 200, 255, (alpha * 255.0) as u8),
            );
        }
    }

    fn show_setup(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            // Logo
            ui.heading(
                egui::RichText::new("🔮 Quantum Purse")
                    .size(32.0)
                    .color(self.colors.accent)
                    .strong(),
            );

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Post-Quantum Secure Wallet")
                    .size(14.0)
                    .color(self.colors.text_muted),
            );

            ui.add_space(40.0);

            // Setup card
            egui::Frame::new()
                .fill(self.colors.surface2)
                .corner_radius(16.0)
                .inner_margin(32.0)
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(ui, |ui| {
                    ui.set_max_width(400.0);

                    ui.label(
                        egui::RichText::new("Create New Wallet")
                            .size(20.0)
                            .strong(),
                    );

                    ui.add_space(24.0);

                    ui.label("Select SPHINCS+ variant:");
                    ui.add_space(8.0);

                    egui::ComboBox::from_id_salt("variant")
                        .selected_text(format!("{}", self.selected_variant))
                        .width(350.0)
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

                    ui.add_space(32.0);

                    #[cfg(target_os = "macos")]
                    let is_busy = self.pending_op.is_some();
                    #[cfg(not(target_os = "macos"))]
                    let is_busy = false;

                    let button = egui::Button::new(
                        egui::RichText::new(if is_busy {
                            "Creating wallet..."
                        } else {
                            "Create with Touch ID"
                        })
                        .size(16.0)
                    )
                    .fill(self.colors.accent)
                    .min_size(egui::vec2(350.0, 48.0));

                    if ui.add_enabled(!is_busy, button).clicked() {
                        self.start_registration(frame);
                    }
                });

            ui.add_space(24.0);
            self.show_status(ui);
        });
    }

    fn show_locked(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);

            // Lock icon
            ui.label(
                egui::RichText::new("🔒")
                    .size(64.0),
            );

            ui.add_space(24.0);

            ui.heading(
                egui::RichText::new("Wallet Locked")
                    .size(28.0)
                    .color(self.colors.text),
            );

            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("Authenticate to access your wallet")
                    .color(self.colors.text_muted),
            );

            ui.add_space(40.0);

            #[cfg(target_os = "macos")]
            let is_busy = self.pending_op.is_some();
            #[cfg(not(target_os = "macos"))]
            let is_busy = false;

            let button = egui::Button::new(
                egui::RichText::new(if is_busy {
                    "Waiting for Touch ID..."
                } else {
                    "Unlock with Touch ID"
                })
                .size(16.0)
            )
            .fill(self.colors.accent2)
            .min_size(egui::vec2(280.0, 48.0));

            if ui.add_enabled(!is_busy, button).clicked() {
                self.start_unlock(frame);
            }

            ui.add_space(24.0);
            self.show_status(ui);
        });
    }

    fn show_unlocked(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Left sidebar matching the mockup layout.
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(236.0)
            .frame(
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border)),
            )
            .show(ctx, |ui| {
                // ── Logo section ──
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);

                    // Logo icon with gradient-like background
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        icon_rect,
                        10.0,
                        self.colors.accent,
                    );
                    ui.painter().text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Q",
                        egui::FontId::proportional(17.0),
                        egui::Color32::from_rgb(5, 12, 10),
                    );

                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("QPV2")
                                .size(21.0)
                                .color(self.colors.accent)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new("NERVOS NETWORK")
                                .size(8.0)
                                .color(self.colors.text_muted),
                        );
                    });
                });

                ui.add_space(6.0);
                // Divider below logo
                ui.horizontal(|ui| {
                    let rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [rect.left_top(), egui::pos2(rect.right(), rect.top())],
                        egui::Stroke::new(1.0, self.colors.border),
                    );
                    ui.add_space(1.0);
                });

                // ── Node selector ──
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);

                    // Make it interactive
                    let (response, painter) = ui.allocate_painter(
                        egui::vec2(208.0, 52.0),
                        egui::Sense::click(),
                    );

                    let rect = response.rect;
                    let is_hovered = response.hovered();

                    // Store rect for dropdown positioning
                    self.node_selector_rect = Some(rect);

                    // Draw background
                    let bg_color = if is_hovered {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10)
                    } else {
                        self.colors.surface2
                    };

                    painter.rect_filled(rect, 9.0, bg_color);
                    painter.rect_stroke(rect, 9.0, egui::Stroke::new(1.0, self.colors.border), egui::StrokeKind::Outside);

                    // Content layout
                    let inner = rect.shrink2(egui::vec2(12.0, 9.0));

                    // "ACTIVE NODE" label
                    painter.text(
                        inner.left_top() + egui::vec2(0.0, 0.0),
                        egui::Align2::LEFT_TOP,
                        "ACTIVE NODE",
                        egui::FontId::proportional(8.0),
                        self.colors.text_muted,
                    );

                    // Node info row
                    let row_y = inner.top() + 14.0;

                    // Connection dot
                    let connected = self.rpc_client.is_some();
                    let dot_color = if connected {
                        self.colors.accent
                    } else {
                        self.colors.text_muted
                    };
                    painter.circle_filled(
                        egui::pos2(inner.left() + 4.0, row_y + 7.0),
                        3.0,
                        dot_color,
                    );

                    // Node type
                    let node_name = match self.node_config.node_type {
                        NodeType::PublicRpc => "Public RPC",
                        NodeType::LightClient => "Light Client",
                        NodeType::FullNode => "Full Node",
                    };
                    painter.text(
                        egui::pos2(inner.left() + 14.0, row_y),
                        egui::Align2::LEFT_TOP,
                        node_name,
                        egui::FontId::proportional(13.0),
                        self.colors.text,
                    );

                    // Dropdown arrow
                    painter.text(
                        egui::pos2(inner.right() - 28.0, row_y),
                        egui::Align2::LEFT_TOP,
                        "▼",
                        egui::FontId::proportional(9.0),
                        self.colors.text_muted,
                    );

                    // Network badge
                    let network = match self.node_config.network {
                        node_manager::NetworkType::Mainnet => "MAIN",
                        node_manager::NetworkType::Testnet => "TEST",
                    };
                    let network_color = if self.node_config.network == node_manager::NetworkType::Mainnet {
                        self.colors.accent
                    } else {
                        self.colors.warn
                    };
                    painter.text(
                        egui::pos2(inner.right() - 5.0, row_y),
                        egui::Align2::RIGHT_TOP,
                        network,
                        egui::FontId::proportional(8.0),
                        network_color,
                    );

                    // Handle click
                    if response.clicked() {
                        self.node_selector_open = !self.node_selector_open;
                        // Update temp values when opening
                        if self.node_selector_open {
                            self.temp_network = self.node_config.network;
                            self.temp_node_type = self.node_config.node_type;
                        }
                    }

                    // Change cursor on hover
                    if is_hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });

                // ── Navigation ──
                ui.add_space(14.0);

                // Section: Wallet
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("WALLET")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::Dashboard, "◈", "Dashboard");
                self.draw_nav_item(ui, Tab::Transfer, "↑", "Transfer");

                ui.add_space(10.0);

                // Section: NervosDAO
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("NERVOSDAO")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::DaoOperations, "⬡", "DAO Operations");

                ui.add_space(10.0);

                // Section: Security
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("SECURITY")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::Accounts, "◎", "Accounts");

                // ── Bottom: account card + lock ──
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(10.0);

                    // Account card
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        egui::Frame::new()
                            .fill(self.colors.surface2)
                            .corner_radius(9.0)
                            .inner_margin(egui::Margin::symmetric(13, 10))
                            .stroke(egui::Stroke::new(1.0, self.colors.border))
                            .show(ui, |ui| {
                                ui.set_width(194.0);

                                if let Some(lock_args) = self.accounts.first() {
                                    ui.label(
                                        egui::RichText::new("Account #0")
                                            .size(12.0),
                                    );

                                    let addr_short = match qpv2_core::utilities::lock_args_to_address(
                                        lock_args,
                                        self.is_mainnet(),
                                    ) {
                                        Ok(addr) => {
                                            if addr.len() > 20 {
                                                format!(
                                                    "{}...{}",
                                                    &addr[..10],
                                                    &addr[addr.len() - 5..]
                                                )
                                            } else {
                                                addr
                                            }
                                        }
                                        Err(_) => format!("0x{}...", &lock_args[..8.min(lock_args.len())]),
                                    };
                                    ui.label(
                                        egui::RichText::new(addr_short)
                                            .size(9.0)
                                            .color(self.colors.text_muted)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new("No accounts")
                                            .size(12.0)
                                            .color(self.colors.text_muted),
                                    );
                                }
                            });
                    });

                    ui.add_space(4.0);

                    // Divider above account area
                    ui.horizontal(|ui| {
                        let rect = ui.available_rect_before_wrap();
                        ui.painter().line_segment(
                            [rect.left_top(), egui::pos2(rect.right(), rect.top())],
                            egui::Stroke::new(1.0, self.colors.border),
                        );
                        ui.add_space(1.0);
                    });
                });
            });

        // ── Main content area ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.colors.bg))
            .show(ctx, |ui| {
                self.draw_gradient_bg(ui);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(24.0);

                    // Content padding
                    ui.horizontal(|ui| {
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width() - 60.0);

                            match self.active_tab {
                                Tab::Dashboard => self.show_dashboard_tab(ui, frame),
                                Tab::Transfer => self.show_transfer_tab(ui),
                                Tab::DaoOperations => self.show_dao_tab(ui),
                                Tab::Accounts => self.show_accounts_tab(ui, frame),
                            }
                        });
                    });
                });
            });
    }

    fn draw_nav_item(&mut self, ui: &mut egui::Ui, tab: Tab, icon: &str, label: &str) {
        let is_active = self.active_tab == tab;

        let response = ui.allocate_response(
            egui::vec2(ui.available_width(), 36.0),
            egui::Sense::click(),
        );

        if response.clicked() {
            self.active_tab = tab;
        }

        let rect = response.rect;
        let painter = ui.painter();

        // Inset rect for rounded background (matching mockup .nav-item padding)
        let inner = egui::Rect::from_min_size(
            rect.min + egui::vec2(10.0, 0.0),
            egui::vec2(rect.width() - 20.0, rect.height()),
        );

        if is_active {
            painter.rect_filled(
                inner,
                9.0,
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, 26),
            );
        } else if response.hovered() {
            painter.rect_filled(
                inner,
                9.0,
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, 15),
            );
        }

        let text_color = if is_active {
            self.colors.accent
        } else if response.hovered() {
            self.colors.text
        } else {
            self.colors.text_muted
        };

        // Icon
        painter.text(
            inner.left_center() + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            icon,
            egui::FontId::proportional(15.0),
            text_color,
        );

        // Label
        painter.text(
            inner.left_center() + egui::vec2(34.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            text_color,
        );
    }

    // ── Dashboard tab ──────────────────────────────────────────────────

    fn show_dashboard_tab(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Top bar: title + subtitle
        ui.heading(
            egui::RichText::new("Dashboard")
                .size(26.0)
                .strong(),
        );
        ui.label(
            egui::RichText::new("Portfolio overview & activity")
                .size(13.0)
                .color(self.colors.text_muted),
        );

        ui.add_space(22.0);

        // ── Balance hero card ──
        egui::Frame::new()
            .fill(egui::Color32::from_rgba_unmultiplied(0, 255, 180, 8))
            .corner_radius(20.0)
            .inner_margin(egui::Margin::symmetric(34, 30))
            .stroke(egui::Stroke::new(1.0, self.colors.border2))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("TOTAL BALANCE")
                        .size(10.0)
                        .color(self.colors.text_muted)
                        .family(egui::FontFamily::Monospace),
                );
                ui.add_space(6.0);

                // Sum all balances
                let total_shannons: u64 = self
                    .balances
                    .values()
                    .filter_map(|b| b.as_ref().copied())
                    .sum();

                ui.label(
                    egui::RichText::new(format_ckb_balance(total_shannons))
                        .size(42.0)
                        .strong()
                        .color(self.colors.text),
                );

                ui.add_space(16.0);

                // Meta row
                ui.horizontal(|ui| {
                    let rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [rect.left_top(), egui::pos2(rect.right(), rect.top())],
                        egui::Stroke::new(1.0, self.colors.border),
                    );
                });
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    // Available
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("AVAILABLE")
                                .size(9.0)
                                .color(self.colors.text_muted)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(format_ckb_balance(total_shannons))
                                .size(15.0)
                                .strong()
                                .color(self.colors.accent),
                        );
                    });

                    ui.add_space(30.0);

                    // Accounts
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("ACCOUNTS")
                                .size(9.0)
                                .color(self.colors.text_muted)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(format!("{}", self.accounts.len()))
                                .size(15.0)
                                .strong()
                                .color(self.colors.accent2),
                        );
                    });

                    ui.add_space(30.0);

                    // DAO Locked (placeholder)
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("DAO LOCKED")
                                .size(9.0)
                                .color(self.colors.text_muted)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new("0 CKB")
                                .size(15.0)
                                .strong()
                                .color(self.colors.accent3),
                        );
                    });
                });
            });

        ui.add_space(16.0);

        // ── Quick actions ──
        ui.columns(4, |cols| {
            let actions = [
                ("↑", "Send", Tab::Transfer),
                ("↓", "Receive", Tab::Accounts),
                ("⬡", "DAO", Tab::DaoOperations),
                ("◎", "Accounts", Tab::Accounts),
            ];

            for (i, (icon, label, target_tab)) in actions.iter().enumerate() {
                let response = egui::Frame::new()
                    .fill(self.colors.surface)
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::symmetric(10, 16))
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .show(&mut cols[i], |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(*icon)
                                    .size(20.0)
                                    .color(self.colors.text_muted),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(*label)
                                    .size(12.0)
                                    .color(self.colors.text_muted),
                            );
                        });
                    })
                    .response;

                if response.interact(egui::Sense::click()).clicked() {
                    self.active_tab = *target_tab;
                }
            }
        });

        ui.add_space(20.0);

        // ── Status messages ──
        self.show_status(ui);

        ui.add_space(16.0);

        // ── Lock wallet button ──
        let lock_btn = egui::Button::new(
            egui::RichText::new("🔒 Lock Wallet")
                .size(12.0),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, self.colors.border));

        if ui.add(lock_btn).clicked() {
            self.lock_wallet();
        }

        let _ = frame; // Suppress unused warning.
    }

    // ── Transfer tab ───────────────────────────────────────────────────

    fn show_transfer_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("Transfer CKB")
                .size(26.0)
                .strong()
                .color(self.colors.text),
        );
        ui.label(
            egui::RichText::new("Send CKB to any Nervos address.")
                .size(13.0)
                .color(self.colors.text_muted),
        );

        ui.add_space(22.0);

        // Show success/error status from previous transfer
        match &self.transfer_status {
            TransferStatus::Success(tx_hash) => {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(0, 255, 136, 20))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.label(
                            egui::RichText::new("Transaction sent successfully!")
                                .strong()
                                .color(self.colors.accent),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Tx: 0x{}..{}", &tx_hash[..8], &tx_hash[tx_hash.len()-8..]))
                                    .size(11.0)
                                    .color(self.colors.text_muted)
                                    .family(egui::FontFamily::Monospace),
                            );
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(format!("0x{}", tx_hash));
                            }
                        });
                    });
                ui.add_space(12.0);
            }
            TransferStatus::Error(msg) => {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(255, 70, 70, 20))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.label(
                            egui::RichText::new(format!("Error: {}", msg))
                                .color(self.colors.danger),
                        );
                    });
                ui.add_space(12.0);
            }
            _ => {}
        }

        egui::Frame::new()
            .fill(self.colors.surface)
            .corner_radius(20.0)
            .inner_margin(egui::Margin::symmetric(30, 26))
            .stroke(egui::Stroke::new(1.0, self.colors.border))
            .show(ui, |ui| {
                ui.set_max_width(560.0);

                let is_busy = !matches!(
                    self.transfer_status,
                    TransferStatus::Idle | TransferStatus::Success(_) | TransferStatus::Error(_)
                );

                // ── From Account ──
                ui.label(
                    egui::RichText::new("From")
                        .size(12.0)
                        .color(self.colors.text_muted),
                );
                ui.add_space(4.0);

                let from_text = if self.accounts.is_empty() {
                    "No accounts available".to_string()
                } else {
                    let idx = self.transfer_from_account.min(self.accounts.len() - 1);
                    let lock_args = &self.accounts[idx];
                    let bal = self
                        .balances
                        .get(lock_args)
                        .and_then(|b| b.as_ref())
                        .copied();
                    let bal_str = match bal {
                        Some(b) => format_ckb_balance(b),
                        None => "--".to_string(),
                    };
                    format!("Account #{} ({})", idx, bal_str)
                };

                egui::ComboBox::from_id_salt("transfer_from")
                    .selected_text(&from_text)
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for (i, lock_args) in self.accounts.iter().enumerate() {
                            let bal = self
                                .balances
                                .get(lock_args)
                                .and_then(|b| b.as_ref())
                                .copied();
                            let label = match bal {
                                Some(b) => format!("Account #{} ({})", i, format_ckb_balance(b)),
                                None => format!("Account #{}", i),
                            };
                            ui.selectable_value(
                                &mut self.transfer_from_account,
                                i,
                                label,
                            );
                        }
                    });

                ui.add_space(16.0);

                // ── Recipient Address ──
                ui.label(
                    egui::RichText::new("To")
                        .size(12.0)
                        .color(self.colors.text_muted),
                );
                ui.add_space(4.0);

                let recipient_edit = egui::TextEdit::singleline(&mut self.transfer_recipient)
                    .hint_text("ckt1q... or ckb1q...")
                    .desired_width(ui.available_width())
                    .font(egui::FontId::monospace(13.0))
                    .interactive(!is_busy);
                ui.add(recipient_edit);

                ui.add_space(16.0);

                // ── Amount ──
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Amount (CKB)")
                            .size(12.0)
                            .color(self.colors.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !is_busy && !self.accounts.is_empty() {
                            if ui.small_button("MAX").clicked() {
                                let idx = self
                                    .transfer_from_account
                                    .min(self.accounts.len() - 1);
                                let lock_args = &self.accounts[idx];
                                if let Some(Some(bal)) = self.balances.get(lock_args) {
                                    // Leave 1 CKB for fee estimation
                                    let max = bal.saturating_sub(CKB_DECIMAL_PLACES);
                                    let whole = max / CKB_DECIMAL_PLACES;
                                    let frac = max % CKB_DECIMAL_PLACES;
                                    if frac == 0 {
                                        self.transfer_amount = format!("{}", whole);
                                    } else {
                                        let frac_str = format!("{:08}", frac);
                                        let trimmed = frac_str.trim_end_matches('0');
                                        self.transfer_amount =
                                            format!("{}.{}", whole, trimmed);
                                    }
                                }
                            }
                        }
                    });
                });
                ui.add_space(4.0);

                let amount_edit = egui::TextEdit::singleline(&mut self.transfer_amount)
                    .hint_text("0.0")
                    .desired_width(ui.available_width())
                    .font(egui::FontId::monospace(13.0))
                    .interactive(!is_busy);
                ui.add(amount_edit);

                ui.add_space(16.0);

                // ── Fee Rate (collapsible) ──
                egui::CollapsingHeader::new(
                    egui::RichText::new("Advanced")
                        .size(12.0)
                        .color(self.colors.text_muted),
                )
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Fee rate (shannons/KB)")
                            .size(11.0)
                            .color(self.colors.text_muted),
                    );
                    ui.add_space(4.0);
                    let fee_edit =
                        egui::TextEdit::singleline(&mut self.transfer_fee_rate)
                            .hint_text("1000")
                            .desired_width(120.0)
                            .font(egui::FontId::monospace(12.0))
                            .interactive(!is_busy);
                    ui.add(fee_edit);
                });

                ui.add_space(20.0);

                // ── Send Button ──
                let connected = self.rpc_client.is_some();
                let has_accounts = !self.accounts.is_empty();
                let can_send = connected
                    && has_accounts
                    && !is_busy
                    && !self.transfer_recipient.is_empty()
                    && !self.transfer_amount.is_empty();

                let btn_text = match &self.transfer_status {
                    TransferStatus::Building => "Building transaction...",
                    TransferStatus::AwaitingSignature => "Waiting for Touch ID...",
                    TransferStatus::Sending => "Sending...",
                    _ => "Send",
                };

                let send_btn = egui::Button::new(
                    egui::RichText::new(btn_text).size(15.0).strong(),
                )
                .fill(if can_send {
                    self.colors.accent
                } else {
                    self.colors.surface2
                })
                .min_size(egui::vec2(ui.available_width(), 44.0));

                if ui.add_enabled(can_send, send_btn).clicked() {
                    self.start_transfer();
                }

                if !connected {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Not connected to node.")
                            .size(11.0)
                            .color(self.colors.warn),
                    );
                }
            });
    }

    // ── DAO Operations tab (placeholder) ────────────────────────────────

    fn show_dao_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("NervosDAO")
                .size(26.0)
                .strong(),
        );
        ui.label(
            egui::RichText::new("Deposit, withdraw, and manage DAO positions")
                .size(13.0)
                .color(self.colors.text_muted),
        );

        ui.add_space(22.0);

        // DAO action cards (3 columns)
        ui.columns(3, |cols| {
            let dao_items = [
                ("📥", "DAO Deposit", "Lock CKB to earn compensation against secondary issuance inflation.", 0),
                ("⏳", "Request Withdrawal", "Begin the unlock process. Wait for an epoch boundary to complete.", 1),
                ("📤", "Withdraw", "Claim CKB + compensation after the epoch boundary is reached.", 2),
            ];

            for (icon, title, desc, i) in &dao_items {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(20, 22))
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .show(&mut cols[*i], |ui| {
                        ui.label(
                            egui::RichText::new(*icon)
                                .size(26.0),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(*title)
                                .size(14.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(*desc)
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });
            }
        });

        ui.add_space(22.0);

        ui.label(
            egui::RichText::new("Coming soon")
                .size(14.0)
                .color(self.colors.warn),
        );
        ui.label(
            egui::RichText::new(
                "DAO operations will be available in the next update. \
                Transaction builders are already implemented in the node-manager crate.",
            )
            .size(12.0)
            .color(self.colors.text_muted),
        );
    }

    // ── Accounts tab ────────────────────────────────────────────────────

    fn show_accounts_tab(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading(
            egui::RichText::new("Accounts")
                .size(26.0)
                .strong(),
        );
        ui.label(
            egui::RichText::new("Manage wallets, keys, and authentication")
                .size(13.0)
                .color(self.colors.text_muted),
        );

        ui.add_space(22.0);

        // ── Action cards (3-column grid) ──
        ui.columns(3, |cols| {
            // New Account
            let new_card = egui::Frame::new()
                .fill(self.colors.surface)
                .corner_radius(18.0)
                .inner_margin(egui::Margin::symmetric(20, 24))
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(&mut cols[0], |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("✦").size(30.0));
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("New Account")
                                .size(14.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Derive a new SPHINCS+ account from your master seed.")
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });
                })
                .response;

            if new_card.interact(egui::Sense::click()).clicked() {
                self.start_create_new_account(frame);
            }

            // Import (CLI only)
            egui::Frame::new()
                .fill(self.colors.surface)
                .corner_radius(18.0)
                .inner_margin(egui::Margin::symmetric(20, 24))
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(&mut cols[1], |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("⬇").size(30.0));
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Import Seed")
                                .size(14.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("CLI only: qpv2 import-seed")
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });
                });

            // Export (CLI only)
            egui::Frame::new()
                .fill(self.colors.surface)
                .corner_radius(18.0)
                .inner_margin(egui::Margin::symmetric(20, 24))
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(&mut cols[2], |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("⬆").size(30.0));
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Export Seed")
                                .size(14.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("CLI only: qpv2 export-seed")
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });
                });
        });

        ui.add_space(18.0);

        // ── Section title ──
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Saved Accounts")
                    .size(17.0)
                    .strong()
                    .color(self.colors.text),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{} accounts", self.accounts.len()))
                    .size(9.0)
                    .color(self.colors.accent)
                    .family(egui::FontFamily::Monospace),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🔄 Refresh").clicked() {
                    self.fetch_all_balances();
                }
            });
        });

        ui.add_space(10.0);

        // ── Account list ──
        if self.accounts.is_empty() {
            ui.label(
                egui::RichText::new("No accounts yet. Create one to get started.")
                    .color(self.colors.text_muted),
            );
        } else {
            let avatar_colors = [
                (self.colors.accent, egui::Color32::from_rgb(5, 12, 10)),
                (self.colors.accent3, egui::Color32::WHITE),
                (self.colors.warn, egui::Color32::from_rgb(5, 12, 10)),
            ];

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

                let is_first = i == 0;
                let (av_bg, av_fg) = avatar_colors[i % avatar_colors.len()];

                egui::Frame::new()
                    .fill(self.colors.surface)
                    .corner_radius(9.0)
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .stroke(egui::Stroke::new(
                        1.0,
                        if is_first { self.colors.border2 } else { self.colors.border },
                    ))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Avatar circle
                            let (avatar_rect, _) = ui.allocate_exact_size(
                                egui::vec2(38.0, 38.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(
                                avatar_rect.center(),
                                19.0,
                                av_bg,
                            );
                            let letter = (b'A' + (i as u8 % 26)) as char;
                            ui.painter().text(
                                avatar_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                letter.to_string(),
                                egui::FontId::proportional(15.0),
                                av_fg,
                            );

                            ui.add_space(10.0);

                            // Info
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("Account #{}", i))
                                            .size(13.0),
                                    );
                                    if is_first {
                                        ui.label(
                                            egui::RichText::new("ACTIVE")
                                                .size(9.0)
                                                .color(self.colors.accent)
                                                .family(egui::FontFamily::Monospace),
                                        );
                                    }
                                });
                                ui.label(
                                    egui::RichText::new(&address_text)
                                        .size(9.0)
                                        .color(self.colors.text_muted)
                                        .family(egui::FontFamily::Monospace),
                                );
                            });

                            // Balance + copy (right-aligned)
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("📋").on_hover_text("Copy address").clicked() {
                                        ui.ctx().copy_text(address_text.clone());
                                        self.status =
                                            Status::Info("Address copied!".to_string());
                                    }

                                    ui.add_space(8.0);

                                    ui.vertical(|ui| {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(&balance_text)
                                                        .size(15.0)
                                                        .strong()
                                                        .color(self.colors.text)
                                                        .family(egui::FontFamily::Monospace),
                                                );
                                            },
                                        );
                                    });
                                },
                            );
                        });
                    });

                ui.add_space(6.0);
            }
        }

        ui.add_space(16.0);

        // ── Wallet management ──
        ui.horizontal(|ui| {
            // Node settings
            let settings_btn = egui::Button::new("⚙ Node Settings")
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.0, self.colors.border));

            if ui.add(settings_btn).clicked() {
                // Inline the settings into a simple save action for now.
                self.save_node_config();
            }

            ui.add_space(8.0);

            // Wallet info
            if let Ok(info) = KeyVault::new(SpxVariant::Sha2128S).read_wallet_info() {
                ui.label(
                    egui::RichText::new(format!("SPHINCS+ {}", info.spx_variant))
                        .size(10.0)
                        .color(self.colors.text_muted)
                        .family(egui::FontFamily::Monospace),
                );
                ui.label(
                    egui::RichText::new(match info.auth_method {
                        AuthMethod::PasskeyPrf { .. } => "Touch ID",
                        AuthMethod::Password => "Password",
                    })
                    .size(10.0)
                    .color(self.colors.accent2)
                    .family(egui::FontFamily::Monospace),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Remove wallet
                let remove_label = if self.confirm_remove {
                    "⚠ Confirm Remove?"
                } else {
                    "🗑 Remove Wallet"
                };

                let remove_btn = egui::Button::new(
                    egui::RichText::new(remove_label)
                        .size(11.0)
                        .color(self.colors.danger),
                )
                .fill(egui::Color32::from_rgba_unmultiplied(255, 77, 109, 25))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(255, 77, 109, 77),
                ));

                if ui.add(remove_btn).clicked() {
                    if self.confirm_remove {
                        match KeyVault::clear_database() {
                            Ok(()) => {
                                self.lock_wallet();
                                self.screen = Screen::Setup;
                                self.status =
                                    Status::Info("Wallet removed successfully.".to_string());
                            }
                            Err(e) => {
                                self.status = Status::Error(format!(
                                    "Failed to remove wallet: {}",
                                    e
                                ));
                            }
                        }
                    } else {
                        self.confirm_remove = true;
                    }
                }
            });
        });

        ui.add_space(10.0);
        self.show_status(ui);
    }

    // ── Shared helpers ──────────────────────────────────────────────────

    /// Whether the app is configured for CKB mainnet (derived from node config).
    fn is_mainnet(&self) -> bool {
        self.node_config.network == node_manager::NetworkType::Mainnet
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        match &self.status {
            Status::None => {}
            Status::Info(msg) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("ℹ️")
                            .color(self.colors.accent2),
                    );
                    ui.label(
                        egui::RichText::new(msg)
                            .color(self.colors.accent2),
                    );
                });
            }
            Status::Error(msg) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("❌")
                            .color(self.colors.danger),
                    );
                    ui.label(
                        egui::RichText::new(msg)
                            .color(self.colors.danger),
                    );
                });
            }
        }
    }

    /// Lock the wallet: clear sensitive state and return to the Locked screen.
    fn lock_wallet(&mut self) {
        self.accounts.clear();
        self.balances.clear();
        self.confirm_remove = false;
        self.rpc_client = None;
        self.active_tab = Tab::Dashboard;
        self.screen = Screen::Locked;
        self.status = Status::None;
    }

    /// Called when the node type dropdown changes in settings.
    #[allow(dead_code)]
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
            PendingOp::SignTransferAssert {
                pending,
                unsigned_tx,
                input_cells,
                lock_args,
            } => match pending.poll() {
                None => {
                    self.pending_op = Some(PendingOp::SignTransferAssert {
                        pending,
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
        }
    }

    /// Complete wallet creation after receiving the PRF output.
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

    /// Kick off a transfer: validate inputs, then build the unsigned tx in a background thread.
    fn start_transfer(&mut self) {
        // Validate inputs
        if self.accounts.is_empty() {
            self.transfer_status = TransferStatus::Error("No accounts available.".to_string());
            return;
        }

        let from_idx = self.transfer_from_account.min(self.accounts.len() - 1);
        let lock_args = self.accounts[from_idx].clone();

        let is_mainnet = self.is_mainnet();
        let from_addr_str = match qpv2_core::utilities::lock_args_to_address(&lock_args, is_mainnet) {
            Ok(a) => a,
            Err(e) => {
                self.transfer_status = TransferStatus::Error(format!("Invalid sender address: {}", e));
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
                self.transfer_status = TransferStatus::Error(format!("Failed to read wallet variant: {}", e));
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
    fn finish_sign_transfer(
        &mut self,
        prf_output: &qpv2_core::SecureVec,
        unsigned_tx: ckb_types::core::TransactionView,
        input_cells: Vec<(ckb_types::packed::CellOutput, ckb_types::bytes::Bytes)>,
        lock_args: String,
    ) {
        use ckb_types::prelude::*;

        // Derive AES key from PRF output
        let key = match qpv2_core::utilities::derive_key_from_prf(prf_output) {
            Ok(k) => k,
            Err(e) => {
                self.transfer_status = TransferStatus::Error(format!("Key derivation failed: {}", e));
                return;
            }
        };

        // Get the wallet variant
        let variant = match KeyVault::get_spx_variant() {
            Ok(v) => v,
            Err(e) => {
                self.transfer_status = TransferStatus::Error(format!("Failed to read variant: {}", e));
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
        let gen_inputs: Vec<(ckb_gen_types::packed::CellOutput, ckb_gen_types::bytes::Bytes)> =
            input_cells
                .iter()
                .map(|(output, data)| {
                    let raw = output.as_slice();
                    let gen_output =
                        ckb_gen_types::packed::CellOutput::from_slice(raw).expect("valid CellOutput");
                    (gen_output, ckb_gen_types::bytes::Bytes::copy_from_slice(data))
                })
                .collect();

        // The packed_tx from ckb_types::packed::Transaction needs converting to
        // ckb_gen_types::packed::Transaction too.
        let gen_tx = ckb_gen_types::packed::Transaction::from_slice(packed_tx.as_slice())
            .expect("valid Transaction");

        if let Err(e) = ckb_fips205_utils::ckb_tx_message_all_from_mock_tx::generate_ckb_tx_message_all(
            &gen_tx,
            &gen_inputs,
            ckb_fips205_utils::ckb_tx_message_all_from_mock_tx::ScriptOrIndex::Index(0),
            &mut hasher,
        ) {
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
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                self.status = Status::Error(format!("Failed to create account: {}", e));
            }
        }
    }

    /// Poll the transfer build channel. When the unsigned tx is ready, trigger Touch ID.
    fn poll_transfer_build(&mut self, frame: &eframe::Frame) {
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
                    match passkey_prf::assert_async(
                        &window,
                        rp_id,
                        &credential_id,
                        Some(salt),
                    ) {
                        Ok(pending) => {
                            self.pending_op = Some(PendingOp::SignTransferAssert {
                                pending,
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
    fn poll_transfer_send(&mut self) {
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

    /// Drain available balance results from the background thread.
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

    /// Show the node selector configuration popup.
    fn show_node_selector_popup(&mut self, ctx: &egui::Context) {
        if !self.node_selector_open {
            return;
        }

        let Some(selector_rect) = self.node_selector_rect else {
            return;
        };

        // Position dropdown below the selector box
        let dropdown_pos = egui::pos2(
            selector_rect.left(),
            selector_rect.bottom() + 4.0,
        );

        egui::Area::new(egui::Id::new("node_selector_dropdown"))
            .fixed_pos(dropdown_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .corner_radius(8.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_width(selector_rect.width() - 24.0);

                        // Network selection (compact horizontal)
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut self.temp_network,
                                node_manager::NetworkType::Mainnet,
                                "Mainnet"
                            );
                            ui.radio_value(
                                &mut self.temp_network,
                                node_manager::NetworkType::Testnet,
                                "Testnet"
                            );
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Node type selection
                        ui.vertical(|ui| {
                            ui.radio_value(
                                &mut self.temp_node_type,
                                NodeType::PublicRpc,
                                "Public RPC"
                            );
                            ui.radio_value(
                                &mut self.temp_node_type,
                                NodeType::LightClient,
                                "Light Client"
                            );
                            ui.radio_value(
                                &mut self.temp_node_type,
                                NodeType::FullNode,
                                "Full Node"
                            );
                        });

                        ui.add_space(8.0);

                        // Apply button
                        let apply_btn = egui::Button::new("Apply")
                            .fill(self.colors.accent)
                            .min_size(egui::vec2(ui.available_width(), 28.0));

                        if ui.add(apply_btn).clicked() {
                            // Check if changes were made
                            let network_changed = self.temp_network != self.node_config.network;
                            let node_type_changed = self.temp_node_type != self.node_config.node_type;

                            if network_changed || node_type_changed {
                                // Update configuration
                                self.node_config.network = self.temp_network;
                                self.node_config.node_type = self.temp_node_type;

                                // Update RPC URL for new configuration
                                if node_type_changed {
                                    self.on_node_type_changed();
                                } else if network_changed && self.node_config.node_type == NodeType::PublicRpc {
                                    // For Public RPC, update URL when network changes
                                    let default_url = self.node_config.default_rpc_url().to_string();
                                    self.node_config.rpc_url = default_url.clone();
                                    self.settings_rpc_url = default_url;
                                }

                                // Save and reconnect
                                self.save_node_config();
                                self.status = Status::Info("Connecting...".to_string());
                            }

                            self.node_selector_open = false;
                        }
                    });
            });

        // Click outside to close
        if ctx.input(|i| i.pointer.any_click()) {
            let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
            if let Some(pos) = pointer_pos {
                let dropdown_rect = egui::Rect::from_min_size(
                    dropdown_pos,
                    egui::vec2(selector_rect.width(), 200.0), // Approximate height
                );
                if !dropdown_rect.contains(pos) && !selector_rect.contains(pos) {
                    self.node_selector_open = false;
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

        // Poll transfer build/send channels.
        self.poll_transfer_build(frame);
        self.poll_transfer_send();

        // Show node selector popup if open
        self.show_node_selector_popup(ctx);

        match self.screen.clone() {
            Screen::Setup => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(self.colors.bg))
                    .show(ctx, |ui| {
                        self.draw_gradient_bg(ui);
                        self.show_setup(ui, frame);
                    });
            }
            Screen::Locked => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(self.colors.bg))
                    .show(ctx, |ui| {
                        self.draw_gradient_bg(ui);
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
        let transfer_pending =
            self.transfer_build_rx.is_some() || self.transfer_send_rx.is_some();
        #[cfg(target_os = "macos")]
        let has_pending_op = self.pending_op.is_some();
        #[cfg(not(target_os = "macos"))]
        let has_pending_op = false;

        if has_pending_op || balance_pending || transfer_pending {
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
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("Quantum Purse"),
        ..Default::default()
    };

    eframe::run_native(
        "qpv2",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}