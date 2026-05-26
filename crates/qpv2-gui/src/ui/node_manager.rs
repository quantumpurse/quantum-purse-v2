//! Node Manager tab rendering.

use ckb_node::{NetworkType, NodeConfig, NodeType};
use eframe::egui;

use crate::types::Status;
use crate::App;

const METRIC_LABEL_PAD: usize = 13;
const ROW_INDENT: f32 = 24.0;

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

                ui.add_space(16.0);
                self.draw_network_toggle(ui);
                ui.add_space(14.0);

                self.draw_backend_section(ui, NodeType::PublicRpc);
                self.draw_backend_section(ui, NodeType::LightClient);
                self.draw_backend_section(ui, NodeType::FullNode);
            });
        });
    }

    fn backend_accent(&self, backend: NodeType) -> egui::Color32 {
        match backend {
            NodeType::FullNode => self.colors.accent,
            NodeType::LightClient => self.colors.accent2,
            NodeType::PublicRpc => self.colors.accent3,
        }
    }

    fn draw_network_toggle(&mut self, ui: &mut egui::Ui) {
        let current_network = self.qp_client.config().network;
        let mut selected = current_network;

        ui.horizontal(|ui| {
            let seg_w = 80.0;
            let seg_h = 28.0;
            let total_w = seg_w * 2.0 + 2.0;
            let (outer_rect, _) =
                ui.allocate_exact_size(egui::vec2(total_w, seg_h), egui::Sense::hover());
            let painter = ui.painter();

            painter.rect_filled(outer_rect, 6.0, self.colors.surface2);
            painter.rect_stroke(
                outer_rect,
                6.0,
                egui::Stroke::new(1.0, self.colors.border),
                egui::StrokeKind::Inside,
            );

            let segments = [
                (NetworkType::Mainnet, "Mainnet", self.colors.accent),
                (NetworkType::Testnet, "Testnet", self.colors.accent2),
            ];

            for (i, &(net, label, accent)) in segments.iter().enumerate() {
                let x = outer_rect.left() + 1.0 + i as f32 * seg_w;
                let seg_rect = egui::Rect::from_min_size(
                    egui::pos2(x, outer_rect.top() + 1.0),
                    egui::vec2(seg_w, seg_h - 2.0),
                );

                let is_active = selected == net;
                let resp = ui.interact(
                    seg_rect,
                    ui.id().with(("net-seg", i)),
                    egui::Sense::click(),
                );

                if is_active {
                    painter.rect_filled(
                        seg_rect,
                        5.0,
                        egui::Color32::from_rgba_unmultiplied(
                            accent.r(),
                            accent.g(),
                            accent.b(),
                            25,
                        ),
                    );
                } else if resp.hovered() {
                    painter.rect_filled(
                        seg_rect,
                        5.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6),
                    );
                }

                let text_color = if is_active {
                    accent
                } else {
                    self.colors.text_muted
                };
                painter.text(
                    seg_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(12.0),
                    text_color,
                );

                if resp.clicked() {
                    selected = net;
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
        });

        if selected != current_network {
            self.temp_network = selected;
            self.temp_node_type = self.qp_client.config().node_type;
            if self.temp_node_type == NodeType::PublicRpc {
                self.settings_rpc_url =
                    NodeConfig::default_rpc_url_for(self.temp_node_type, self.temp_network)
                        .to_string();
            }
            self.commit_node_switch();
        }
    }

    pub(crate) fn switch_to_backend(&mut self, backend: NodeType) {
        let current = self.qp_client.config();
        if current.node_type == backend {
            return;
        }
        self.temp_node_type = backend;
        self.temp_network = current.network;
        self.on_node_type_changed();
        self.commit_node_switch();
    }

    fn commit_node_switch(&mut self) {
        let old_network = self.qp_client.config().network;
        self.local_node.stop();
        self.apply_node_config();
        if let Err(e) = self.local_node.spawn() {
            let msg = format!("Failed to start local node: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
        }
        if self.temp_network != old_network {
            self.tx_history_rx = None;
            self.load_tx_history_from_disk();
        }
        if !matches!(self.status, Status::Error(_)) {
            self.status = Status::Info("Connecting...".to_string());
        }
    }

    fn sync_pct(&self, backend: NodeType) -> f32 {
        match backend {
            NodeType::LightClient => {
                match (self.node_status.synced_block, self.node_status.tip_block) {
                    (Some(s), Some(t)) if t > 0 => (s as f64 / t as f64).clamp(0.0, 1.0) as f32,
                    _ => 0.0,
                }
            }
            NodeType::FullNode => full_node_sync_view(self.node_status.sync_state.as_ref()).0,
            NodeType::PublicRpc => 0.0,
        }
    }

    fn draw_backend_section(&mut self, ui: &mut egui::Ui, backend: NodeType) {
        let active = self.qp_client.config().node_type == backend;
        let accent = self.backend_accent(backend);

        let (icon, title, subtitle) = match backend {
            NodeType::LightClient => (
                "\u{1F4A1}",
                "Light Node",
                "FlyClient protocol \u{00B7} Fast & lightweight",
            ),
            NodeType::PublicRpc => (
                "\u{1F310}",
                "Remote RPC Node",
                "Remote endpoint \u{00B7} No local storage",
            ),
            NodeType::FullNode => (
                "\u{1F5A5}",
                "Full Node",
                "Full chain verification \u{00B7} Local sovereignty",
            ),
        };

        self.paint_entanglement_divider(ui);
        ui.add_space(14.0);

        let header = ui.horizontal(|ui| {
            ui.add_space(13.0);
            ui.label(egui::RichText::new(icon).size(20.0));
            ui.add_space(8.0);
            ui.vertical(|ui| {
                let title_color = if active { accent } else { self.colors.text };
                ui.label(
                    egui::RichText::new(title)
                        .size(16.0)
                        .strong()
                        .color(title_color),
                );
                ui.label(
                    egui::RichText::new(subtitle)
                        .size(11.0)
                        .color(self.colors.text_muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.draw_status_pill(ui, backend, active, accent);
            });
        });

        let header_rect = header.response.rect;

        if active {
            let bar = egui::Rect::from_min_max(
                egui::pos2(header_rect.left(), header_rect.top() + 2.0),
                egui::pos2(header_rect.left() + 3.0, header_rect.bottom() - 2.0),
            );
            ui.painter().rect_filled(bar, 1.5, accent);
        }

        let mut switch_requested = false;
        if !active {
            let click = ui
                .interact(
                    header_rect,
                    ui.id().with(backend as u8),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if click.hovered() {
                ui.painter().rect_filled(
                    header_rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 10),
                );
            }
            switch_requested = click.clicked();
        }

        ui.add_space(14.0);

        let has_sync = active && backend != NodeType::PublicRpc;
        let pct = if has_sync {
            self.sync_pct(backend)
        } else {
            0.0
        };
        let is_lc = active && backend == NodeType::LightClient;
        let is_editing = self.set_block_editing;

        let surface2 = self.colors.surface2;
        let text_muted = self.colors.text_muted;

        ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
            ui.vertical(|ui| {
                if has_sync {
                    ui.set_width(ui.available_width() - 110.0);
                }

                let metrics = self.metric_cells(backend, active);
                for (i, (label, value)) in metrics.iter().enumerate() {
                    if is_lc && i == 0 && !is_editing {
                        self.draw_sync_metric_row(ui, value);
                    } else {
                        self.draw_metric_row(ui, label, value);
                    }
                    if i < metrics.len() - 1 {
                        ui.add_space(4.0);
                    }
                }

                if is_lc && is_editing {
                    ui.add_space(8.0);
                    self.draw_sync_edit_section(ui);
                }
            });

            if has_sync {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    draw_donut_gauge(ui, pct, accent, surface2, text_muted);
                });
            }
        });

        if backend == NodeType::FullNode && active {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(ROW_INDENT);
                ui.label(
                    egui::RichText::new(
                        "\u{26A0} Full node will sync ~100 GB and may take \
						 several days. Disk and bandwidth heavy.",
                    )
                    .size(10.0)
                    .color(self.colors.warn),
                );
            });
        }

        if switch_requested {
            self.switch_to_backend(backend);
        }

        ui.add_space(14.0);
    }

    fn draw_status_pill(
        &self,
        ui: &mut egui::Ui,
        backend: NodeType,
        active: bool,
        accent: egui::Color32,
    ) {
        let (text, bg, fg) = if !active {
            (
                "\u{25CB} STANDBY",
                self.colors.surface2,
                self.colors.text_muted,
            )
        } else if self.node_status.online {
            let tint =
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30);
            ("\u{25CF} ONLINE", tint, accent)
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
                        .size(10.0)
                        .family(egui::FontFamily::Monospace)
                        .strong()
                        .color(fg),
                );
            });
    }

    fn metric_cells(&self, backend: NodeType, active: bool) -> Vec<(&'static str, String)> {
        const DASH: &str = "\u{2014}";
        match backend {
            NodeType::PublicRpc => {
                let (active_url, fallback_url);
                let url: &str = if active {
                    active_url = self.qp_client.config().rpc_url.clone();
                    &active_url
                } else {
                    fallback_url =
                        NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                    fallback_url
                };
                let block_height = if active {
                    block_height_text(self.node_status.tip_block)
                } else {
                    DASH.into()
                };
                let port = if active {
                    port_text(self.node_status.rpc_port)
                } else {
                    port_text(crate::ckb::parse_rpc_port(url))
                };
                let peers = if active {
                    peers_text(self.node_status.peer_count)
                } else {
                    DASH.into()
                };
                vec![
                    ("Block Height", block_height),
                    ("Endpoint", hostname_of(url)),
                    ("Peers", peers),
                    ("Port", port),
                ]
            }
            NodeType::LightClient | NodeType::FullNode => {
                let tip_label = if backend == NodeType::FullNode {
                    "Local Tip"
                } else {
                    "Tip"
                };
                if active {
                    vec![
                        ("Sync", synced_value(backend, &self.node_status)),
                        (tip_label, target_tip_value(backend, &self.node_status)),
                        ("Peers", peers_text(self.node_status.peer_count)),
                        ("RPC Port", port_text(self.node_status.rpc_port)),
                    ]
                } else {
                    let url = NodeConfig::default_rpc_url_for(backend, self.qp_client.network());
                    vec![
                        ("Sync", DASH.into()),
                        (tip_label, DASH.into()),
                        ("Peers", DASH.into()),
                        ("RPC Port", port_text(crate::ckb::parse_rpc_port(url))),
                    ]
                }
            }
        }
    }

    fn draw_metric_row(&self, ui: &mut egui::Ui, label: &str, value: &str) {
        ui.horizontal(|ui| {
            ui.add_space(ROW_INDENT);
            ui.label(
                egui::RichText::new(format!("{:<width$}", label, width = METRIC_LABEL_PAD))
                    .size(12.0)
                    .family(egui::FontFamily::Monospace)
                    .color(self.colors.text_muted),
            );
            ui.label(
                egui::RichText::new(value)
                    .size(12.0)
                    .family(egui::FontFamily::Monospace)
                    .strong()
                    .color(self.colors.text),
            );
        });
    }

    fn draw_sync_metric_row(&mut self, ui: &mut egui::Ui, value: &str) {
        let synced = self.node_status.synced_block;
        ui.horizontal(|ui| {
            ui.add_space(ROW_INDENT);
            ui.label(
                egui::RichText::new(format!("{:<width$}", "Sync", width = METRIC_LABEL_PAD))
                    .size(12.0)
                    .family(egui::FontFamily::Monospace)
                    .color(self.colors.text_muted),
            );
            ui.label(
                egui::RichText::new(value)
                    .size(12.0)
                    .family(egui::FontFamily::Monospace)
                    .strong()
                    .color(self.colors.text),
            );
            ui.add_space(4.0);
            let pen = ui.add(
                egui::Button::new(
                    egui::RichText::new("\u{270f}")
                        .size(11.0)
                        .color(self.colors.text_muted),
                )
                .fill(egui::Color32::TRANSPARENT)
                .frame(false),
            );
            if pen.clicked() {
                self.set_block_editing = true;
                self.set_block_input = synced.map(|b| b.to_string()).unwrap_or_default();
            }
        });
    }

    fn draw_sync_edit_section(&mut self, ui: &mut egui::Ui) {
        let tip = self.node_status.tip_block;

        ui.horizontal(|ui| {
            ui.add_space(ROW_INDENT);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("SET SYNC START BLOCK")
                        .size(9.0)
                        .family(egui::FontFamily::Monospace)
                        .color(self.colors.text_muted),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.set_block_input)
                        .desired_width(260.0)
                        .font(egui::FontId::monospace(13.0))
                        .text_color(self.colors.text),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let parsed = self.set_block_input.trim().replace(',', "").parse::<u64>();
                    let valid = matches!(&parsed, Ok(b) if tip.is_none_or(|t| *b <= t));

                    let set_clicked = ui.add_enabled(valid, egui::Button::new("Set")).clicked();
                    let cancel_clicked = ui.button("Cancel").clicked();
                    let auto_enabled =
                        self.earliest_funding_block_rx.is_none() && !self.accounts.is_empty();
                    let auto_label = if self.earliest_funding_block_rx.is_some() {
                        "Auto\u{2026}"
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
            });
        });
    }
}

