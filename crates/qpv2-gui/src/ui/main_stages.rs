//! App chrome (telemetry strip, module rail, status line) and the
//! Setup and cover screens.

use crate::types::{label_font, Status, Tab};
use crate::ui::cover::{draw_cover, letters_bottom, reserve_stage, restart_cover};
use crate::ui::utils::{
    blinking_cursor, breathing_dot, draw_qp_logo_chip, ghost_button, hold_button, panel_frame,
    value_flash,
};
use crate::ui::wallet_modal::WALLET_FORM_W;
use crate::App;
use ckb_node::NodeType;
use eframe::egui;

/// Height of the top telemetry strip.
const TELEMETRY_H: f32 = 38.0;
/// Height of the bottom status line.
const STATUSLINE_H: f32 = 26.0;
/// Width of the left module rail.
const RAIL_W: f32 = 138.0;
/// Width of a telemetry segment's dropdown caret, glyph plus its 2px offset.
/// The carets are decoration only — the segment beside each one is the click
/// target — but the wallet's sits in a right-to-left layout with nothing to its
/// right to paint into, so it has to reserve this width to hold its place.
const ARROW_W: f32 = 12.0;
/// How long HOLD TO UNLOCK must be held. Short: the wallet is shown often, and
/// the hold only guards against showing it by accident.
const SHOW_HOLD_SECS: f64 = 1.0;
/// Gap between the cover's stage and its panel.
const PANEL_GAP: f32 = 12.0;

impl App {
    // ────────────────────────────────────────────────────────────────
    // Unlocked chrome
    // ────────────────────────────────────────────────────────────────

    pub(crate) fn show_unlocked(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx();
        self.handle_module_shortcuts(ctx);

        // ── Top telemetry strip ──
        egui::Panel::top("telemetry")
            .exact_size(TELEMETRY_H)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(self.colors.surface))
            .show(ui, |ui| {
                self.draw_telemetry_strip(ui);
                let r = ui.clip_rect();
                ui.painter().hline(
                    r.x_range(),
                    r.bottom() - 0.5,
                    egui::Stroke::new(1.0, self.colors.border),
                );
            });

