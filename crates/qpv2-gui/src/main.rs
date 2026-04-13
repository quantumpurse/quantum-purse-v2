//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

mod types;
mod ui;
mod handlers;
#[cfg(target_os = "macos")]
mod window_handle;

use eframe::egui;
use node_manager::{CkbRpc, NodeConfig, NodeType};
use qpv2_core::types::SpxVariant;
use qpv2_core::KeyVault;
use std::collections::HashMap;
use std::sync::mpsc;

#[cfg(target_os = "macos")]
use types::PasskeyOp;
use types::{
    AppColors, BalanceResult, DaoQueryResult, DaoView, Screen, SpendableCapacityTarget, Status,
    Tab, TransactionSendResult, TransactionStatus, TxBuildResult,
};

pub(crate) struct App {
    pub(crate) screen: Screen,
    pub(crate) status: Status,
    pub(crate) colors: AppColors,

    // Setup screen state.
    pub(crate) selected_variant: SpxVariant,

    // Unlocked screen state.
    pub(crate) active_tab: Tab,
    pub(crate) accounts: Vec<String>,
    pub(crate) confirm_remove: bool,

    // Balance cache: lock_args -> balance in shannons (None = not yet fetched).
    pub(crate) balances: HashMap<String, Option<u64>>,

    // Node configuration and RPC connection.
    pub(crate) node_config: NodeConfig,
    pub(crate) rpc_client: Option<Box<dyn CkbRpc>>,

    // Editable settings fields (buffered until saved).
    pub(crate) settings_rpc_url: String,
    pub(crate) settings_binary_path: String,
    pub(crate) settings_data_dir: String,

    // Receives balance results from background thread.
    pub(crate) balance_receiver: Option<mpsc::Receiver<BalanceResult>>,

    // In-flight passkey operation (macOS only).
    #[cfg(target_os = "macos")]
    pub(crate) passkey_op: Option<PasskeyOp>,

    // Node selector popup state.
    pub(crate) node_selector_open: bool,
    pub(crate) node_selector_rect: Option<egui::Rect>,
    // Temporary values for node selector popup.
    pub(crate) temp_network: node_manager::NetworkType,
    pub(crate) temp_node_type: NodeType,

    // Transaction state used for both DAO and transfer flows.
    pub(crate) tx_status: TransactionStatus,
    // Channel for receiving the final send result (tx hash or error).
    pub(crate) transaction_send_rx: Option<mpsc::Receiver<TransactionSendResult>>,
    // Channel for receiving the built unsigned transaction from the background thread.
    pub(crate) transaction_build_rx: Option<mpsc::Receiver<TxBuildResult>>,

    // Transfer form state.
    pub(crate) transfer_recipient: String,
    pub(crate) transfer_amount: String,
    pub(crate) transfer_fee_rate: String,
    pub(crate) transfer_from_account: usize,
    pub(crate) transfer_all: bool,
    pub(crate) spendable_capacity_rx:
        Option<(SpendableCapacityTarget, mpsc::Receiver<Result<u64, String>>)>,

    // DAO state.
    // Each cell is stored with the lock_args of the account that owns it.
    pub(crate) dao_view: DaoView,
    pub(crate) dao_deposited_cells: Vec<(String, node_manager::DepositedCell)>,
    pub(crate) dao_prepared_cells: Vec<(String, node_manager::PreparedCell)>,
    pub(crate) dao_cells_query_rx: Option<mpsc::Receiver<DaoQueryResult>>,
    pub(crate) dao_deposit_amount: String,
    pub(crate) dao_deposit_fee_rate: String,
    pub(crate) dao_deposit_from_account: usize,
    pub(crate) dao_deposit_all: bool,
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

        // Register custom fonts.
        let mut fonts = egui::FontDefinitions::default();

        // Syne ExtraBold for hero balance display.
        fonts.font_data.insert(
            "syne_extrabold".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../../assets/fonts/Syne-ExtraBold.ttf"
            ))),
        );
        fonts.families.insert(
            egui::FontFamily::Name("syne".into()),
            vec!["syne_extrabold".to_owned()],
        );

        // Noto Sans Symbols for arrows and basic symbol glyphs (U+2190–U+21FF, etc.).
        fonts.font_data.insert(
            "noto_symbols".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../../assets/fonts/NotoSansSymbols-Regular.ttf"
            ))),
        );
        // Noto Sans Symbols 2 for geometric shapes and extended symbol glyphs.
        fonts.font_data.insert(
            "noto_symbols2".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../../assets/fonts/NotoSansSymbols2-Regular.ttf"
            ))),
        );
        // Append both as fallbacks so missing glyphs are resolved.
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.push("noto_symbols".to_owned());
            family.push("noto_symbols2".to_owned());
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.push("noto_symbols".to_owned());
            family.push("noto_symbols2".to_owned());
        }

        cc.egui_ctx.set_fonts(fonts);

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
            passkey_op: None,
            node_selector_open: false,
            node_selector_rect: None,
            temp_network,
            temp_node_type,
            transfer_recipient: String::new(),
            transfer_amount: String::new(),
            transfer_fee_rate: "1000".to_string(),
            transfer_from_account: 0,
            transfer_all: false,
            spendable_capacity_rx: None,
            tx_status: TransactionStatus::Idle,
            transaction_build_rx: None,
            transaction_send_rx: None,
            dao_view: DaoView::Overview,
            dao_deposited_cells: Vec::new(),
            dao_prepared_cells: Vec::new(),
            dao_cells_query_rx: None,
            dao_deposit_amount: String::new(),
            dao_deposit_fee_rate: "1000".to_string(),
            dao_deposit_from_account: 0,
            dao_deposit_all: false,
        }
    }

    /// Extract the NSWindow from the eframe Frame (macOS only).
    #[cfg(target_os = "macos")]
    fn get_ns_window(
        frame: &eframe::Frame,
    ) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, String> {
        window_handle::get_ns_window(frame)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Poll passkey operations each frame.
        #[cfg(target_os = "macos")]
        self.poll_passkey_ops();

        // Drain balance results from the background thread.
        self.poll_all_balances();
        self.poll_spendable_capacity();

        // Poll shared transaction build/send channels.
        self.poll_transaction_build(frame);
        self.poll_transaction_send();

        // Poll DAO-specific channels.
        self.poll_dao_cells();

        // Show node selector popup if open
        self.show_node_selector_popup(ctx);

        // Polling main stages of the wallet.
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
        let async_pending = self.balance_receiver.is_some()
            || self.spendable_capacity_rx.is_some()
            || self.transaction_build_rx.is_some()
            || self.transaction_send_rx.is_some()
            || self.dao_cells_query_rx.is_some();
        #[cfg(target_os = "macos")]
        let has_pending_op = self.passkey_op.is_some();
        #[cfg(not(target_os = "macos"))]
        let has_pending_op = false;

        if has_pending_op || async_pending {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 600.0])
            .with_min_inner_size([1000.0, 600.0])
            .with_title("Quantum Purse"),
        ..Default::default()
    };

    eframe::run_native(
        "qpv2",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
