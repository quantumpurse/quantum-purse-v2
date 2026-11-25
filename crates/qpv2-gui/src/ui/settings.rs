//! Settings tab: backend selection, the wallet's colors and live node
//! instrumentation.

use ckb_node::{NetworkType, NodeType};
use eframe::egui;

use crate::palette::{self, ColorRole};
use crate::types::{display_font, label_font, AppColors, Status};
use crate::ui::utils::{
    data_row, data_row_colored, ghost_button, group_thousands, lerp_color, panel_frame, row_hover,
    section_header, truncate_middle_to, truncate_tail, value_flash, SectionCounter,
};
use crate::App;

const DASH: &str = "\u{2014}";

impl App {
    pub(crate) fn show_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 24.0);

                // ── Screen header ────────────────────────────
                // While a foreign node holds the port, this page reports
                // that node instead of one QPV2 manages
                let foreign_node = self.foreign_node();
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("SETTINGS")
                                    .font(display_font(16.0))
                                    .color(self.colors.text),
                            );
                            if let Some((backend, network)) = &foreign_node {
                                let backend = match backend {
                                    NodeType::LightClient => "light node",
                                    NodeType::FullNode => "full node",
                                    NodeType::PublicRpc => "remote node",
                                };
                                let network = match network.as_str() {
                                    "Mainnet" => "mainnet",
                                    "Testnet" => "testnet",
                                    other => other,
                                };
                                ui.label(
                                    egui::RichText::new(format!(
                                        "(foreign - {} {})",
                                        backend, network
                                    ))
                                    .font(display_font(12.0))
                                    .color(self.colors.warn),
                                );
                            }
                        });
                        ui.add_space(2.0);
                        let subtitle = if foreign_node.is_some() {
                            "Set the wallet's colors, and configure and monitor foreign CKB node."
                        } else {
                            "Set the wallet's colors, and configure and monitor your CKB node."
                        };
                        ui.label(
                            egui::RichText::new(subtitle)
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                    });
                });

                ui.add_space(14.0);

                // Sections are numbered in render order because two of
                // them are backend-conditional; fixed codes would leave
                // gaps whenever one is skipped.
                let mut sec = SectionCounter::new();

                // ── Connection ───────────────────────────────
                let mut switch_to = None;
                panel_frame(&self.colors).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    section_header(ui, &self.colors, &sec.next_code(), "Connection");
                    ui.add_space(6.0);
                    // A foreign node decides the network, so there is none
                    // to choose; picking another backend is the way out.
                    if foreign_node.is_none() {
                        self.draw_network_row(ui);
                    }
                    switch_to = self.draw_backend_row(ui);
                });

                let backend = self.qp_client.config().node_type;

                // ── Chain ────────────────────────────────────
                ui.add_space(12.0);
                panel_frame(&self.colors).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    section_header(ui, &self.colors, &sec.next_code(), "Chain");
                    ui.add_space(6.0);
                    self.draw_blockchain_metrics(ui);
                });

                // ── Tx Pool (not available on LC) ────────────
                if backend != NodeType::LightClient {
                    ui.add_space(12.0);
                    panel_frame(&self.colors).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        section_header(ui, &self.colors, &sec.next_code(), "Tx Pool");
                        ui.add_space(6.0);
                        self.draw_tx_pool_metrics(ui);
                    });
                }

                // ── Node ─────────────────────────────────────
                ui.add_space(12.0);
                panel_frame(&self.colors).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    section_header(ui, &self.colors, &sec.next_code(), "Node");
                    ui.add_space(6.0);
                    self.draw_node_metrics(ui, backend);
                });

                // ── Peers ────────────────────────────────────
                ui.add_space(12.0);
                panel_frame(&self.colors).show(ui, |ui| {
                    section_header(
                        ui,
                        &self.colors,
                        &sec.next_code(),
                        &format!("Peers ({})", self.node_status.peers.len()),
                    );
                    if !self.node_status.peers.is_empty() {
                        ui.add_space(6.0);
                        self.draw_peer_table(ui);
                    }
                });

                // ── Tracked Scripts (LC only) ────────────────
                if backend == NodeType::LightClient && !self.node_status.tracked_scripts.is_empty()
                {
                    ui.add_space(12.0);
                    panel_frame(&self.colors).show(ui, |ui| {
                        self.draw_tracked_scripts_section(ui, &sec.next_code());
                    });
                }

                // ── Colors ───────────────────────────────────
                ui.add_space(12.0);
                panel_frame(&self.colors).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    self.draw_colors_section(ui, &sec.next_code());
                });

                // Applied after rendering so the frame draws against the
                // pre-switch state instead of half-updated config.
                if let Some(backend) = switch_to {
                    let network = self.qp_client.config().network;
                    self.switch_node(backend, network);
                }

                ui.add_space(20.0);
            });
        });
    }

    fn backend_label(backend: NodeType) -> &'static str {
        match backend {
            NodeType::LightClient => "LIGHT CLIENT",
            NodeType::FullNode => "FULL NODE",
            NodeType::PublicRpc => "PUBLIC RPC",
        }
    }

    // ── Connection rows ──────────────────────────────────────

    /// NETWORK: Mainnet or Testnet, as a row of the Connection panel.
    fn draw_network_row(&mut self, ui: &mut egui::Ui) {
        let current_network = self.qp_client.config().network;
        let networks = [NetworkType::Mainnet, NetworkType::Testnet];
        let mut selected = current_network;
        setting_row(ui, &self.colors, "Network", |ui| {
            let chosen = networks
                .iter()
                .position(|&n| n == current_network)
                .unwrap_or(0);
            if let Some(i) = text_choice(ui, &self.colors, &["MAINNET", "TESTNET"], Some(chosen)) {
                selected = networks[i];
            }
        });

        if selected != current_network {
            let node_type = self.qp_client.config().node_type;
            self.switch_node(node_type, selected);
        }
    }

    pub(crate) fn sync_pct(&self, backend: NodeType) -> f32 {
        match backend {
            NodeType::LightClient => {
                match (self.node_status.synced_block, self.node_status.tip_block()) {
                    (Some(s), Some(t)) if t > 0 => (s as f64 / t as f64).clamp(0.0, 1.0) as f32,
                    _ => 0.0,
                }
            }
            NodeType::FullNode => full_node_sync_pct(self.node_status.sync_state.as_ref()),
            NodeType::PublicRpc => 0.0,
        }
    }

    /// BACKEND: which node the wallet talks to, as a row of the Connection
    /// panel. Returns the one picked this frame, applied by the caller after
    /// the frame is drawn.
    fn draw_backend_row(&self, ui: &mut egui::Ui) -> Option<NodeType> {
        let current = self.qp_client.config().node_type;
        let backends = [
            NodeType::LightClient,
            NodeType::FullNode,
            NodeType::PublicRpc,
        ];
        let mut switch_to = None;
        setting_row(ui, &self.colors, "Backend", |ui| {
            let chosen = backends.iter().position(|&b| b == current).unwrap_or(0);
            let labels = backends.map(Self::backend_label);
            if let Some(i) = text_choice(ui, &self.colors, &labels, Some(chosen)) {
                switch_to = Some(backends[i]);
            }
        });
        switch_to
    }

    // ── Colors ───────────────────────────────────────────────

    /// The wallet's colors: LIGHT and DARK, then a row per named
    /// color with a swatch that opens a picker and the hex code, which can
    /// be typed or pasted. A change shows at once across the app and is
    /// saved when it settles.
    fn draw_colors_section(&mut self, ui: &mut egui::Ui, code: &str) {
        section_header(ui, &self.colors, code, "Colors");
        ui.add_space(6.0);
        let mut colors = self.colors;
        setting_row(ui, &self.colors, "Palette", |ui| {
            // A theme is marked as chosen only while every color is its
            // own: one changed by hand makes it neither.
            let chosen = if colors == AppColors::light() {
                Some(0)
            } else if colors == AppColors::default() {
                Some(1)
            } else {
                None
            };
            if let Some(i) = text_choice(ui, &self.colors, &["LIGHT", "DARK"], chosen) {
                colors = if i == 0 {
                    AppColors::light()
                } else {
                    AppColors::default()
                };
            }
        });
        for role in ColorRole::ALL {
            setting_row(ui, &self.colors, role.label(), |ui| {
                if let Some(c) = color_control(ui, &self.colors, role, role.get(&colors)) {
                    role.set(&mut colors, c);
                }
            });
        }
        // Hand-picked colors aren't held to any contrast, so say when the
        // text has become hard to read, and how to get back.
        let ratio = palette::contrast(colors.text, colors.bg);
        if ratio < 4.5 {
            ui.label(
                egui::RichText::new(format!(
                    "Text is hard to read on this background ({ratio:.1}:1). \
                     LIGHT or DARK brings back a readable theme."
                ))
                .size(11.0)
                .color(colors.warn),
            );
        }

        if colors != self.colors {
            self.colors = colors;
            crate::apply_visuals(ui.ctx(), &colors);
        }
        // While a picker is dragged every frame is a change, so save once
        // the pointer lets go.
        if self.colors != self.saved_colors && !ui.input(|i| i.pointer.any_down()) {
            if let Err(e) = palette::save_colors(&self.colors) {
                let msg = format!("Couldn't save the colors: {e}");
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
            }
            // Not retried every frame after a failure: the next change is.
            self.saved_colors = self.colors;
        }
    }

    /// Detects a node QPV2 did not start answering on its port: a local
    /// backend is selected, no child of its own is running, and the last
    /// poll reached a node on either chain. Returns that node's backend
    /// and network, or `None` when there is no such node.
    pub(crate) fn foreign_node(&self) -> Option<(NodeType, String)> {
        let cfg = self.qp_client.config();
        if cfg.node_type == NodeType::PublicRpc || self.local_node.has_local_process() {
            return None;
        }

        // node_status is wired to the port. So when no local node(and not public rpc) is running,
        // here we check if the network on the port is the same as the configured network.
        let foreign_network = match &self.node_status.mismatched_network {
            // foreign node but different than the qp's configured network.
            Some(detected) => detected.clone(),
            // foreign node and the same as the qp's configured network.
            None if self.node_status.online => cfg.network.to_string(),
            // no foreign node detected on the port.
            None => return None,
        };
        Some((cfg.node_type, foreign_network))
    }

    // ── Tracked scripts ──────────────────────────────────────

    fn draw_tracked_scripts_section(&self, ui: &mut egui::Ui, code: &str) {
        let scripts = &self.node_status.tracked_scripts;
        section_header(
            ui,
            &self.colors,
            code,
            &format!("Tracked Scripts ({})", scripts.len()),
        );
        ui.add_space(6.0);

        // Build a map: wallet_id -> (wallet_name, Vec<lock_args>)
        let mut wallet_accounts: Vec<(String, Vec<String>)> = Vec::new();
        for cw in &self.wallet_cache {
            let accounts = qpv2_core::KeyVault::get_all_lock_args(cw.id).unwrap_or_default();
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

        // Flatten into a single list with wallet name for the table
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
            // Orphaned scripts are an error state; owned ones stay
            // neutral (green is reserved for online/synced).
            let color = if wallet_name == "Orphaned" {
                self.colors.danger
            } else {
                self.colors.text
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

        let c_border = self.colors.border;
        let c_muted = self.colors.text_muted;
        let w = ui.available_width();
        let col_acct = 0.24 * w;
        let col_args = 0.34 * w;
        // ~6.5px per glyph at 10.5pt mono; leave room for the synced column.
        let args_chars = (((w - col_args) - 120.0) / 6.5).max(12.0) as usize;

        let (hr, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
        let painter = ui.painter();
        for (label, x) in [
            ("WALLET", 6.0),
            ("ACCOUNT", col_acct),
            ("LOCK SCRIPT ARGS", col_args),
        ] {
            painter.text(
                egui::pos2(hr.left() + x, hr.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                label_font(9.0),
                c_muted,
            );
        }
        painter.text(
            egui::pos2(hr.right(), hr.center().y),
            egui::Align2::RIGHT_CENTER,
            "SYNCED BLOCK",
            label_font(9.0),
            c_muted,
        );
        painter.hline(
            hr.x_range(),
            hr.bottom() - 0.5,
            egui::Stroke::new(1.0, c_border),
        );

        let mut prev_wallet = String::new();
        for entry in &flat {
            let (rr, resp) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            let painter = ui.painter();
            if resp.hovered() {
                row_hover(painter, rr, &self.colors);
            }
            let cy = rr.center().y;

            let wallet_label = if entry.wallet_name != prev_wallet {
                prev_wallet = entry.wallet_name.clone();
                entry.wallet_name.clone()
            } else {
                "\u{2502}".to_string()
            };
            painter.text(
                egui::pos2(rr.left() + 6.0, cy),
                egui::Align2::LEFT_CENTER,
                wallet_label,
                egui::FontId::proportional(10.5),
                entry.wallet_color,
            );
            let idx_str = match entry.account_idx {
                Some(idx) => format!("#{}", idx),
                None => "?".to_string(),
            };
            painter.text(
                egui::pos2(rr.left() + col_acct, cy),
                egui::Align2::LEFT_CENTER,
                idx_str,
                egui::FontId::proportional(10.5),
                self.colors.text,
            );
            painter.text(
                egui::pos2(rr.left() + col_args, cy),
                egui::Align2::LEFT_CENTER,
                truncate_middle_to(&entry.args, args_chars),
                egui::FontId::proportional(10.5),
                c_muted,
            );
            painter.text(
                egui::pos2(rr.right(), cy),
                egui::Align2::RIGHT_CENTER,
                format!("#{}", group_thousands(entry.block_number)),
                egui::FontId::proportional(10.5),
                self.colors.text,
            );
            painter.hline(
                rr.x_range(),
                rr.bottom() - 0.5,
                egui::Stroke::new(1.0, c_border),
            );
        }
    }

    // ── Node metrics ─────────────────────────────────────────

    fn draw_node_metrics(&self, ui: &mut egui::Ui, backend: NodeType) {
        for (label, value) in self.node_metrics(backend) {
            if label == "Protocols" {
                // Protocol lists run long; stack the value under the
                // label so it can wrap instead of colliding with it.
                ui.label(
                    egui::RichText::new("PROTOCOLS")
                        .font(label_font(9.5))
                        .color(self.colors.text_muted),
                );
                ui.label(
                    egui::RichText::new(value)
                        .size(11.0)
                        .color(self.colors.text),
                );
            } else {
                data_row(ui, &self.colors, label, &value);
            }
        }
    }

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

    fn draw_sync_edit_inline(&mut self, ui: &mut egui::Ui) {
        let tip = self.node_status.tip_block();

        let response = ui.add(
            egui::TextEdit::singleline(&mut self.set_block_input)
                .desired_width(90.0)
                .font(egui::FontId::monospace(12.0))
                .text_color(self.colors.accent),
        );

        let parsed = self.set_block_input.trim().replace(',', "").parse::<u64>();
        let valid = matches!(&parsed, Ok(b) if tip.is_none_or(|t| *b <= t));

        let btn_size = egui::vec2(58.0, 20.0);
        let set_btn = ui.add_enabled(valid, ghost_button(&self.colors, "SET", btn_size));
        let cancel_btn = ui.add(ghost_button(&self.colors, "CANCEL", btn_size));

        let auto_running = self.earliest_funding_block_rx.is_some();
        let auto_enabled = auto_running.eq(&false) && !self.accounts.is_empty();
        let auto_label = if auto_running { "AUTO..." } else { "AUTO" };
        let auto_btn = ui.add_enabled(
            auto_enabled,
            ghost_button(&self.colors, auto_label, btn_size),
        );

        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if (set_btn.clicked() || enter) && valid {
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
        let c_text = self.colors.text;
        let c_muted = self.colors.text_muted;

        // Pre-compute info-dependent values as owned strings so the
        // borrow of blockchain_info doesn't conflict with &mut self below.
        let info = self.node_status.blockchain_info.as_deref();
        let chain = info.map(|i| i.chain.clone()).unwrap_or_else(|| DASH.into());
        let difficulty = info
            .map(|i| format!("{:#x}", i.difficulty))
            .unwrap_or_else(|| DASH.into());
        let ibd = info
            .map(|i| {
                if i.is_initial_block_download {
                    "Yes"
                } else {
                    "No"
                }
            })
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
        // Tip flash mirrors the telemetry strip: accent flash ~1s on change.
        let tip_raw = match backend {
            NodeType::FullNode => self
                .node_status
                .sync_state
                .as_ref()
                .map(|s| s.best_known_block_number.value())
                .unwrap_or(0),
            _ => self.node_status.tip_block().unwrap_or(0),
        };
        let flash = value_flash(ui, egui::Id::new("nm-tip-flash"), tip_raw);
        data_row_colored(
            ui,
            &self.colors,
            tip_label,
            &tip_value,
            lerp_color(c_text, self.colors.accent, flash),
        );

        // Synced block (LC and FN only)
        if backend != NodeType::PublicRpc {
            if backend == NodeType::LightClient && self.set_block_editing {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("SYNCED")
                            .font(label_font(9.5))
                            .color(c_muted),
                    );
                    self.draw_sync_edit_inline(ui);
                });
            } else {
                let synced_value = match backend {
                    NodeType::LightClient => self
                        .node_status
                        .synced_block
                        .map(|b| format!("#{}", group_thousands(b)))
                        .unwrap_or_else(|| DASH.into()),
                    NodeType::FullNode => self
                        .node_status
                        .sync_state
                        .as_ref()
                        .map(|s| format!("#{}", group_thousands(s.tip_number.value())))
                        .unwrap_or_else(|| DASH.into()),
                    NodeType::PublicRpc => unreachable!(),
                };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("SYNCED")
                            .font(label_font(9.5))
                            .color(c_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if backend == NodeType::LightClient {
                            let edit = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("EDIT")
                                        .font(label_font(8.5))
                                        .color(self.colors.accent),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .frame(false),
                            );
                            if edit.clicked() {
                                self.set_block_editing = true;
                                self.set_block_input = self
                                    .node_status
                                    .synced_block
                                    .map(|b| b.to_string())
                                    .unwrap_or_default();
                            }
                            ui.add_space(6.0);
                        }
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&synced_value).size(12.5).color(c_text),
                            )
                            .selectable(true),
                        );
                    });
                });
            }

            self.draw_sync_bar(ui, backend);
        }

        let epoch = self
            .node_status
            .tip_header
            .as_ref()
            .map(|h| group_thousands(h.epoch().number()))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, &self.colors, "Epoch", &epoch);

        data_row(ui, &self.colors, "Chain", &chain);
        data_row(ui, &self.colors, "Difficulty", &difficulty);
        data_row(ui, &self.colors, "IBD", &ibd);
        data_row(ui, &self.colors, "Network Time", &median);
    }

    /// Compact segmented block meter (htop idiom), right-aligned like
    /// every other value row: cyan cells while catching up, green once
    /// fully synced (the one semantic green).
    fn draw_sync_bar(&self, ui: &mut egui::Ui, backend: NodeType) {
        const SEGS: usize = 20;
        const SEG_W: f32 = 6.0;
        const SEG_GAP: f32 = 2.0;

        let pct = self.sync_pct(backend);
        let fill = if pct >= 0.999 {
            self.colors.accent2
        } else {
            self.colors.accent
        };
        let c_off = self.colors.border;
        let c_muted = self.colors.text_muted;

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SYNC")
                    .font(label_font(9.5))
                    .color(c_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.1}%", pct * 100.0))
                        .size(11.5)
                        .color(fill),
                );
                ui.add_space(6.0);
                let meter_w = SEGS as f32 * (SEG_W + SEG_GAP) - SEG_GAP;
                let (meter, _) =
                    ui.allocate_exact_size(egui::vec2(meter_w, 9.0), egui::Sense::hover());
                let painter = ui.painter();
                let lit = ((pct * SEGS as f32).round() as usize).min(SEGS);
                for s in 0..SEGS {
                    let x = meter.left() + s as f32 * (SEG_W + SEG_GAP);
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(x, meter.top()),
                        egui::vec2(SEG_W, meter.height()),
                    );
                    painter.rect_filled(cell, 0.0, if s < lit { fill } else { c_off });
                }
            });
        });
    }

    // ── Tx Pool metrics ──────────────────────────────────────

    fn draw_tx_pool_metrics(&self, ui: &mut egui::Ui) {
        let pool = self.node_status.tx_pool_info.as_ref();
        let c = &self.colors;

        let pending = pool
            .map(|p| group_thousands(p.pending.value()))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, c, "Pending", &pending);

        let proposed = pool
            .map(|p| group_thousands(p.proposed.value()))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, c, "Proposed", &proposed);

        let orphan = pool
            .map(|p| group_thousands(p.orphan.value()))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, c, "Orphan", &orphan);

        let fee = pool
            .map(|p| format!("{} sh/KB", group_thousands(p.min_fee_rate.value())))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, c, "Min Fee", &fee);

        let cycles = pool
            .map(|p| group_thousands(p.total_tx_cycles.value()))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, c, "Cycles", &cycles);

        let size = pool
            .map(|p| format!("{} B", group_thousands(p.total_tx_size.value())))
            .unwrap_or_else(|| DASH.into());
        data_row(ui, c, "Tx Size", &size);
    }

    // ── Peer table ───────────────────────────────────────────

    fn draw_peer_table(&self, ui: &mut egui::Ui) {
        let c_border = self.colors.border;
        let c_muted = self.colors.text_muted;
        let c_text = self.colors.text;
        let w = ui.available_width();
        let col_ver = 0.44 * w;
        let col_dir = 0.78 * w;
        // ~6.5px per glyph at 10.5pt mono drives the truncation budgets.
        let id_chars = ((col_ver - 18.0) / 6.5).max(12.0) as usize;
        let ver_chars = (((col_dir - col_ver) - 12.0) / 6.5).max(8.0) as usize;

        let (hr, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
        let painter = ui.painter();
        for (label, x) in [
            ("NODE ID", 6.0),
            ("VERSION", col_ver),
            ("DIRECTION", col_dir),
        ] {
            painter.text(
                egui::pos2(hr.left() + x, hr.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                label_font(9.0),
                c_muted,
            );
        }
        painter.text(
            egui::pos2(hr.right(), hr.center().y),
            egui::Align2::RIGHT_CENTER,
            "PING",
            label_font(9.0),
            c_muted,
        );
        painter.hline(
            hr.x_range(),
            hr.bottom() - 0.5,
            egui::Stroke::new(1.0, c_border),
        );

        for peer in &self.node_status.peers {
            let (rr, resp) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            let painter = ui.painter();
            if resp.hovered() {
                row_hover(painter, rr, &self.colors);
            }
            let cy = rr.center().y;

            painter.text(
                egui::pos2(rr.left() + 6.0, cy),
                egui::Align2::LEFT_CENTER,
                truncate_middle_to(&peer.node_id, id_chars),
                egui::FontId::proportional(10.5),
                c_text,
            );
            painter.text(
                egui::pos2(rr.left() + col_ver, cy),
                egui::Align2::LEFT_CENTER,
                truncate_tail(&peer.version, ver_chars),
                egui::FontId::proportional(10.5),
                c_muted,
            );
            let dir = if peer.is_outbound {
                "OUTBOUND"
            } else {
                "INBOUND"
            };
            painter.text(
                egui::pos2(rr.left() + col_dir, cy),
                egui::Align2::LEFT_CENTER,
                dir,
                label_font(9.0),
                c_muted,
            );
            let ping = peer
                .last_ping_duration
                .as_ref()
                .map(|d| format!("{}ms", d.value()))
                .unwrap_or_else(|| DASH.into());
            painter.text(
                egui::pos2(rr.right(), cy),
                egui::Align2::RIGHT_CENTER,
                ping,
                egui::FontId::proportional(10.5),
                c_text,
            );
            painter.hline(
                rr.x_range(),
                rr.bottom() - 0.5,
                egui::Stroke::new(1.0, c_border),
            );
        }
    }
}

// ── Free functions ───────────────────────────────────────────

/// Space either side of the "·" between a choice's options.
const CHOICE_GAP: f32 = 8.0;

/// A settings row laid out like `data_row`: the label on the left and
/// `control` on the right, where the values of the rows below sit. The
/// control is drawn right to left.
fn setting_row(
    ui: &mut egui::Ui,
    colors: &AppColors,
    label: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label.to_uppercase())
                .font(label_font(9.5))
                .color(colors.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// A choice drawn as text, so it reads like the values below it: the
/// options in a row with "·" between them, in the values' font. The chosen
/// one is in the accent and underlined; the others are muted until hovered.
/// With `chosen` None, no option is chosen, so the options work as plain
/// links, for actions. Returns the option clicked this frame, if it isn't
/// the chosen one.
fn text_choice(
    ui: &mut egui::Ui,
    colors: &AppColors,
    options: &[&str],
    chosen: Option<usize>,
) -> Option<usize> {
    // `data_row`'s value font. Laid out without a colour, so each option can
    // be painted in its own.
    let font = egui::FontId::proportional(12.5);
    let layout = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
    };
    let galleys: Vec<_> = options.iter().map(|option| layout(option)).collect();
    let dot = layout("·");
    let dot_w = dot.size().x + 2.0 * CHOICE_GAP;
    let text_h = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max);
    let total = galleys.iter().map(|g| g.size().x).sum::<f32>()
        + dot_w * options.len().saturating_sub(1) as f32;
    // Room under the text for the underline.
    let (area, _) = ui.allocate_exact_size(egui::vec2(total, text_h + 3.0), egui::Sense::hover());

    let mut clicked = None;
    let mut x = area.left();
    for (i, (option, galley)) in options.iter().zip(galleys).enumerate() {
        if i > 0 {
            ui.painter().galley(
                egui::pos2(x + CHOICE_GAP, area.top()),
                dot.clone(),
                colors.text_muted,
            );
            x += dot_w;
        }
        let rect = egui::Rect::from_min_size(egui::pos2(x, area.top()), galley.size());
        x += galley.size().x;
        let resp = ui.interact(
            rect,
            ui.id().with(("choice", *option)),
            egui::Sense::click(),
        );
        let active = chosen == Some(i);
        let color = if active {
            colors.accent
        } else if resp.hovered() {
            colors.text
        } else {
            colors.text_muted
        };
        ui.painter().galley(rect.min, galley, color);
        if active {
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() + 1.5,
                egui::Stroke::new(1.0, colors.accent),
            );
        } else {
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    }
    clicked
}

