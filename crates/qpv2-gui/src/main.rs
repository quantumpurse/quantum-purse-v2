//! GUI for SPHINCS+ key vault with Passkey PRF / Touch ID support.

mod ckb;
mod poller;
mod transactor;
mod tx_history;
mod types;
mod ui;
mod wallet;

use ckb_node::{LocalNodeProcess, NodeConfig, NodeType, QpClient};
use eframe::egui;
use qpv2_core::types::SpxVariant;
use qpv2_core::KeyVault;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use wallet::{load_last_wallet_id, save_last_wallet_id};

/// Interval between periodic data refreshes (balances, tx history, DAO cells).
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// How long a non-None status banner stays visible before auto-clearing.
const STATUS_DURATION: Duration = Duration::from_secs(5);

use types::{
    AppColors, BalanceResult, DaoQueryResult, DaoView, NodeStatus, NodeStatusUpdate, Screen,
    Status, Tab, TransactionSendResult, TransactionStatus, TxBuildResult, TxHistoryEvent, TxRecord,
};

pub(crate) struct App {
    pub(crate) screen: Screen,
    pub(crate) status: Status,
    pub(crate) colors: AppColors,

    // Active wallet identity.
    pub(crate) wallet_id: u32,
    pub(crate) wallet_name: String,

    // Cached wallet registry — populated on startup, refreshed on
    // create/delete/switch so rendering never hits the filesystem.
    pub(crate) wallet_cache: Vec<types::CurrentWallet>,

    // Wallet selector popup state.
    pub(crate) wallet_selector_open: bool,
    pub(crate) wallet_selector_rect: Option<egui::Rect>,
    // Temporary values for new wallet creation in popup.
    pub(crate) new_wallet_name: String,
    pub(crate) new_wallet_variant: SpxVariant,

    // Wallet create/import modal state.
    pub(crate) wallet_modal: types::WalletModal,

    // Setup screen state.
    pub(crate) selected_variant: SpxVariant,
    pub(crate) import_mode: bool,

    // Unlocked screen state.
    pub(crate) active_tab: Tab,
    pub(crate) accounts: Vec<String>,
    pub(crate) rename_wallet_id: Option<u32>,
    pub(crate) rename_wallet_buf: String,

    // Balance cache: lock_args -> balance in shannons (None = not yet fetched).
    pub(crate) balances: HashMap<String, Option<u64>>,
    // Spendable balance cache: lock_args -> spendable shannons (None = not yet fetched).
    pub(crate) spendable_balances: HashMap<String, Option<u64>>,

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
    pub(crate) node_status_reconnected_at: Option<std::time::Instant>,

    // Manual "set scan from block" input on the Light Client card.
    // Transient UI state; not persisted across sessions.
    pub(crate) set_block_input: String,
    pub(crate) set_block_editing: bool,
    // In-flight async lookup for the "Auto" button — uses a one-shot
    // FullNodeClient against a public endpoint to discover the earliest
    // funding block across all accounts. Some(_) means a detection is
    // running; the poller swaps the result into `set_block_input`.
    pub(crate) earliest_funding_block_rx: Option<mpsc::Receiver<Result<Option<u64>, String>>>,

    // Latched: `true` once `fetch_qr_lock_dep` has confirmed the
    // QR-lock-script cell dep is in the LC's local store. The poller
    // retries the warmup on every status tick where this is still
    // false and the LC is reachable, so the post-spawn race against
    // RPC readiness is invisible to the user. Reset on backend switch.
    pub(crate) lc_qr_dep_warmup_done: bool,

    // Latched: `true` once the poller has registered all accounts'
    // lock scripts with the LC after a backend switch. Reset on
    // backend switch so the poller re-registers against the new LC.
    pub(crate) lc_scripts_registered: bool,

    // ── Auth state ──
    // The auth method recorded in `meta.json`, populated at
    // startup and after wallet creation. Drives Locked-screen rendering
    // (Touch ID button vs none) and per-op routing (Touch ID async
    // flow vs synchronous pinentry prompt).
    pub(crate) auth_method: Option<qpv2_core::types::AuthMethod>,
    // True when App::new put us into `Screen::Unlocked` directly
    // (password-mode wallet at startup) and the first frame still
    // needs to run the same fetches `unlock_with_passkey_finish` does. Cleared
    // on the first `update()` after consumption.
    pub(crate) needs_initial_fetch: bool,

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

