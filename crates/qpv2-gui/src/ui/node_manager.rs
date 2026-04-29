//! Node Manager tab — one card per backend (Public RPC + Light Client).
//! The currently-active backend renders live metrics from the cached
//! `NodeStatus`; the other card shows its static config so the user knows
//! the endpoint exists and can be switched to.

use ckb_node::{NodeConfig, NodeType};
use eframe::egui;

use crate::App;

impl App {
    pub(crate) fn show_node_manager_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Node Manager")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Configure and monitor your CKB node")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                // Three cards stacked vertically. Side-by-side layout is
                // left to a future design pass; vertical scales down
                // nicely on narrow windows.
                self.draw_backend_card(ui, NodeType::PublicRpc);
                ui.add_space(14.0);
                self.draw_backend_card(ui, NodeType::LightClient);
                ui.add_space(14.0);
                self.draw_backend_card(ui, NodeType::FullNode);
            });
        });
    }

    fn draw_backend_card(&mut self, ui: &mut egui::Ui, backend: NodeType) {
        let active = self.qp_client.config().node_type == backend;

        let (icon, title, subtitle) = match backend {
            NodeType::LightClient => (
                "\u{1F4A1}",
                "Light Node",
                "Header-only sync · Fast & lightweight",
            ),
            NodeType::PublicRpc => (
                "\u{1F310}",
                "Public RPC Node",
                "Remote endpoint · No local storage",
            ),
            NodeType::FullNode => (
                "\u{1F5A5}",
                "Full Node",
                "Full chain verification · Local sovereignty",
            ),
        };

        // Active card gets the accent stroke so the user can see at a
        // glance which backend the wallet is currently pointed at.
        let stroke = if active {
            egui::Stroke::new(1.5, self.colors.accent)
        } else {
            egui::Stroke::new(1.0, self.colors.border)
        };

        egui::Frame::new()
            .fill(self.colors.surface)
            .corner_radius(18.0)
            .inner_margin(egui::Margin::symmetric(22, 22))
            .stroke(stroke)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(28.0));
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(title)
                                .size(17.0)
                                .strong()
                                .color(self.colors.text),
                        );
                        ui.label(
                            egui::RichText::new(subtitle)
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.draw_status_pill(ui, backend, active);
                    });
                });

                ui.add_space(18.0);

                // Metric row — live for active, static for inactive.
                // LightClient gets a 5th column ("Synced") so the user
                // can see how far the indexer has caught up.
                let cols_count = if backend == NodeType::LightClient {
                    5
                } else {
                    4
                };
                ui.columns(cols_count, |cols| match (backend, active) {
                    (NodeType::PublicRpc, true) => {
                        self.draw_metric(
                            &mut cols[0],
                            "Block Height",
                            block_height_text(self.node_status.tip_block),
                        );
                        self.draw_metric(
                            &mut cols[1],
                            "Endpoint",
                            hostname_of(&self.qp_client.config().rpc_url),
                        );
                        self.draw_metric(
                            &mut cols[2],
                            "Port",
                            port_text(self.node_status.rpc_port),
                        );
                        self.draw_metric(&mut cols[3], "Peers", "—".into());
                    }
                    (NodeType::PublicRpc, false) => {
                        let url =
                            NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                        self.draw_metric(&mut cols[0], "Block Height", "—".into());
                        self.draw_metric(&mut cols[1], "Endpoint", hostname_of(url));
                        self.draw_metric(&mut cols[2], "Port", default_port(url));
                        self.draw_metric(&mut cols[3], "Peers", "—".into());
                    }
                    (NodeType::LightClient, true) => {
                        self.draw_metric(
                            &mut cols[0],
                            "Block Height",
                            block_height_text(self.node_status.tip_block),
                        );
                        self.draw_synced_metric_editable(&mut cols[1]);
                        self.draw_metric(
                            &mut cols[2],
                            "Peers",
                            peers_text(self.node_status.peer_count),
                        );
                        self.draw_metric(
                            &mut cols[3],
                            "RPC Port",
                            port_text(self.node_status.rpc_port),
                        );
                        self.draw_metric(
                            &mut cols[4],
                            "DB Size",
                            db_size_text(self.node_status.db_size_bytes),
                        );
                    }
                    (NodeType::LightClient, false) => {
                        let url =
                            NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                        self.draw_metric(&mut cols[0], "Block Height", "—".into());
                        self.draw_metric(&mut cols[1], "Synced", "—".into());
                        self.draw_metric(&mut cols[2], "Peers", "—".into());
                        self.draw_metric(&mut cols[3], "RPC Port", default_port(url));
                        self.draw_metric(&mut cols[4], "DB Size", "—".into());
                    }
                    (NodeType::FullNode, true) => {
                        self.draw_metric(
                            &mut cols[0],
                            "Block Height",
                            block_height_text(self.node_status.tip_block),
                        );
                        self.draw_metric(
                            &mut cols[1],
                            "Peers",
                            peers_text(self.node_status.peer_count),
                        );
                        self.draw_metric(
                            &mut cols[2],
                            "RPC Port",
                            port_text(self.node_status.rpc_port),
                        );
                        self.draw_metric(
                            &mut cols[3],
                            "DB Size",
                            db_size_text(self.node_status.db_size_bytes),
                        );
                    }
                    (NodeType::FullNode, false) => {
                        let url =
                            NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                        self.draw_metric(&mut cols[0], "Block Height", "—".into());
                        self.draw_metric(&mut cols[1], "Peers", "—".into());
                        self.draw_metric(&mut cols[2], "RPC Port", default_port(url));
                        self.draw_metric(&mut cols[3], "DB Size", "—".into());
                    }
                });
            });
    }

    fn draw_status_pill(&self, ui: &mut egui::Ui, backend: NodeType, active: bool) {
        // Only the active backend has live status. Inactive cards show a
        // neutral "STANDBY" pill so they don't fake data.
        let (text, bg, fg) = if !active {
            (
                "\u{25CB} STANDBY",
                self.colors.surface2,
                self.colors.text_muted,
            )
        } else if self.node_status.online {
            (
                "\u{25CF} ONLINE",
                self.colors.accent_tint,
                self.colors.accent,
            )
        } else if backend != NodeType::PublicRpc && self.local_node.has_local_process() {
            ("\u{25CC} STARTING", self.colors.warn_tint, self.colors.warn)
        } else {
            (
                "\u{25CB} OFFLINE",
                egui::Color32::from_rgba_unmultiplied(255, 77, 109, 30),
                self.colors.danger,
            )
        };

        egui::Frame::new()
            .fill(bg)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .size(10.5)
                        .family(egui::FontFamily::Monospace)
                        .color(fg),
                );
            });
    }

    /// Editable "Synced" metric: shows the current synced block, with a
    /// pencil glyph at right that swaps the value for an input + Set /
    /// Cancel buttons. On Set, force-applies a `set_scripts(Partial)`
    /// for every account at the user's chosen block (manual cursor
    /// reset). Set is disabled until the input is a valid `u64` and
    /// not above the LC's known tip.
    fn draw_synced_metric_editable(&mut self, ui: &mut egui::Ui) {
        let muted = self.colors.text_muted;
        let text_color = self.colors.text;
        let accent = self.colors.accent;
        let synced_text = block_height_text(self.node_status.synced_block);
        let synced_value = self.node_status.synced_block;
        let tip = self.node_status.tip_block;

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("SYNCED")
                    .size(10.0)
                    .family(egui::FontFamily::Monospace)
                    .color(muted),
            );
            ui.add_space(3.0);

            if !self.set_block_editing {
                // Read-only: value + pencil affordance on the right.
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&synced_text)
                            .size(15.0)
                            .strong()
                            .color(text_color),
                    );
                    let pencil =
                        egui::Label::new(egui::RichText::new("\u{270E}").size(12.0).color(muted))
                            .sense(egui::Sense::click());
                    let resp = ui
                        .add(pencil)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        self.set_block_editing = true;
                        self.set_block_input =
                            synced_value.map(|b| b.to_string()).unwrap_or_default();
                    }
                });
            } else {
                // Edit mode: input + Set / Cancel.
                ui.add(
                    egui::TextEdit::singleline(&mut self.set_block_input)
                        .desired_width(110.0)
                        .font(egui::FontId::monospace(13.0))
                        .text_color(accent),
                );

                // Validate: numeric and ≤ known tip (when tip is known).
                let parsed = self.set_block_input.trim().replace(',', "").parse::<u64>();
                let valid = matches!(&parsed, Ok(b) if tip.map_or(true, |t| *b <= t));

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let set_clicked = ui.add_enabled(valid, egui::Button::new("Set")).clicked();
                    let cancel_clicked = ui.button("Cancel").clicked();
                    // Auto-detect via a one-shot FullNodeClient against the
                    // network's public endpoint. Disabled while a
                    // detection is in flight or there are no accounts
                    // to look up.
                    let auto_enabled =
                        self.earliest_funding_block_rx.is_none() && !self.accounts.is_empty();
                    let auto_label = if self.earliest_funding_block_rx.is_some() {
                        "Auto…"
                    } else {
                        "Auto"
                    };
                    let auto_clicked = ui
                        .add_enabled(auto_enabled, egui::Button::new(auto_label))
                        .clicked();
                    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

                    if set_clicked {
                        if let Ok(block) = parsed {
                            self.set_all_accounts_lock_script_block(block);
                            self.set_block_editing = false;
                            self.set_block_input.clear();
                        }
                    } else if cancel_clicked || escape {
                        self.set_block_editing = false;
                        self.set_block_input.clear();
                    } else if auto_clicked {
                        self.detect_earliest_funding_block_async();
                    }
                });
            }
        });
    }

    fn draw_metric(&self, ui: &mut egui::Ui, label: &str, value: String) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(10.0)
                    .family(egui::FontFamily::Monospace)
                    .color(self.colors.text_muted),
            );
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new(value)
                    .size(15.0)
                    .strong()
                    .color(self.colors.text),
            );
        });
    }
}