/// A color row's control, drawn right to left: the hex code in the values'
/// font, then a swatch that opens egui's color picker. Returns the color
/// either one set this frame.
fn color_control(
    ui: &mut egui::Ui,
    colors: &AppColors,
    role: ColorRole,
    current: egui::Color32,
) -> Option<egui::Color32> {
    let mut picked = None;
    ui.push_id(role.label(), |ui| {
        // While the code is edited, the typed text is kept here and each
        // valid code shows at once. Leaving the field shows the current
        // color's code again, so a half-typed one just goes away.
        let draft_id = ui.id().with("hex-draft");
        let mut text = ui
            .data(|d| d.get_temp::<String>(draft_id))
            .unwrap_or_else(|| palette::to_hex(current));
        let font = egui::FontId::proportional(12.5);
        let width = ui
            .painter()
            .layout_no_wrap("#000000".to_string(), font.clone(), colors.text)
            .size()
            .x;
        let text_color = if palette::parse_hex(&text).is_some() {
            colors.text
        } else {
            colors.danger
        };
        let edit = ui.add(
            egui::TextEdit::singleline(&mut text)
                .font(font)
                .text_color(text_color)
                .frame(egui::Frame::NONE)
                .desired_width(width)
                .char_limit(7),
        );
        // No box around the code, so an underline shows it can be edited.
        if edit.has_focus() || edit.hovered() {
            let line = if edit.has_focus() {
                colors.accent
            } else {
                colors.border2
            };
            ui.painter().hline(
                edit.rect.x_range(),
                edit.rect.bottom() + 1.5,
                egui::Stroke::new(1.0, line),
            );
        }
        if edit.has_focus() {
            if edit.changed() {
                picked = palette::parse_hex(&text);
            }
            ui.data_mut(|d| d.insert_temp(draft_id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(draft_id));
        }

        ui.add_space(10.0);
        // A small square, not egui's wide default swatch.
        ui.spacing_mut().interact_size = egui::vec2(14.0, 14.0);
        let mut color = current;
        let swatch = egui::color_picker::color_edit_button_srgba(
            ui,
            &mut color,
            egui::color_picker::Alpha::Opaque,
        );
        if swatch.on_hover_text(role.description()).changed() {
            picked = Some(color);
        }
    });
    picked
}

fn full_node_sync_pct(sync_state: Option<&ckb_jsonrpc_types::SyncState>) -> f32 {
    let Some(s) = sync_state else {
        return 0.0;
    };
    let tip = s.tip_number.value();
    let best = s.best_known_block_number.value();
    if best > 0 {
        (tip as f64 / best as f64).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

fn target_tip_value(backend: NodeType, status: &crate::types::NodeStatus) -> String {
    match backend {
        NodeType::FullNode => match status.sync_state.as_ref() {
            Some(s) => format!("#{}", group_thousands(s.best_known_block_number.value())),
            None => DASH.into(),
        },
        NodeType::LightClient => match status.tip_block() {
            Some(t) => format!("#{}", group_thousands(t)),
            None => DASH.into(),
        },
        NodeType::PublicRpc => DASH.into(),
    }
}

fn block_height_text(tip: Option<u64>) -> String {
    tip.map(|n| format!("#{}", group_thousands(n)))
        .unwrap_or_else(|| DASH.to_string())
}

fn port_text(port: Option<u16>) -> String {
    port.map(|p| p.to_string())
        .unwrap_or_else(|| DASH.to_string())
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
