//! Node Manager tab rendering.

use ckb_node::{NetworkType, NodeConfig, NodeType};
use eframe::egui;

use crate::types::Status;
use crate::App;

const LABEL_PAD: usize = 14;
const DASH: &str = "\u{2014}";

impl App {
    pub(crate) fn show_node_manager_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(
                            egui::RichText::new("Node Manager")
                                .size(26.0)
                                .strong()
                                .color(self.colors.text),
                        );
                        ui.label(
                            egui::RichText::new("Configure and monitor your CKB node.")
                                .size(13.0)
                                .color(self.colors.text_muted),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.draw_network_toggle(ui);
                    });
                });

                ui.add_space(14.0);
                let switch_to = self.draw_tab_bar(ui);
                ui.add_space(20.0);

                let backend = self.qp_client.config().node_type;

                // ── Blockchain ───────────────────────────
                self.draw_section_heading(ui, "Blockchain");
                self.draw_blockchain_metrics(ui);

                ui.add_space(16.0);

                // ── Tx Pool (not available on LC) ───────
                if backend != NodeType::LightClient {
                    self.draw_section_heading(ui, "Tx Pool");
                    self.draw_tx_pool_metrics(ui);
                    ui.add_space(16.0);
                }

                // ── Node ─────────────────────────────────
                self.draw_section_heading(ui, "Node");
                let metrics = self.node_metrics(backend);
                for (label, value) in &metrics {
                    draw_metric_row(ui, label, value, self.colors.text_muted, self.colors.text);
                }

                // ── Peers ───────────────────────────────
                ui.add_space(16.0);
                self.draw_section_heading(
                    ui,
                    &format!("Connected Peers ({})", self.node_status.peers.len()),
                );
                if !self.node_status.peers.is_empty() {
                    self.draw_peer_table(ui);
                }

                // ── Tracked Scripts (LC only) ──────────
                if backend == NodeType::LightClient
                    && !self.node_status.tracked_scripts.is_empty()
                {
                    ui.add_space(16.0);
                    self.draw_tracked_scripts_section(ui);
                }

                if let Some(backend) = switch_to {
                    self.switch_to_backend(backend);
                }

                ui.add_space(20.0);
            });
        });
    }

    fn backend_accent(&self, _backend: NodeType) -> egui::Color32 {
        egui::Color32::from_rgb(0, 229, 255)
    }

    fn backend_label(backend: NodeType) -> (&'static str, &'static str) {
        match backend {
            NodeType::LightClient => ("\u{1F4A1}", "Light Client"),
            NodeType::FullNode => ("\u{1F5A5}", "Full Node"),
            NodeType::PublicRpc => ("\u{1F310}", "Public RPC"),
        }
    }

    fn draw_network_toggle(&mut self, ui: &mut egui::Ui) {
        let current_network = self.qp_client.config().network;
        let mut selected = current_network;

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
            let resp = ui.interact(seg_rect, ui.id().with(("net-seg", i)), egui::Sense::click());

            if is_active {
                painter.rect_filled(
                    seg_rect,
                    5.0,
                    egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 25),
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

        if selected != current_network {
            self.network = selected;
            self.node_type = self.qp_client.config().node_type;
            if self.node_type == NodeType::PublicRpc {
                self.settings_rpc_url =
                    NodeConfig::default_rpc_url_for(self.node_type, self.network)
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
        self.node_type = backend;
        self.network = current.network;
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
        if self.network != old_network {
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
                match (self.node_status.synced_block, self.node_status.tip_block()) {
                    (Some(s), Some(t)) if t > 0 => (s as f64 / t as f64).clamp(0.0, 1.0) as f32,
                    _ => 0.0,
                }
            }
            NodeType::FullNode => full_node_sync_view(self.node_status.sync_state.as_ref()).0,
            NodeType::PublicRpc => 0.0,
        }
    }

    // ── Tab bar ──────────────────────────────────────────────

    fn draw_tab_bar(&mut self, ui: &mut egui::Ui) -> Option<NodeType> {
        let backends = [
            NodeType::LightClient,
            NodeType::FullNode,
            NodeType::PublicRpc,
        ];
        let current = self.qp_client.config().node_type;
        let mut switch_to = None;

        let pad = 4.0;
        let gap = 4.0;
        let seg_h = 36.0;
        let total_w = ui.available_width();
        let bar_h = seg_h + pad * 2.0;
        let seg_w =
            (total_w - pad * 2.0 - gap * (backends.len() as f32 - 1.0)) / backends.len() as f32;

        let (outer_rect, _) =
            ui.allocate_exact_size(egui::vec2(total_w, bar_h), egui::Sense::hover());
        let painter = ui.painter();

        painter.rect_filled(outer_rect, 10.0, self.colors.surface);
        painter.rect_stroke(
            outer_rect,
            10.0,
            egui::Stroke::new(1.0, self.colors.border),
            egui::StrokeKind::Inside,
        );

        for (i, &backend) in backends.iter().enumerate() {
            let active = current == backend;
            let accent = self.backend_accent(backend);
            let (icon, name) = Self::backend_label(backend);

            let x = outer_rect.left() + pad + i as f32 * (seg_w + gap);
            let seg_rect = egui::Rect::from_min_size(
                egui::pos2(x, outer_rect.top() + pad),
                egui::vec2(seg_w, seg_h),
            );

            let resp = ui.interact(seg_rect, ui.id().with(("tab", i)), egui::Sense::click());

            if active {
                painter.rect_filled(
                    seg_rect,
                    7.0,
                    egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30),
                );
            } else if resp.hovered() {
                painter.rect_filled(
                    seg_rect,
                    7.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                );
            }

            let text_color = if active {
                accent
            } else {
                self.colors.text_muted
            };

            let (pill_text, pill_bg, pill_fg) = self.status_pill_data(backend, active, accent);
            let pill_galley = painter.layout_no_wrap(
                pill_text.to_string(),
                egui::FontId::new(8.0, egui::FontFamily::Monospace),
                pill_fg,
            );

            let sync_pill: Option<(String, egui::Color32, egui::Color32)> =
                if active && self.node_status.online && backend != NodeType::PublicRpc {
                    let pct = self.sync_pct(backend);
                    let tint = egui::Color32::from_rgba_unmultiplied(
                        accent.r(),
                        accent.g(),
                        accent.b(),
                        38,
                    );
                    Some((format!("{:.1}%", pct * 100.0), tint, accent))
                } else {
                    None
                };

            let label_text = format!("{} {}", icon, name);
            let label_galley =
                painter.layout_no_wrap(label_text, egui::FontId::proportional(12.0), text_color);

            let pill_hpad = 5.0;
            let pill_w = pill_galley.rect.width() + pill_hpad * 2.0;
            let pill_h = pill_galley.rect.height() + 4.0;
            let inner_gap = 6.0;

            let sync_pill_galley = sync_pill.as_ref().map(|(text, _, fg)| {
                painter.layout_no_wrap(
                    text.clone(),
                    egui::FontId::new(8.0, egui::FontFamily::Monospace),
                    *fg,
                )
            });
            let sync_pill_w = sync_pill_galley
                .as_ref()
                .map(|g| g.rect.width() + pill_hpad * 2.0 + inner_gap)
                .unwrap_or(0.0);

            let total_inner = label_galley.rect.width() + inner_gap + pill_w + sync_pill_w;
            let start_x = seg_rect.center().x - total_inner / 2.0;
            let cy = seg_rect.center().y;

            painter.galley(
                egui::pos2(start_x, cy - label_galley.rect.height() / 2.0),
                label_galley.clone(),
                text_color,
            );

            let pill_x = start_x + label_galley.rect.width() + inner_gap;
            let pill_rect = egui::Rect::from_min_size(
                egui::pos2(pill_x, cy - pill_h / 2.0),
                egui::vec2(pill_w, pill_h),
            );
            painter.rect_filled(pill_rect, 4.0, pill_bg);
            painter.galley(
                egui::pos2(
                    pill_rect.center().x - pill_galley.rect.width() / 2.0,
                    pill_rect.center().y - pill_galley.rect.height() / 2.0,
                ),
                pill_galley,
                pill_fg,
            );

            if let Some((_, sync_bg, sync_fg)) = &sync_pill {
                if let Some(sync_galley) = sync_pill_galley {
                    let sx = pill_rect.right() + inner_gap;
                    let sw = sync_galley.rect.width() + pill_hpad * 2.0;
                    let sync_rect = egui::Rect::from_min_size(
                        egui::pos2(sx, cy - pill_h / 2.0),
                        egui::vec2(sw, pill_h),
                    );
                    painter.rect_filled(sync_rect, 4.0, *sync_bg);
                    painter.galley(
                        egui::pos2(
                            sync_rect.center().x - sync_galley.rect.width() / 2.0,
                            sync_rect.center().y - sync_galley.rect.height() / 2.0,
                        ),
                        sync_galley,
                        *sync_fg,
                    );
                }
            }

            if pill_text.contains("STARTING") {
                let spinner_size = pill_h - 4.0;
                let spinner_rect = egui::Rect::from_center_size(
                    egui::pos2(pill_rect.left() + 8.0, pill_rect.center().y),
                    egui::vec2(spinner_size, spinner_size),
                );
                egui::Spinner::new()
                    .size(spinner_size)
                    .color(pill_fg)
                    .paint_at(ui, spinner_rect);
            }

            if resp.hovered() && !active {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && !active {
                switch_to = Some(backend);
            }
        }

        switch_to
    }

    fn status_pill_data(
        &self,
        backend: NodeType,
        active: bool,
        accent: egui::Color32,
    ) -> (&'static str, egui::Color32, egui::Color32) {
        if !active {
            (
                "\u{25CB} STANDBY",
                self.colors.surface2,
                self.colors.text_muted,
            )
        } else if self.node_status.online {
            let tint =
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 38);
            ("\u{25CF} ONLINE", tint, accent)
        } else if backend != NodeType::PublicRpc && self.local_node.has_local_process() {
            ("   STARTING", self.colors.warn_tint, self.colors.warn)
        } else {
            (
                "\u{25CB} OFFLINE",
                egui::Color32::from_rgba_unmultiplied(255, 77, 109, 30),
                self.colors.danger,
            )
        }
    }

    // ── Section heading ──────────────────────────────────────

    fn draw_section_heading(&self, ui: &mut egui::Ui, title: &str) {
        ui.label(
            egui::RichText::new(title)
                .size(16.0)
                .strong()
                .color(self.colors.text),
        );
        ui.add_space(6.0);
    }

    fn draw_tracked_scripts_section(&self, ui: &mut egui::Ui) {
        let scripts = &self.node_status.tracked_scripts;
        self.draw_section_heading(
            ui,
            &format!("Tracked Scripts ({})", scripts.len()),
        );

        // Build a map: wallet_id -> (wallet_name, Vec<lock_args>)
        let mut wallet_accounts: Vec<(String, Vec<String>)> = Vec::new();
        for cw in &self.wallet_cache {
            let accounts =
                qpv2_core::KeyVault::get_all_lock_args(cw.id).unwrap_or_default();
            wallet_accounts.push((cw.name.clone(), accounts));
        }

        // Classify each script into a wallet or "Orphaned"
        struct ScriptEntry {
            args_short: String,
            block_number: u64,
            account_idx: Option<usize>,
        }

        let mut grouped: std::collections::BTreeMap<String, Vec<ScriptEntry>> =
            std::collections::BTreeMap::new();

        for (args_hex, block) in scripts {
            let mut found = false;
            for (wallet_name, accounts) in &wallet_accounts {
                if let Some(idx) = accounts.iter().position(|a| a == args_hex) {
                    grouped
                        .entry(wallet_name.clone())
                        .or_default()
                        .push(ScriptEntry {
                            args_short: args_hex.clone(),
                            block_number: *block,
                            account_idx: Some(idx),
                        });
                    found = true;
                    break;
                }
            }
            if !found {
                grouped
                    .entry("Orphaned".to_string())
                    .or_default()
                    .push(ScriptEntry {
                        args_short: args_hex.clone(),
                        block_number: *block,
                        account_idx: None,
                    });
            }
        }

        let muted = self.colors.text_muted;
        let text = self.colors.text;

        // Flatten into a single list with wallet name for the grid
        struct FlatEntry {
            wallet_name: String,
            wallet_color: egui::Color32,
            account_idx: Option<usize>,
            args: String,
            block_number: u64,
        }

        let mut flat: Vec<FlatEntry> = Vec::new();
        for (wallet_name, entries) in &mut grouped {
            entries.sort_by_key(|e| e.account_idx.unwrap_or(usize::MAX));
            let color = if wallet_name == "Orphaned" {
                self.colors.danger
            } else {
                self.colors.accent2
            };
            for entry in entries.iter() {
                flat.push(FlatEntry {
                    wallet_name: wallet_name.clone(),
                    wallet_color: color,
                    account_idx: entry.account_idx,
                    args: entry.args_short.clone(),
                    block_number: entry.block_number,
                });
            }
        }

        egui::Grid::new("tracked-scripts-grid")
            .num_columns(4)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for label in ["Wallet", "Account", "Lock Script Args", "Synced Block"] {
                    ui.label(
                        egui::RichText::new(label)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                }
                ui.end_row();

                let mut prev_wallet = String::new();
                for entry in &flat {
                    let wallet_label = if entry.wallet_name != prev_wallet {
                        prev_wallet = entry.wallet_name.clone();
                        entry.wallet_name.clone()
                    } else {
                        "\u{2502}".to_string()
                    };
                    ui.label(
                        egui::RichText::new(wallet_label)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .strong()
                            .color(entry.wallet_color),
                    );
                    let idx_str = match entry.account_idx {
                        Some(idx) => format!("#{}", idx),
                        None => "?".to_string(),
                    };
                    ui.label(
                        egui::RichText::new(idx_str)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(text),
                    );
                    ui.label(
                        egui::RichText::new(&entry.args)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "#{}",
                            crate::utils::format_with_commas(entry.block_number)
                        ))
                        .size(10.0)
                        .family(egui::FontFamily::Monospace)
                        .color(text),
                    );
                    ui.end_row();
                }
            });
    }

    // ── Node metrics ─────────────────────────────────────────

    fn node_metrics(&self, backend: NodeType) -> Vec<(&'static str, String)> {
        let node = self.node_status.local_node_info.as_ref();
        let version = node
            .map(|n| n.version.clone())
            .unwrap_or_else(|| DASH.into());
        let protocols = node
            .map(|n| {
                n.protocols
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|| DASH.into());
        let node_id = node
            .map(|n| n.node_id.clone())
            .unwrap_or_else(|| DASH.into());

        let mut m = match backend {
            NodeType::PublicRpc => {
                let url = self.qp_client.config().rpc_url.clone();
                vec![
                    ("Endpoint", hostname_of(&url)),
                    ("Port", port_text(self.node_status.rpc_port)),
                ]
            }
            NodeType::LightClient | NodeType::FullNode => {
                vec![("RPC Port", port_text(self.node_status.rpc_port))]
            }
        };
        m.push(("Version", version));
        m.push(("Node ID", node_id));
        m.push(("Protocols", protocols));
        m
    }

    fn draw_sync_edit_inline(&mut self, ui: &mut egui::Ui, accent: egui::Color32) {
        let tip = self.node_status.tip_block();

        let response = ui.add(
            egui::TextEdit::singleline(&mut self.set_block_input)
                .desired_width(80.0)
                .font(egui::FontId::monospace(12.0))
                .text_color(accent),
        );

        let parsed = self.set_block_input.trim().replace(',', "").parse::<u64>();
        let valid = matches!(&parsed, Ok(b) if tip.is_none_or(|t| *b <= t));

        let ok_color = if valid {
            self.colors.accent
        } else {
            self.colors.text_muted
        };
        let ok_btn = ui.add_enabled(
            valid,
            egui::Button::new(egui::RichText::new("\u{2713} ok").size(12.0).color(ok_color))
                .fill(egui::Color32::TRANSPARENT),
        );

        let cancel_btn = ui.add(
            egui::Button::new(
                egui::RichText::new("\u{2715} cancel")
                    .size(12.0)
                    .color(self.colors.text_muted),
            )
            .fill(egui::Color32::TRANSPARENT),
        );

        let auto_enabled =
            self.earliest_funding_block_rx.is_none() && !self.accounts.is_empty();
        let auto_label = if self.earliest_funding_block_rx.is_some() {
            "\u{2699} auto..."
        } else {
            "\u{2699} auto"
        };
        let auto_btn = ui.add_enabled(
            auto_enabled,
            egui::Button::new(egui::RichText::new(auto_label).size(12.0).color(
                if auto_enabled {
                    self.colors.accent2
                } else {
                    self.colors.text_muted
                },
            ))
            .fill(egui::Color32::TRANSPARENT),
        );

        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if (ok_btn.clicked() || enter) && valid {
            if let Ok(block) = parsed {
                self.set_all_accounts_lock_script_block(block);
                self.set_block_editing = false;
                self.set_block_input.clear();
            }
        } else if cancel_btn.clicked() || escape {
            self.set_block_editing = false;
            self.set_block_input.clear();
        } else if auto_btn.clicked() {
            self.detect_earliest_funding_block_async();
        }
    }

    // ── Blockchain metrics ───────────────────────────────────

    fn draw_blockchain_metrics(&mut self, ui: &mut egui::Ui) {
        let backend = self.qp_client.config().node_type;
        let muted = self.colors.text_muted;
        let text = self.colors.text;

        // Pre-compute info-dependent values as owned strings so the
        // borrow of blockchain_info doesn't conflict with &mut self below.
        let info = self.node_status.blockchain_info.as_deref();
        let chain = info.map(|i| i.chain.clone()).unwrap_or_else(|| DASH.into());
        let difficulty = info
            .map(|i| format!("{:#x}", i.difficulty))
            .unwrap_or_else(|| DASH.into());
        let ibd = info
            .map(|i| if i.is_initial_block_download { "Yes" } else { "No" })
            .unwrap_or(DASH)
            .to_string();
        let median = info
            .map(|i| format_timestamp_ms(i.median_time.value()))
            .unwrap_or_else(|| DASH.into());
        let tip_label = match backend {
            NodeType::PublicRpc => "Block Height",
            NodeType::FullNode => "Local Tip",
            NodeType::LightClient => "Tip",
        };
        let tip_value = match backend {
            NodeType::PublicRpc => block_height_text(self.node_status.tip_block()),
            _ => target_tip_value(backend, &self.node_status),
        };
        draw_metric_row(ui, tip_label, &tip_value, muted, text);

        // Synced block (LC and FN only)
        if backend != NodeType::PublicRpc {
            if backend == NodeType::LightClient && self.set_block_editing {
                let accent = self.backend_accent(backend);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:<width$}", "Synced", width = LABEL_PAD))
                            .size(12.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                    self.draw_sync_edit_inline(ui, accent);
                });
            } else {
                let synced_value = match backend {
                    NodeType::LightClient => self
                        .node_status
                        .synced_block
                        .map(|b| format!("#{}", crate::utils::format_with_commas(b)))
                        .unwrap_or_else(|| DASH.into()),
                    NodeType::FullNode => self
                        .node_status
                        .sync_state
                        .as_ref()
                        .map(|s| {
                            format!("#{}", crate::utils::format_with_commas(s.tip_number.value()))
                        })
                        .unwrap_or_else(|| DASH.into()),
                    NodeType::PublicRpc => unreachable!(),
                };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:<width$}", "Synced", width = LABEL_PAD))
                            .size(12.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&synced_value)
                                .size(12.0)
                                .family(egui::FontFamily::Monospace)
                                .strong()
                                .color(text),
                        )
                        .selectable(true),
                    );
                    if backend == NodeType::LightClient {
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
                            self.set_block_input = self
                                .node_status
                                .synced_block
                                .map(|b| b.to_string())
                                .unwrap_or_default();
                        }
                    }
                });
            }
        }

        let epoch = self
            .node_status
            .tip_header
            .as_ref()
            .map(|h| crate::utils::format_with_commas(h.epoch().number()))
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Epoch", &epoch, muted, text);

        draw_metric_row(ui, "Chain", &chain, muted, text);
        draw_metric_row(ui, "Difficulty", &difficulty, muted, text);
        draw_metric_row(ui, "IBD", &ibd, muted, text);
        draw_metric_row(ui, "Network Time", &median, muted, text);
    }

    // ── Tx Pool metrics ──────────────────────────────────────

    fn draw_tx_pool_metrics(&self, ui: &mut egui::Ui) {
        let pool = self.node_status.tx_pool_info.as_ref();
        let muted = self.colors.text_muted;
        let text = self.colors.text;

        let pending = pool
            .map(|p| crate::utils::format_with_commas(p.pending.value()))
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Pending", &pending, muted, text);

        let proposed = pool
            .map(|p| crate::utils::format_with_commas(p.proposed.value()))
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Proposed", &proposed, muted, text);

        let orphan = pool
            .map(|p| crate::utils::format_with_commas(p.orphan.value()))
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Orphan", &orphan, muted, text);

        let fee = pool
            .map(|p| {
                format!(
                    "{} sh/KB",
                    crate::utils::format_with_commas(p.min_fee_rate.value())
                )
            })
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Min Fee", &fee, muted, text);

        let cycles = pool
            .map(|p| crate::utils::format_with_commas(p.total_tx_cycles.value()))
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Cycles", &cycles, muted, text);

        let size = pool
            .map(|p| {
                format!(
                    "{} B",
                    crate::utils::format_with_commas(p.total_tx_size.value())
                )
            })
            .unwrap_or_else(|| DASH.into());
        draw_metric_row(ui, "Tx Size", &size, muted, text);
    }

    // ── Peer table ───────────────────────────────────────────

    fn draw_peer_table(&self, ui: &mut egui::Ui) {
        let muted = self.colors.text_muted;
        let text = self.colors.text;

        egui::Grid::new("peer-grid")
            .num_columns(4)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for label in ["Node ID", "Version", "Direction", "Ping"] {
                    ui.label(
                        egui::RichText::new(label)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                }
                ui.end_row();

                for peer in &self.node_status.peers {
                    ui.label(
                        egui::RichText::new(&peer.node_id)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(text),
                    );
                    ui.label(
                        egui::RichText::new(&peer.version)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                    let dir = if peer.is_outbound {
                        "Outbound"
                    } else {
                        "Inbound"
                    };
                    ui.label(
                        egui::RichText::new(dir)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                    let ping = peer
                        .last_ping_duration
                        .as_ref()
                        .map(|d| format!("{}ms", d.value()))
                        .unwrap_or_else(|| DASH.into());
                    ui.label(
                        egui::RichText::new(ping)
                            .size(10.0)
                            .family(egui::FontFamily::Monospace)
                            .color(muted),
                    );
                    ui.end_row();
                }
            });
    }
}