        // ── Bottom status line ──
        egui::Panel::bottom("statusline")
            .exact_size(STATUSLINE_H)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(self.colors.surface))
            .show(ui, |ui| {
                self.draw_status_line(ui);
                let r = ui.clip_rect();
                ui.painter().hline(
                    r.x_range(),
                    r.top() + 0.5,
                    egui::Stroke::new(1.0, self.colors.border),
                );
            });

        // ── Left module rail ──
        egui::Panel::left("rail")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(RAIL_W)
            .frame(egui::Frame::new().fill(self.colors.surface))
            .show(ui, |ui| {
                self.draw_module_rail(ui);
                let r = ui.clip_rect();
                ui.painter().vline(
                    r.right() - 0.5,
                    r.y_range(),
                    egui::Stroke::new(1.0, self.colors.border),
                );
            });

        // ── Main content area ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.colors.bg))
            .show(ui, |ui| {
                self.draw_unlocked_bg(ui);

                egui::ScrollArea::vertical()
                    .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_space(18.0);

                        match self.active_tab {
                            Tab::Dashboard => self.show_dashboard_tab(ui),
                            Tab::Transfer => self.show_transfer_tab(ui),
                            Tab::DaoOperations => self.show_dao_tab(ui),
                            Tab::NodeManager => self.show_node_manager_tab(ui),
                            Tab::Accounts => self.show_accounts_tab(ui),
                            Tab::Multisig => self.show_multisig_tab(ui),
                            Tab::Wallets => self.show_wallets_tab(ui),
                            Tab::Governance => self.show_governance_tab(ui),
                        }
                    });
            });
    }

    /// Number keys 1–8 switch modules when no text field is focused and no
    /// device task is waiting on the user (see `device_task_in_flight`).
    fn handle_module_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) || self.device_task_in_flight() {
            return;
        }
        const KEYS: [egui::Key; 8] = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
        ];
        for (key, tab) in KEYS.iter().zip(Tab::ALL) {
            if ctx.input(|i| i.key_pressed(*key)) && self.active_tab != tab {
                self.reset_finished_tx_status();
                self.active_tab = tab;
            }
        }
    }

    /// Full-width strip: ident, node telemetry, tip block, network,
    /// then wallet identity and lock on the right.
    ///
    /// Each block is its own `draw_*` below. A block that can disappear owns
    /// its leading divider so the two vanish together; the rest are free-
    /// standing here, which is what makes the run's order readable in one view.
    fn draw_telemetry_strip(&mut self, ui: &mut egui::Ui) {
        let t = ui.input(|i| i.time) as f32;

        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);

            self.draw_ident_block(ui);
            self.strip_divider(ui);
            self.draw_node_segment(ui, t);
            self.strip_divider(ui);
            self.draw_tip_block(ui);
            // Draws its own leading divider, and only when peers are reported.
            self.draw_peers_block(ui);
            // Draws its own leading divider, and only for device wallets.
            self.draw_device_segment(ui, t);
            self.strip_divider(ui);
            self.draw_wallet_group(ui, t);
        });
    }

    /// Ident block: the qp∞ mark knocked out of the accent chip.
    fn draw_ident_block(&self, ui: &mut egui::Ui) {
        let (logo, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
        draw_qp_logo_chip(ui.painter(), logo, self.colors.accent, self.colors.bg);
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(concat!("V", env!("CARGO_PKG_VERSION")).to_uppercase())
                .font(label_font(9.0))
                .color(self.colors.text_muted),
        );
    }

    /// Node segment — clickable, opens the node selector.
    fn draw_node_segment(&mut self, ui: &mut egui::Ui, t: f32) {
        let c_muted = self.colors.text_muted;
        let node_name = match self.qp_client.config().node_type {
            NodeType::PublicRpc => "REMOTE",
            NodeType::LightClient => "LIGHT",
            NodeType::FullNode => "FULL",
        };
        let network = match self.qp_client.config().network {
            ckb_node::NetworkType::Mainnet => "MAIN",
            ckb_node::NetworkType::Testnet => "TEST",
        };
        let network_color = if self.qp_client.is_mainnet() {
            self.colors.accent
        } else {
            self.colors.warn
        };
        // A foreign node on the port is named as what it is, and the
        // selector is locked: the network it would offer to change is not
        // the one the wallet is reading from. See `foreign_node`.
        let foreign_node = self.foreign_node();
        let online = self.node_status.online;
        let dot_color = if foreign_node.is_some() {
            self.colors.warn
        } else if online {
            self.colors.accent2
        } else {
            self.colors.danger
        };

        // Sync percentage rides along for local backends — this is
        // its home; the Networks tab's backend cards show state only.
        // Green once fully synced, accent while catching up.
        let node_type = self.qp_client.config().node_type;
        let sync_suffix = if online && node_type != NodeType::PublicRpc {
            let pct = self.sync_pct(node_type);
            let color = if pct >= 0.999 {
                self.colors.accent2
            } else {
                self.colors.accent
            };
            Some((format!("{:.1}%", pct * 100.0), color))
        } else {
            None
        };
        let node_text = match &foreign_node {
            Some((backend, network)) => {
                let backend = match backend {
                    NodeType::LightClient => "LIGHT",
                    NodeType::FullNode => "FULL",
                    NodeType::PublicRpc => "REMOTE",
                };
                let network = match network.as_str() {
                    "Mainnet" => "MAIN",
                    "Testnet" => "TEST",
                    other => other,
                };
                format!("FOREIGN {} {}", backend, network)
            }
            None => format!("{} / {}", node_name, network),
        };
        let seg = self.strip_segment(
            ui,
            "NODE",
            &node_text,
            sync_suffix.as_ref().map(|(s, c)| (s.as_str(), *c)),
            Some((dot_color, !online && foreign_node.is_none())),
            t,
        );
        if foreign_node.is_some() {
            // No dropdown: the Networks tab is where the foreign node is
            // explained and where the way out (another node type) lives.
            if seg.clicked() && !self.device_task_in_flight() {
                self.active_tab = Tab::NodeManager;
            }
            seg.on_hover_text(
                "Another process holds this node's port, so QPV2 is reading that node \
                 instead of running its own. Click to open the Networks tab.",
            );
            ui.add_space(10.0);
            self.node_selector_open = false;
            if !online {
                ui.ctx().request_repaint();
            }
            return;
        }
        ui.painter().text(
            seg.rect.right_center() + egui::vec2(2.0, 2.0),
            egui::Align2::LEFT_CENTER,
            "▾",
            egui::FontId::proportional(16.0),
            c_muted,
        );
        ui.add_space(10.0);
        self.node_selector_rect = Some(seg.rect);
        if seg.clicked() {
            self.node_selector_open = !self.node_selector_open;
            self.wallet_selector_open = false;
            self.device_popup_open = false;
            if self.node_selector_open {
                let cfg = self.qp_client.config();
                self.network = cfg.network;
                self.node_type = cfg.node_type;
            }
        }
        // Recolor of the network half happens via badge color below
        // the generic segment; repaint while offline so the dot
        // breathes.
        if !online {
            ui.ctx().request_repaint();
        }
        let _ = network_color;
    }

    /// Tip block, flashing bright on each new block.
    fn draw_tip_block(&self, ui: &mut egui::Ui) {
        let c_muted = self.colors.text_muted;
        let tip = self.node_status.tip_block();
        let tip_text = tip
            .map(crate::ui::utils::group_thousands)
            .unwrap_or_else(|| "------".into());
        ui.label(
            egui::RichText::new("TIP")
                .font(label_font(9.0))
                .color(c_muted),
        );
        ui.add_space(2.0);
        // Tip lives in the accent; a new block flashes it bright.
        let flash = value_flash(ui, egui::Id::new("tip-flash"), tip.unwrap_or(0));
        let tip_color = crate::ui::utils::lerp_color(self.colors.accent, self.colors.text, flash);
        ui.label(egui::RichText::new(tip_text).size(11.5).color(tip_color));
    }

    /// Peer count, when the backend reports peers.
    ///
    /// Draws its own leading divider: the block is conditional, so the divider
    /// has to disappear with it rather than leave a stray line behind.
    fn draw_peers_block(&self, ui: &mut egui::Ui) {
        if self.node_status.peers.is_empty() {
            return;
        }
        self.strip_divider(ui);
        ui.label(
            egui::RichText::new("PEERS")
                .font(label_font(9.0))
                .color(self.colors.text_muted),
        );
        ui.add_space(2.0);
        // Green: peers present means healthy connectivity (the
        // segment is hidden entirely at zero peers).
        ui.label(
            egui::RichText::new(format!("{}", self.node_status.peers.len()))
                .size(11.5)
                .color(self.colors.accent2),
        );
    }

    /// Right side: lock + wallet, right-aligned against the strip's edge.
    fn draw_wallet_group(&mut self, ui: &mut egui::Ui, t: f32) {
        let c_muted = self.colors.text_muted;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);

            // Every wallet type can be hidden behind the cover; see
            // `show_locked` for why showing it again needs only a hold. Not
            // while a device task waits on the user: see
            // `device_task_in_flight`.
            let locked = self.device_task_in_flight();
            let hide = ghost_button(&self.colors, "LOCK", egui::vec2(56.0, 22.0));
            if ui.add_enabled(!locked, hide).clicked() {
                self.hide_wallet();
            }
            ui.add_space(10.0);

            let wallet_text = self.wallet_name.to_uppercase();
            // Dropdown arrow, mirroring the node selector's. RTL
            // flow: allocated before the segment so it renders to
            // the segment's right, clear of the LOCK button.
            //
            // Unlike the other two this arrow is a real allocation, because
            // RTL leaves no room to its right to paint into. That means
            // egui's 8px item spacing lands between it and the label, which
            // is what made this gap the widest of the three. Scoped to zero
            // so the caret sits the same 2px off the label as the others,
            // without disturbing the LOCK gap or the right margin outside.
            let seg = ui
                .scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (arrow, _) = ui.allocate_exact_size(
                        egui::vec2(ARROW_W, TELEMETRY_H - 10.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().text(
                        arrow.left_center() + egui::vec2(2.0, 2.0),
                        egui::Align2::LEFT_CENTER,
                        "▾",
                        egui::FontId::proportional(16.0),
                        c_muted,
                    );
                    self.strip_segment(ui, "WALLET", &wallet_text, None, None, t)
                })
                .inner;
            self.wallet_selector_rect = Some(seg.rect);
            if seg.clicked() {
                self.wallet_selector_open = !self.wallet_selector_open;
                self.node_selector_open = false;
                self.device_popup_open = false;
            }
        });
    }

    /// Trezor link segment — only for device-backed wallets.
    ///
    /// Speaks the same grammar as the node segment beside it: breathing dot,
    /// condensed label, clickable. It reports *availability*, not readiness —
    /// whether the device is unlocked cannot be known without starting a
    /// handshake, which would prompt the user and so cannot be polled.
    fn draw_device_segment(&mut self, ui: &mut egui::Ui, t: f32) {
        use qpv2_core::types::AuthMethod;
        use trezor_connect::DeviceStatus;

        if !matches!(self.auth_method, Some(AuthMethod::Trezor { .. })) {
            return;
        }
        self.strip_divider(ui);

        // Any device operation in flight outranks the probe. Probe results
        // arrive through a channel, so one can be dispatched while the device
        // is free and consumed after a session has taken it — displaying
        // LINKED over a device that is mid-signature. Live state is authoritative
        // for "busy"; the probe only answers what we cannot observe directly.
        let connecting = self.trezor_reconnect_rx.is_some();
        let link = if self.trezor_operation_in_flight() {
            DeviceStatus::Working
        } else {
            self.device_status
        };

        let (value, dot_color, urgent) = match link {
            DeviceStatus::Linked => ("LINKED", self.colors.accent2, false),
            DeviceStatus::Emulator => ("EMULATOR", self.colors.accent2, false),
            DeviceStatus::Busy => ("IN USE", self.colors.warn, true),
            DeviceStatus::Absent => ("OFFLINE", self.colors.danger, true),
            DeviceStatus::Working => (
                if connecting { "CONNECTING" } else { "WORKING" },
                self.colors.accent,
                false,
            ),
        };

        let seg = self.strip_segment(ui, "TREZOR", value, None, Some((dot_color, urgent)), t);
        ui.painter().text(
            seg.rect.right_center() + egui::vec2(2.0, 2.0),
            egui::Align2::LEFT_CENTER,
            "▾",
            egui::FontId::proportional(16.0),
            self.colors.text_muted,
        );
        ui.add_space(10.0);

        // Opens the detail dropdown; reconnecting is a deliberate choice made
        // in there, never a side effect of clicking the strip.
        self.device_popup_rect = Some(seg.rect);
        if seg.clicked() {
            self.device_popup_open = !self.device_popup_open;
            self.node_selector_open = false;
            self.wallet_selector_open = false;
        }
        if urgent || connecting {
            ui.ctx().request_repaint();
        }
    }

    /// One clickable label/value segment in the telemetry strip.
    /// `dot` paints a breathing status dot before the value:
    /// `(color, urgent)`.
    fn strip_segment(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        value: &str,
        suffix: Option<(&str, egui::Color32)>,
        dot: Option<(egui::Color32, bool)>,
        t: f32,
    ) -> egui::Response {
        let label_w = label.len() as f32 * 6.0;
        let value_w = value.len() as f32 * 7.2;
        let suffix_w = suffix.map_or(0.0, |(s, _)| s.len() as f32 * 7.2 + 6.0);
        let dot_w = if dot.is_some() { 12.0 } else { 0.0 };
        let w = label_w + dot_w + value_w + suffix_w + 10.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(w, TELEMETRY_H - 10.0), egui::Sense::click());
        let painter = ui.painter();

        if response.hovered() {
            painter.rect_filled(rect, 0.0, self.colors.accent_tint);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let mut x = rect.left() + 2.0;
        painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            label_font(9.0),
            self.colors.text_muted,
        );
        x += label_w + 4.0;
        if let Some((color, urgent)) = dot {
            breathing_dot(
                painter,
                egui::pos2(x + 3.0, rect.center().y),
                color,
                t,
                urgent,
            );
            x += dot_w;
        }
        let value_rect = painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            value,
            egui::FontId::proportional(11.5),
            self.colors.text,
        );
        if let Some((text, color)) = suffix {
            painter.text(
                egui::pos2(value_rect.right() + 6.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                egui::FontId::proportional(11.5),
                color,
            );
        }

        response
    }

    fn strip_divider(&self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(1.0, TELEMETRY_H - 14.0), egui::Sense::hover());
        ui.painter().vline(
            rect.center().x,
            rect.y_range(),
            egui::Stroke::new(1.0, self.colors.border),
        );
        ui.add_space(12.0);
    }

    /// Persistent one-line event log and UTC clock.
    fn draw_status_line(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            match &self.status {
                Status::None => {
                    ui.label(
                        egui::RichText::new("STATUS")
                            .font(label_font(9.5))
                            .color(self.colors.text_muted),
                    );
                }
                _ => self.show_status(ui),
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(chrono::Utc::now().format("%H:%M:%S UTC").to_string())
                        .font(label_font(9.5))
                        .color(self.colors.text_muted),
                );
            });
        });
    }

    /// Slim left rail: numbered module codes, accent active state with
    /// a blinking cursor.
    fn draw_module_rail(&mut self, ui: &mut egui::Ui) {
        let t = ui.input(|i| i.time) as f32;
        ui.add_space(10.0);
        // The user stays on this module while a device task waits on them:
        // see `device_task_in_flight`.
        let locked = self.device_task_in_flight();

        for (i, tab) in Tab::ALL.into_iter().enumerate() {
            let is_active = self.active_tab == tab;
            let response =
                ui.allocate_response(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
            let hovered = response.hovered() && !locked;
            if response.clicked() && !locked && self.active_tab != tab {
                self.reset_finished_tx_status();
                self.active_tab = tab;
            }

            let rect = response.rect;
            let painter = ui.painter();

            if is_active {
                painter.rect_filled(rect, 0.0, self.colors.accent_tint);
                painter.rect_filled(
                    egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.0, rect.height())),
                    0.0,
                    self.colors.accent,
                );
            } else if hovered {
                painter.rect_filled(rect, 0.0, self.colors.surface2);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            let code_color = if is_active {
                self.colors.accent
            } else if hovered {
                self.colors.text
            } else {
                self.colors.text_muted
            };

            // Index number.
            painter.text(
                egui::pos2(rect.left() + 14.0, rect.top() + 13.0),
                egui::Align2::LEFT_CENTER,
                format!("{:02}", i + 1),
                label_font(8.0),
                self.colors.text_muted,
            );
            // Module code.
            let code_pos = egui::pos2(rect.left() + 34.0, rect.top() + 13.0);
            painter.text(
                code_pos,
                egui::Align2::LEFT_CENTER,
                tab.code(),
                label_font(12.0),
                code_color,
            );
            if is_active {
                let code_w = tab.code().len() as f32 * 9.0;
                blinking_cursor(
                    painter,
                    egui::pos2(code_pos.x + code_w + 5.0, code_pos.y),
                    11.0,
                    self.colors.accent,
                    t,
                );
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(120));
            }
            // Module name.
            painter.text(
                egui::pos2(rect.left() + 34.0, rect.top() + 28.0),
                egui::Align2::LEFT_CENTER,
                tab.name(),
                egui::FontId::proportional(9.5),
                if is_active {
                    self.colors.text
                } else {
                    self.colors.text_muted
                },
            );
        }
    }

    // ────────────────────────────────────────────────────────────────
    // Setup — wallet bootstrap terminal
    // ────────────────────────────────────────────────────────────────

    pub(crate) fn show_welcome(&mut self, ui: &mut egui::Ui) {
        // The form matches the dialog's; the extra 30 px is the panel's
        // padding and border.
        let panel_w = WALLET_FORM_W + 30.0;

        ui.vertical_centered(|ui| {
            ui.add_space(15.0);

            // The qp∞ mark at full size — the one place it gets room.
            let (logo, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
            draw_qp_logo_chip(ui.painter(), logo, self.colors.accent, self.colors.bg);
            ui.add_space(10.0);

            ui.label(
                egui::RichText::new("QUANTUM PURSE // POST-QUANTUM HARDENED // NERVOS CKB")
                    .font(label_font(10.0))
                    .color(self.colors.text_muted),
            );

            ui.add_space(15.0);

            ui.allocate_ui_with_layout(
                egui::vec2(panel_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    // The New Wallet dialog's form, minus its Cancel.
                    panel_frame(&self.colors).show(ui, |ui| {
                        ui.set_width(panel_w - 30.0);
                        if let Some(is_import) =
                            self.create_import_wallet_form(ui, self.is_import_mode)
                        {
                            self.is_import_mode = is_import;
                        }
                    });
                },
            );

            ui.add_space(16.0);
            // Same centered block width as the panel above, so the
            // status line's left edge aligns with the panel's.
            ui.allocate_ui_with_layout(
                egui::vec2(panel_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.show_status(ui),
            );
        });
    }

    // ────────────────────────────────────────────────────────────────
    // Hidden — the cover over the open wallet
    // ────────────────────────────────────────────────────────────────

    /// The cover: the qp∞ mark assembling out of dust, the wallet's name, and
    /// a hold to show it. Privacy, not security: every action that spends or
    /// reveals a secret asks for authentication on its own, so the hold only
    /// guards against showing the wallet by accident.
    pub(crate) fn show_locked(&mut self, ui: &mut egui::Ui) {
        let panel_w = 520.0;

        let variant = self
            .wallet_cache
            .iter()
            .find(|w| w.id == self.wallet_id)
            .map(|cw| format!("SPHINCS+ {}", cw.spx_variant))
            .unwrap_or_else(|| "SPHINCS+".to_string());

        // Every stop of a local node is followed by a fresh start, except the
        // watchdog's after a crash, so a local backend with no process is one
        // that failed to start (`poll_local_node_liveness`).
        let node_name = match self.qp_client.config().node_type {
            NodeType::LightClient => "LIGHT CLIENT",
            NodeType::FullNode => "FULL NODE",
            NodeType::PublicRpc => "REMOTE RPC",
        };
        let noding = if self.qp_client.config().node_type == NodeType::PublicRpc {
            format!("USING {}", node_name)
        } else if self.local_node.has_local_process() {
            format!("STARTED {}", node_name)
        } else {
            format!("FAILED TO START {}", node_name)
        };

        ui.vertical_centered(|ui| {
            // Centre the mark and panel vertically, using the height they took
            // last frame; the status line hangs below and is left out, so a
            // message appearing doesn't shift them.
            let height_id = egui::Id::new("cover-group-height");
            let last_height = ui
                .ctx()
                .memory(|m| m.data.get_temp::<f32>(height_id))
                .unwrap_or(0.0);
            ui.add_space(((ui.available_height() - last_height) / 2.0).max(24.0));
            let top = ui.cursor().min.y;

            // The stage is painted after the panel, once the hold button has
            // said how far through the hold this frame is.
            let stage = reserve_stage(ui);

            // The screen reads as a face: q and p are the eyes, the headline
            // the nose, the boot lines the mustache. The nose sits halfway
            // between the letters and the boot lines, so it reads as its own
            // feature rather than part of either.
            let boot_top = stage.bottom()
                + ui.spacing().item_spacing.y
                + PANEL_GAP
                + panel_frame(&self.colors).total_margin().top;
            ui.painter().text(
                egui::pos2(stage.center().x, (letters_bottom(stage) + boot_top) / 2.0),
                egui::Align2::CENTER_CENTER,
                "POST QUANTUM HARDENED, POWERED BY CKB",
                label_font(10.0),
                self.colors.text_muted,
            );
            ui.add_space(PANEL_GAP);

            let hold = ui
                .allocate_ui_with_layout(
                    egui::vec2(panel_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        panel_frame(&self.colors)
                            .show(ui, |ui| {
                                ui.set_width(panel_w - 30.0);

                                // Centred as one block, its lines still left
                                // aligned.
                                ui.vertical_centered(|ui| {
                                    boot_lines(
                                        ui,
                                        "locked-boot",
                                        &format!(
                                            "> WALLET ......... {}\n\
                                             > SCHEME ......... {}\n\
                                             > STATUS ......... LOCKED\n\
                                             > NODING ......... {}",
                                            self.wallet_name.to_uppercase(),
                                            variant,
                                            noding,
                                        ),
                                        90.0,
                                        11.5,
                                        self.colors.text_muted,
                                    );
                                });

                                ui.add_space(18.0);

                                // Unlabelled: the label is painted below, once
                                // the letters' pull is known.
                                hold_button(
                                    ui,
                                    egui::Id::new("cover-hold-to-show"),
                                    "",
                                    self.colors.accent,
                                    egui::vec2(panel_w - 30.0, 42.0),
                                    SHOW_HOLD_SECS,
                                    None,
                                )
                            })
                            .inner
                    },
                )
                .inner;
            let cover = draw_cover(ui, stage, &self.colors, &hold);
            // HOLD TO UNLOCK rides the letters' pull rather than the fill: the
            // same while held, but it eases back with them on an early
            // release, and stays right while the logo forms, where the fill
            // has already reset.
            hold_label(ui, hold.rect, cover.pull, self.colors.accent);
            if cover.done {
                restart_cover(ui.ctx());
                self.show_wallet();
            }
            let height = ui.cursor().min.y - top;
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(height_id, height));

            ui.add_space(16.0);
            // Same centered block width as the panel above, so the
            // status line's left edge aligns with the panel's.
            ui.allocate_ui_with_layout(
                egui::vec2(panel_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.show_status(ui),
            );
        });
    }
}

/// Paints HOLD TO UNLOCK over `button`, `along` (0 to 1) of the way from its
/// left padding to its right.
fn hold_label(ui: &egui::Ui, button: egui::Rect, along: f32, color: egui::Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap("HOLD TO UNLOCK".to_string(), label_font(11.0), color);
    let pad = ui.spacing().button_padding.x;
    let room = (button.width() - 2.0 * pad - galley.size().x).max(0.0);
    let pos = egui::pos2(
        button.left() + pad + along * room,
        button.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
}

/// Type-on boot lines in a fixed-size slot: the full text is measured
/// up front and its rect allocated immediately, so the reveal animation
/// never reflows the content below it.
fn boot_lines(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    text: &str,
    cps: f64,
    size: f32,
    color: egui::Color32,
) {
    let typed = type_on(ui, id, text, cps);
    let font = egui::FontId::proportional(size);
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), color);
    let (rect, _) = ui.allocate_exact_size(full.size(), egui::Sense::hover());
    ui.painter()
        .text(rect.left_top(), egui::Align2::LEFT_TOP, typed, font, color);
}

/// Reveal `text` progressively at `cps` characters per second from the
/// first frame this id is seen — the terminal type-on effect.
fn type_on(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    text: &str,
    cps: f64,
) -> String {
    let id = egui::Id::new(id);
    let now = ui.input(|i| i.time);
    let start = ui
        .ctx()
        .memory_mut(|m| *m.data.get_temp_mut_or_insert_with(id, || now));
    let shown = ((now - start) * cps).max(0.0) as usize;
    if shown < text.chars().count() {
        ui.ctx().request_repaint();
        text.chars().take(shown).collect()
    } else {
        text.to_string()
    }
}