fn block_height_text(tip: Option<u64>) -> String {
    tip.map(|n| format!("#{}", format_int(n)))
        .unwrap_or_else(|| "—".to_string())
}

fn peers_text(count: Option<usize>) -> String {
    count
        .map(|n| format!("{} connected", n))
        .unwrap_or_else(|| "—".to_string())
}

fn port_text(port: Option<u16>) -> String {
    port.map(|p| p.to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn db_size_text(bytes: Option<u64>) -> String {
    bytes.map(format_bytes).unwrap_or_else(|| "—".to_string())
}

/// Strips scheme + path to return just the hostname of an RPC URL.
fn hostname_of(url: &str) -> String {
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    stripped
        .split('/')
        .next()
        .unwrap_or(stripped)
        .split(':')
        .next()
        .unwrap_or(stripped)
        .to_string()
}

/// Returns the port portion of an RPC URL, or a scheme-default fallback
/// (`443` / `80`) when the URL has no explicit port.
fn default_port(url: &str) -> String {
    let scheme_stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host_port = scheme_stripped.split('/').next().unwrap_or(scheme_stripped);
    if let Some((_, port)) = host_port.rsplit_once(':') {
        port.to_string()
    } else if url.starts_with("https://") {
        "443".to_string()
    } else {
        "80".to_string()
    }
}

fn format_int(n: u64) -> String {
    let raw = n.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    let chars: Vec<char> = raw.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