        // Audiowide for hero balance display and headings.
        fonts.font_data.insert(
            "audiowide".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../../assets/fonts/Audiowide-Regular.ttf"
            ))),
        );
        fonts.families.insert(
            egui::FontFamily::Name("hero".into()),
            vec!["audiowide".to_owned()],
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

        // Discover existing wallets and pick the active one.
        // Last-used wallet is remembered; falls back to the first in the
        // list. If no wallets exist at all, go straight to Setup.
        let wallets = KeyVault::list_wallets().unwrap_or_default();
        let (screen, wallet_id, wallet_name, auth_method, accounts, needs_initial_fetch) =
            if wallets.is_empty() {
                (Screen::Setup, 0, String::new(), None, Vec::new(), false)
            } else {
                let last_id = load_last_wallet_id();
                let entry = wallets
                    .iter()
                    .find(|w| Some(w.id) == last_id)
                    .unwrap_or(&wallets[0]);
                let wid = entry.id;
                let wname = entry.name.clone();
                save_last_wallet_id(wid);

                let am = KeyVault::read_wallet_info(wid).ok().map(|w| w.auth_method);
                if matches!(am, Some(qpv2_core::types::AuthMethod::Password)) {
                    let accs = KeyVault::get_all_sphincs_lock_args(wid).unwrap_or_default();
                    (Screen::Unlocked, wid, wname, am, accs, true)
                } else {
                    (Screen::Locked, wid, wname, am, Vec::new(), false)
                }
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
                    Status::Info("Local node started.".to_string())
                } else {
                    Status::Info(format!(
                        "Connected to {} ({}).",
                        node_config.network, node_config.node_type,
                    ))
                }
            }
            Err(e) => {
                let msg = format!("Failed to auto-start node: {}", e);
                tracing::error!("{}", msg);
                Status::Error(msg)
            }
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

        let wallet_cache = Self::current_wallet_cache();

        Self {
            screen,
            status: startup_status,
            colors,
            wallet_id,
            wallet_name,
            wallet_cache,
            wallet_selector_open: false,
            wallet_selector_rect: None,
            new_wallet_name: String::new(),
            new_wallet_variant: SpxVariant::Sha2128S,
            wallet_modal: types::WalletModal::None,
            selected_variant: SpxVariant::Sha2128S,
            import_mode: false,
            active_tab: Tab::Dashboard,
            accounts,
            rename_wallet_id: None,
            rename_wallet_buf: String::new(),
            balances: HashMap::new(),
            spendable_balances: HashMap::new(),
            local_node,
            qp_client,
            settings_rpc_url,
            settings_binary_path,
            settings_data_dir,
            balance_receiver: None,
            node_selector_open: false,
            node_selector_rect: None,
            temp_network,
            temp_node_type,
            transfer_recipient: String::new(),
            transfer_amount: String::new(),
            transfer_fee_rate: "1000".to_string(),
            transfer_from_account: 0,
            transfer_all: false,
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
            node_status: NodeStatus {
                online: true,
                ..NodeStatus::default()
            },
            node_status_rx: None,
            node_status_reconnected_at: None,
            set_block_input: String::new(),
            set_block_editing: false,
            earliest_funding_block_rx: None,
            lc_qr_dep_warmup_done: false,
            lc_scripts_registered: false,
            // Auth method is read from meta.json on demand by
            // each flow that needs it; cached `None` here. Setup screen
            // doesn't need it; Locked screen reads it before rendering.
            auth_method,
            needs_initial_fetch,
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // First-frame setup for password-mode wallets that App::new
        // dropped straight into Screen::Unlocked. Mirrors what
        // `unlock_with_keychain` does for Touch ID wallets after a
        // successful unlock.
        if self.needs_initial_fetch {
            self.needs_initial_fetch = false;
            self.last_poll_time = std::time::Instant::now();
            self.load_tx_history_from_disk();
            self.fetch_all_balances();
            self.fetch_tx_history(true);
            self.fetch_dao_cells();
            self.fetch_node_status();
        }

        self.tick_status(ctx);

        if self.screen == Screen::Unlocked {
            self.poll_all_balances();
            self.poll_transaction_build();
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

        // Show popups / modals if open.
        self.show_node_selector_popup(ctx);
        self.show_wallet_selector_popup(ctx);
        self.show_wallet_modal(ctx);

        // Polling main stages of the wallet.
        match self.screen.clone() {
            Screen::Setup => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(self.colors.bg))
                    .show(ctx, |ui| {
                        self.draw_gradient_bg(ui, false);
                        self.show_welcome(ui);
                    });
            }
            Screen::Locked => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(self.colors.bg))
                    .show(ctx, |ui| {
                        self.draw_gradient_bg(ui, true);
                        self.show_locked(ui);
                    });
            }
            Screen::Unlocked => {
                // Sidebar + content layout handled by show_unlocked.
                self.show_unlocked(ctx);
            }
        }

        // Request repaint while an async operation is pending so we poll promptly.
        let async_pending = self.balance_receiver.is_some()
            || self.transaction_build_rx.is_some()
            || self.transaction_send_rx.is_some()
            || self.dao_cells_query_rx.is_some()
            || self.tx_history_rx.is_some();

        if async_pending {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    if let Ok(data_dir) = qpv2_core::db::get_data_dir() {
        let log_path = data_dir.join("qpv2.log");
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let subscriber = tracing_subscriber::fmt()
                .with_writer(file)
                .with_ansi(false)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_max_level(tracing::Level::INFO)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
        }
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 600.0])
            .with_min_inner_size([1200.0, 600.0])
            .with_title("Quantum Purse"),
        ..Default::default()
    };

    eframe::run_native(
        "qpv2",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
