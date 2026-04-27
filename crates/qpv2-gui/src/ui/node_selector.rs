//! Node selector popup rendering.

use eframe::egui;
use node_manager::NodeType;

use crate::types::Status;
use crate::App;

impl App {
    /// Show the node selector configuration popup.
    pub(crate) fn show_node_selector_popup(&mut self, ctx: &egui::Context) {
        if !self.node_selector_open {
            return;
        }

        let Some(selector_rect) = self.node_selector_rect else {
            return;
        };

        // Position dropdown below the selector box
        let dropdown_pos = egui::pos2(selector_rect.left(), selector_rect.bottom() + 4.0);

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

                        // ── NETWORK section ──
                        ui.label(
                            egui::RichText::new("NETWORK")
                                .size(8.5)
                                .family(egui::FontFamily::Monospace)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(4.0);
                        // Override hover styling locally. The app sets `override_text_color`
                        // globally in main.rs, which forces radio labels to stay cream on
                        // hover — the tint behind the text looks weak. Clearing the override
                        // inside this scope lets per-state text colors shine through.
                        ui.scope(|ui| {
                            let vis = &mut ui.style_mut().visuals;
                            vis.override_text_color = None;
                            vis.widgets.hovered.fg_stroke.color = self.colors.accent;
                            vis.widgets.hovered.weak_bg_fill = self.colors.accent_tint;
                            ui.horizontal(|ui| {
                                ui.radio_value(
                                    &mut self.temp_network,
                                    node_manager::NetworkType::Mainnet,
                                    "Mainnet",
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                ui.radio_value(
                                    &mut self.temp_network,
                                    node_manager::NetworkType::Testnet,
                                    "Testnet",
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            });
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // ── ACTIVE NODE section ──
                        ui.label(
                            egui::RichText::new("ACTIVE NODE")
                                .size(8.5)
                                .family(egui::FontFamily::Monospace)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(6.0);

                        // Node type rows: icon + name + colored badge. Clickable full-width row.
                        // Mockup emojis, rendered monochrome by Noto Sans Symbols 2
                        // (loaded in main.rs). 🖥 U+1F5A5, 💡 U+1F4A1, 🌐 U+1F310.
                        let row_defs = [
                            (
                                NodeType::FullNode,
                                "\u{1F5A5}", // 🖥 desktop computer
                                "Full Node",
                                "FULL",
                                self.colors.accent_tint,
                                self.colors.accent,
                            ),
                            (
                                NodeType::LightClient,
                                "\u{1F4A1}", // 💡 light bulb
                                "Light Client",
                                "LIGHT",
                                self.colors.accent2_tint,
                                self.colors.accent2,
                            ),
                            (
                                NodeType::PublicRpc,
                                "\u{1F310}", // 🌐 globe with meridians
                                "Public RPC",
                                "RPC",
                                self.colors.warn_tint,
                                self.colors.warn,
                            ),
                        ];

                        for (ntype, icon, name, badge_text, badge_fill, accent_color) in row_defs {
                            let selected = self.temp_node_type == ntype;
                            let row_bg = if selected {
                                self.colors.accent_tint
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            let response = egui::Frame::new()
                                .fill(row_bg)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(icon)
                                                .size(14.0)
                                                .color(accent_color),
                                        );
                                        ui.add_space(6.0);
                                        let name_color = if selected {
                                            self.colors.accent
                                        } else {
                                            self.colors.text
                                        };
                                        ui.label(
                                            egui::RichText::new(name).size(12.5).color(name_color),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                egui::Frame::new()
                                                    .fill(badge_fill)
                                                    .corner_radius(4.0)
                                                    .inner_margin(egui::Margin::symmetric(6, 1))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            egui::RichText::new(badge_text)
                                                                .size(8.5)
                                                                .family(egui::FontFamily::Monospace)
                                                                .color(accent_color),
                                                        );
                                                    });
                                            },
                                        );
                                    });
                                })
                                .response;

                            let click = response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if click.clicked() {
                                self.temp_node_type = ntype;
                            }
                        }

                        ui.add_space(10.0);

                        // Apply button
                        let apply_btn = egui::Button::new(
                            egui::RichText::new("Apply").color(self.colors.bg)
                        )
                        .fill(self.colors.accent)
                        .min_size(egui::vec2(ui.available_width(), 28.0));

                        if ui.add(apply_btn).clicked() {
                            // Compare the popup's draft (`temp_*`) against
                            // the currently-committed config. No draft on
                            // `App` — the committed state lives inside
                            // `node_manager`.
                            let current = self.node_manager.config();
                            let network_changed = self.temp_network != current.network;
                            let node_type_changed =
                                self.temp_node_type != current.node_type;

                            if network_changed || node_type_changed {
                                // Refresh the form's RPC URL preview to
                                // match the new backend before commit.
                                if node_type_changed {
                                    self.on_node_type_changed();
                                } else if network_changed
                                    && self.temp_node_type == NodeType::PublicRpc
                                {
                                    self.settings_rpc_url =
                                        node_manager::NodeConfig::default_rpc_url_for(
                                            self.temp_node_type,
                                            self.temp_network,
                                        )
                                        .to_string();
                                }

                                // Stop the previously-running local node
                                // (if any) before rebuild — a light-client
                                // indexed for the old network must not
                                // outlive the switch.
                                self.node_manager.stop();

                                // Commit edits: save to disk + replace
                                // `node_manager` with one bound to the new
                                // config. Must happen before `spawn()` so
                                // the new manager is the one that owns the
                                // new child handle.
                                self.save_node_config();

                                // Spawn the new local node (no-op for
                                // PublicRpc, unsupported-op for FullNode).
                                // Failure surfaces as a status error and
                                // leaves the process slot empty — user can
                                // retry via Apply.
                                if let Err(e) = self.node_manager.spawn() {
                                    self.status = Status::Error(format!(
                                        "Failed to start local node: {}",
                                        e
                                    ));
                                } else if self.node_manager.has_local_process()
                                    && self.temp_node_type == NodeType::LightClient
                                {
                                    // Warmup the QR-lock-script cell dep so
                                    // the first transfer doesn't race-fail.
                                    // Only the RPC error path is actionable;
                                    // a not-yet-Fetched response is expected.
                                    if let Err(e) = self.node_manager.fetch_qr_lock_dep() {
                                        self.status = Status::Error(format!(
                                            "Failed to request lock-script cell dep fetch: {}",
                                            e
                                        ));
                                    }
                                    // Re-register every account on the
                                    // freshly-spawned LC. Anchored at tip
                                    // — historical txs below tip aren't
                                    // recovered (Phase A's per-account
                                    // start-block work covers that).
                                    let accounts = self.accounts.clone();
                                    self.register_lock_scripts_with_light_client(&accounts);
                                }

                                // Swap the tx-history cache to the new
                                // network's file. Drop the in-flight sync
                                // receiver first so the pending thread (still
                                // querying the previous network) can't land
                                // its results under the new file on `Done`.
                                if network_changed {
                                    self.tx_history_rx = None;
                                    self.load_tx_history_from_disk();
                                }

                                if !matches!(self.status, Status::Error(_)) {
                                    self.status =
                                        Status::Info("Connecting...".to_string());
                                }
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
