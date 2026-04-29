//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

mod ckb;
#[cfg(target_os = "macos")]
mod passkey;
mod poller;
mod transactor;
mod tx_history_store;
mod types;
mod ui;
mod wallet;
#[cfg(target_os = "macos")]
mod window_handle;

use ckb_node::{LocalNodeProcess, NodeConfig, NodeType, QpClient};
use eframe::egui;
use qpv2_core::types::SpxVariant;
use qpv2_core::KeyVault;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

/// Interval between periodic data refreshes (balances, tx history, DAO cells).
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// How long a non-None status banner stays visible before auto-clearing.
const STATUS_DURATION: Duration = Duration::from_secs(5);

#[cfg(target_os = "macos")]
use types::PasskeyOp;
use types::{
    AppColors, BalanceResult, DaoQueryResult, DaoView, NodeStatus, NodeStatusUpdate, Screen,
    SpendableCapacityTarget, Status, Tab, TransactionSendResult, TransactionStatus, TxBuildResult,
    TxHistoryEvent, TxRecord,
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

    /// Single-owner slot for the local CKB node child process.
    /// Not `Clone` and not shared — its `Drop` is what stops the child
    /// (SIGTERM → grace → SIGKILL), so background-thread aliases are
    /// disallowed by construction.
    pub(crate) local_node: LocalNodeProcess,
    /// Cloneable handle to the active backend — rpc client plus the
    /// `NodeConfig` snapshot it was built from. Background threads
    /// take a clone of this single field instead of capturing the rpc
    /// and a fistful of config scalars separately.
    pub(crate) qp_client: QpClient,

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
    pub(crate) temp_network: ckb_node::NetworkType,
    pub(crate) temp_node_type: NodeType,

    // Transaction state shared by both transfer and DAO flows.
    pub(crate) tx_status: TransactionStatus,
    pub(crate) transaction_send_rx: Option<mpsc::Receiver<TransactionSendResult>>,
    pub(crate) transaction_build_rx: Option<mpsc::Receiver<TxBuildResult>>,
    pub(crate) spendable_capacity_rx:
        Option<(SpendableCapacityTarget, mpsc::Receiver<Result<u64, String>>)>,

    // Transfer form state.
    pub(crate) transfer_recipient: String,
    pub(crate) transfer_amount: String,
    pub(crate) transfer_fee_rate: String,
    pub(crate) transfer_from_account: usize,
    pub(crate) transfer_all: bool,

    // DAO state.
    // Each cell is stored with the lock_args of the account that owns it.
    pub(crate) dao_view: DaoView,
    pub(crate) dao_deposited_cells: Vec<(String, ckb_node::DepositedCell)>,
    pub(crate) dao_prepared_cells: Vec<(String, ckb_node::PreparedCell)>,
    // Staging vectors: accumulated during polling, swapped into display on Done.
    dao_deposited_staging: Vec<(String, ckb_node::DepositedCell)>,
    dao_prepared_staging: Vec<(String, ckb_node::PreparedCell)>,
    pub(crate) dao_cells_query_rx: Option<mpsc::Receiver<DaoQueryResult>>,
    pub(crate) dao_deposit_amount: String,
    pub(crate) dao_deposit_fee_rate: String,
    pub(crate) dao_deposit_from_account: usize,
    pub(crate) dao_deposit_all: bool,

    // Recent transaction history for the dashboard.
    // The incremental-sync floor is derived from this vector on demand —
    // see `App::tx_history_watermark()`.
    pub(crate) tx_history: Vec<TxRecord>,
    pub(crate) tx_history_rx: Option<mpsc::Receiver<Result<TxHistoryEvent, String>>>,

    // Node Manager tab — latest cached snapshot + in-flight refresh.
    pub(crate) node_status: NodeStatus,
    pub(crate) node_status_rx: Option<mpsc::Receiver<NodeStatusUpdate>>,

    // Manual "set scan from block" input on the Light Client card.
    // Transient UI state; not persisted across sessions.
    pub(crate) set_block_input: String,
    pub(crate) set_block_editing: bool,
    // Two-click confirmation gate for switching to FullNode (~100GB
    // sync, multi-day). Reset when the popup closes or the user
    // navigates away from the FullNode option.
    pub(crate) confirm_full_node_pending: bool,
    // In-flight async lookup for the "Auto" button — uses a one-shot
    // FullNodeClient against a public endpoint to discover the earliest
    // funding block across all accounts. Some(_) means a detection is
    // running; the poller swaps the result into `set_block_input`.
    pub(crate) earliest_funding_block_rx: Option<mpsc::Receiver<Result<Option<u64>, String>>>,

    // Periodic polling timer for balances, tx history, and DAO cells.
    pub(crate) last_poll_time: std::time::Instant,

    // Snapshot of the last-observed status + when it became non-None. Used by
    // tick_status() to detect writes via PartialEq and auto-clear after STATUS_DURATION
    pub(crate) status_seen: Status,
    pub(crate) status_set_at: Option<std::time::Instant>,
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

        // Background timer thread: wakes the UI every POLL_INTERVAL so the
        // periodic refresh logic in update() gets a chance to run.
        let repaint_ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(POLL_INTERVAL);
            repaint_ctx.request_repaint();
        });

        // Check if a wallet already exists by trying to read wallet info.
        let screen = if KeyVault::wallet_exists() {
            Screen::Locked
        } else {
            Screen::Setup
        };

        // Restore the last-known node configuration (network + backend +
        // RPC URL) so reopening the app preserves the user's previous
        // choice. For local backends (Light Client and Full Node), the
        // spawn below also restarts the child process so the wallet
        // doesn't come up silently OFFLINE.
        let node_config = NodeConfig::load_or_default().unwrap_or_default();
        let mut local_node = LocalNodeProcess::new(node_config.clone());
        let qp_client = QpClient::new(node_config.clone());

        let startup_status = match local_node.spawn() {
            Ok(()) => {
                if local_node.has_local_process() {
                    // LC-only: warmup the QR-lock-script cell dep so
                    // the first transfer doesn't race-fail. Surface RPC
                    // transport errors; not-yet-Fetched is expected.
                    // Full node indexes everything — no warmup needed,
                    // and the call would error `UnsupportedOperation`.
                    if node_config.node_type == NodeType::LightClient {
                        if let Err(e) = ckb_node::wallet_helpers::lc::fetch_qr_lock_dep(
                            qp_client.client_ref(),
                            qp_client.network(),
                            qp_client.node_type(),
                        ) {
                            Status::Error(format!(
                                "Failed to request lock-script cell dep fetch: {}",
                                e
                            ))
                        } else {
                            Status::Info("Local node started.".to_string())
                        }
                    } else {
                        Status::Info("Local node started.".to_string())
                    }
                } else {
                    // `spawn()` returns Ok(()) on PublicRpc even though
                    // no process is started — it's a no-op backend. Emit
                    // no banner so we don't misleadingly claim a spawn.
                    Status::None
                }
            }
            Err(e) => Status::Error(format!("Failed to auto-start node: {}", e)),
        };

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
            status: startup_status,
            colors,
            selected_variant: SpxVariant::Sha2128S,
            active_tab: Tab::Dashboard,
            accounts: Vec::new(),
            confirm_remove: false,
            balances: HashMap::new(),
            local_node,
            qp_client,
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
            dao_deposited_staging: Vec::new(),
            dao_prepared_staging: Vec::new(),
            dao_cells_query_rx: None,
            dao_deposit_amount: String::new(),
            dao_deposit_fee_rate: "1000".to_string(),
            dao_deposit_from_account: 0,
            dao_deposit_all: false,
            tx_history: Vec::new(),
            tx_history_rx: None,
            node_status: NodeStatus::default(),
            node_status_rx: None,
            set_block_input: String::new(),
            set_block_editing: false,
            confirm_full_node_pending: false,
            earliest_funding_block_rx: None,
            last_poll_time: std::time::Instant::now(),
            status_seen: Status::None,
            status_set_at: None,
        }
    }

    /// Auto-clear the status banner after STATUS_DURATION. Detects writes by
    /// diffing against a snapshot rather than wrapping all 47 `self.status = ...`
    /// call sites in a setter.
    fn tick_status(&mut self, ctx: &egui::Context) {
        if self.status != self.status_seen {
            self.status_seen = self.status.clone();
            self.status_set_at = match self.status {
                Status::None => None,
                _ => Some(std::time::Instant::now()),
            };
            if self.status_set_at.is_some() {
                // Force a repaint at the expiry boundary so the banner
                // disappears without waiting for the next 10s poll tick.
                ctx.request_repaint_after(STATUS_DURATION);
            }
        }
        if let Some(t) = self.status_set_at {
            if t.elapsed() >= STATUS_DURATION {
                self.status = Status::None;
                self.status_seen = Status::None;
                self.status_set_at = None;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Poll passkey operations each frame.
        #[cfg(target_os = "macos")]
        self.poll_passkey_ops();

        if self.screen == Screen::Unlocked {
            self.tick_status(ctx);
            self.poll_all_balances();
            self.poll_spendable_capacity();
            self.poll_transaction_build(frame);
            self.poll_transaction_send();
            self.poll_dao_cells();
            self.poll_tx_history();
            self.poll_node_status();
            self.poll_earliest_funding_block();
        }

        // Periodic refresh of balances, transaction history, DAO cells,
        // and node status.
        if self.screen == Screen::Unlocked && self.last_poll_time.elapsed() >= POLL_INTERVAL {
            self.last_poll_time = std::time::Instant::now();
            self.fetch_all_balances();
            self.fetch_tx_history(true);
            self.fetch_dao_cells();
            self.fetch_node_status();
        }

        // Show node selector popup if open
        self.show_node_selector_popup(ctx);

        // Polling main stages of the wallet.
        match self.screen.clone() {
            Screen::Setup => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(self.colors.bg))
                    .show(ctx, |ui| {
                        self.draw_gradient_bg(ui);
                        self.show_welcome(ui, frame);
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
            || self.dao_cells_query_rx.is_some()
            || self.tx_history_rx.is_some();
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
            .with_inner_size([1100.0, 600.0])
            .with_min_inner_size([1100.0, 600.0])
            .with_title("Quantum Purse"),
        ..Default::default()
    };

    eframe::run_native(
        "qpv2",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