// ── Free functions ───────────────────────────────────────────

fn draw_metric_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    muted: egui::Color32,
    text: egui::Color32,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{:<width$}", label, width = LABEL_PAD))
                .size(12.0)
                .family(egui::FontFamily::Monospace)
                .color(muted),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(value)
                    .size(12.0)
                    .family(egui::FontFamily::Monospace)
                    .strong()
                    .color(text),
            )
            .wrap(),
        );
    });
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


fn target_tip_value(backend: NodeType, status: &crate::types::NodeStatus) -> String {
    match backend {
        NodeType::FullNode => match status.sync_state.as_ref() {
            Some(s) => format!(
                "#{}",
                crate::utils::format_with_commas(s.best_known_block_number.value())
            ),
            None => "\u{2014}".into(),
        },
        NodeType::LightClient => match status.tip_block() {
            Some(t) => format!("#{}", crate::utils::format_with_commas(t)),
            None => "\u{2014}".into(),
        },
        NodeType::PublicRpc => "\u{2014}".into(),
    }
}

fn block_height_text(tip: Option<u64>) -> String {
    tip.map(|n| format!("#{}", crate::utils::format_with_commas(n)))
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

fn format_timestamp_ms(ms: u64) -> String {
    chrono::DateTime::from_timestamp((ms / 1000) as i64, 0)
        .map(|dt| dt.format("%b %d %Y %H:%M").to_string())
        .unwrap_or_else(|| DASH.into())
}
