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

                        // Network selection (compact horizontal)
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut self.temp_network,
                                node_manager::NetworkType::Mainnet,
                                "Mainnet",
                            );
                            ui.radio_value(
                                &mut self.temp_network,
                                node_manager::NetworkType::Testnet,
                                "Testnet",
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
                                "Public RPC",
                            );
                            ui.radio_value(
                                &mut self.temp_node_type,
                                NodeType::LightClient,
                                "Light Client",
                            );
                            ui.radio_value(
                                &mut self.temp_node_type,
                                NodeType::FullNode,
                                "Full Node",
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
                            let node_type_changed =
                                self.temp_node_type != self.node_config.node_type;

                            if network_changed || node_type_changed {
                                // Update configuration
                                self.node_config.network = self.temp_network;
                                self.node_config.node_type = self.temp_node_type;

                                // Update RPC URL for new configuration
                                if node_type_changed {
                                    self.on_node_type_changed();
                                } else if network_changed
                                    && self.node_config.node_type == NodeType::PublicRpc
                                {
                                    // For Public RPC, update URL when network changes
                                    let default_url =
                                        self.node_config.default_rpc_url().to_string();
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