fn draw_donut_gauge(
    ui: &mut egui::Ui,
    pct: f32,
    accent: egui::Color32,
    track_color: egui::Color32,
    muted_color: egui::Color32,
) {
    const SIZE: f32 = 90.0;
    const RADIUS: f32 = 32.0;
    const STROKE_W: f32 = 6.0;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    let center = rect.center();
    let painter = ui.painter();

    painter.circle_stroke(center, RADIUS, egui::Stroke::new(STROKE_W, track_color));

    if pct > 0.001 {
        let start = -std::f32::consts::FRAC_PI_2;
        let sweep = std::f32::consts::TAU * pct.min(1.0);
        let n = (64.0 * pct).max(4.0) as usize;
        let points: Vec<egui::Pos2> = (0..=n)
            .map(|i| {
                let angle = start + sweep * (i as f32 / n as f32);
                egui::pos2(
                    center.x + RADIUS * angle.cos(),
                    center.y + RADIUS * angle.sin(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(STROKE_W, accent),
        ));
    }

    let (text, color) = if pct > 0.001 {
        (format!("{:.1}%", pct * 100.0), accent)
    } else {
        ("\u{2014}".to_string(), muted_color)
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        &text,
        egui::FontId::new(13.0, egui::FontFamily::Monospace),
        color,
    );
}

fn full_node_sync_view(sync_state: Option<&ckb_jsonrpc_types::SyncState>) -> (f32, String) {
    let Some(s) = sync_state else {
        return (0.0, "\u{2014}".to_string());
    };
    let tip = s.tip_number.value();
    let best = s.best_known_block_number.value();

    if best > 0 {
        let p = (tip as f64 / best as f64).clamp(0.0, 1.0) as f32;
        (p, format!("{:.1}%", p * 100.0))
    } else {
        (0.0, "\u{2014}".to_string())
    }
}

fn synced_value(backend: NodeType, status: &crate::types::NodeStatus) -> String {
    match backend {
        NodeType::FullNode => match status.sync_state.as_ref() {
            Some(s) => format!("#{}", format_int(s.tip_number.value())),
            None => "\u{2014}".into(),
        },
        NodeType::LightClient => match status.synced_block {
            Some(s) => format!("#{}", format_int(s)),
            None => "\u{2014}".into(),
        },
        NodeType::PublicRpc => "\u{2014}".into(),
    }
}

fn target_tip_value(backend: NodeType, status: &crate::types::NodeStatus) -> String {
    match backend {
        NodeType::FullNode => match status.sync_state.as_ref() {
            Some(s) => format!("#{}", format_int(s.best_known_block_number.value())),
            None => "\u{2014}".into(),
        },
        NodeType::LightClient => match status.tip_block {
            Some(t) => format!("#{}", format_int(t)),
            None => "\u{2014}".into(),
        },
        NodeType::PublicRpc => "\u{2014}".into(),
    }
}

fn block_height_text(tip: Option<u64>) -> String {
    tip.map(|n| format!("#{}", format_int(n)))
        .unwrap_or_else(|| "\u{2014}".to_string())
}

fn peers_text(count: Option<usize>) -> String {
    count
        .map(|n| format!("{} connected", n))
        .unwrap_or_else(|| "\u{2014}".to_string())
}

fn port_text(port: Option<u16>) -> String {
    port.map(|p| p.to_string())
        .unwrap_or_else(|| "\u{2014}".to_string())
}

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

fn format_int(n: u64) -> String {
    let raw = n.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    let chars: Vec<char> = raw.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*ch);
    }
    out
}
