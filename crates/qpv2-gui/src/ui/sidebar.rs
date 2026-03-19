//! Unlocked screen: sidebar navigation and central panel routing.

use eframe::egui;
use node_manager::NodeType;
use qpv2_core::KeyVault;

use crate::types::{Screen, Status, Tab};
use crate::App;

impl App {
    pub(crate) fn show_unlocked(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Left sidebar matching the mockup layout.
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(236.0)
            .frame(
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border)),
            )
            .show(ctx, |ui| {
                // ── Logo section ──
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);

                    // Logo icon with gradient-like background
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(icon_rect, 10.0, self.colors.accent);
                    ui.painter().text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Q",
                        egui::FontId::proportional(17.0),
                        egui::Color32::from_rgb(5, 12, 10),
                    );

                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("QPV2")
                                .size(21.0)
                                .color(self.colors.accent)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new("NERVOS NETWORK")
                                .size(8.0)
                                .color(self.colors.text_muted),
                        );
                    });
                });

                ui.add_space(6.0);
                // Divider below logo
                ui.horizontal(|ui| {
                    let rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [rect.left_top(), egui::pos2(rect.right(), rect.top())],
                        egui::Stroke::new(1.0, self.colors.border),
                    );
                    ui.add_space(1.0);
                });

                // ── Node selector ──
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);

                    // Make it interactive
                    let (response, painter) =
                        ui.allocate_painter(egui::vec2(208.0, 52.0), egui::Sense::click());

                    let rect = response.rect;
                    let is_hovered = response.hovered();

                    // Store rect for dropdown positioning
                    self.node_selector_rect = Some(rect);

                    // Draw background
                    let bg_color = if is_hovered {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10)
                    } else {
                        self.colors.surface2
                    };

                    painter.rect_filled(rect, 9.0, bg_color);
                    painter.rect_stroke(
                        rect,
                        9.0,
                        egui::Stroke::new(1.0, self.colors.border),
                        egui::StrokeKind::Outside,
                    );

                    // Content layout
                    let inner = rect.shrink2(egui::vec2(12.0, 9.0));

                    // "ACTIVE NODE" label
                    painter.text(
                        inner.left_top() + egui::vec2(0.0, 0.0),
                        egui::Align2::LEFT_TOP,
                        "ACTIVE NODE",
                        egui::FontId::proportional(8.0),
                        self.colors.text_muted,
                    );

                    // Node info row
                    let row_y = inner.top() + 14.0;

                    // Connection dot
                    let connected = self.rpc_client.is_some();
                    let dot_color = if connected {
                        self.colors.accent
                    } else {
                        self.colors.text_muted
                    };
                    painter.circle_filled(
                        egui::pos2(inner.left() + 4.0, row_y + 7.0),
                        3.0,
                        dot_color,
                    );

                    // Node type
                    let node_name = match self.node_config.node_type {
                        NodeType::PublicRpc => "Public RPC",
                        NodeType::LightClient => "Light Client",
                        NodeType::FullNode => "Full Node",
                    };
                    painter.text(
                        egui::pos2(inner.left() + 14.0, row_y),
                        egui::Align2::LEFT_TOP,
                        node_name,
                        egui::FontId::proportional(13.0),
                        self.colors.text,
                    );

                    // Dropdown arrow
                    painter.text(
                        egui::pos2(inner.right() - 28.0, row_y),
                        egui::Align2::LEFT_TOP,
                        "\u{25bc}",
                        egui::FontId::proportional(9.0),
                        self.colors.text_muted,
                    );

                    // Network badge
                    let network = match self.node_config.network {
                        node_manager::NetworkType::Mainnet => "MAIN",
                        node_manager::NetworkType::Testnet => "TEST",
                    };
                    let network_color =
                        if self.node_config.network == node_manager::NetworkType::Mainnet {
                            self.colors.accent
                        } else {
                            self.colors.warn
                        };
                    painter.text(
                        egui::pos2(inner.right() - 5.0, row_y),
                        egui::Align2::RIGHT_TOP,
                        network,
                        egui::FontId::proportional(8.0),
                        network_color,
                    );

                    // Handle click
                    if response.clicked() {
                        self.node_selector_open = !self.node_selector_open;
                        // Update temp values when opening
                        if self.node_selector_open {
                            self.temp_network = self.node_config.network;
                            self.temp_node_type = self.node_config.node_type;
                        }
                    }

                    // Change cursor on hover
                    if is_hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });

                // ── Navigation ──
                ui.add_space(14.0);

                // Section: Wallet
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("WALLET")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::Dashboard, "\u{25c8}", "Dashboard");
                self.draw_nav_item(ui, Tab::Transfer, "\u{2191}", "Transfer");

                ui.add_space(10.0);

                // Section: NervosDAO
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("NERVOSDAO")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::DaoOperations, "\u{2b21}", "DAO Operations");

                ui.add_space(10.0);

                // Section: Security
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("SECURITY")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::Accounts, "\u{25ce}", "Accounts");

                // ── Bottom: Lock / Remove Wallet ──
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(30.0);

                    // Remove Wallet button
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let remove_label = if self.confirm_remove {
                            "\u{26a0} Confirm Remove?"
                        } else {
                            "\u{1f5d1} Remove Wallet"
                        };
                        let remove_btn = egui::Button::new(
                            egui::RichText::new(remove_label)
                                .size(11.0)
                                .color(self.colors.danger),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 77, 109, 77),
                        ))
                        .min_size(egui::vec2(194.0, 28.0));

                        if ui.add(remove_btn).clicked() {
                            if self.confirm_remove {
                                match KeyVault::clear_database() {
                                    Ok(()) => {
                                        self.lock_wallet();
                                        self.screen = Screen::Setup;
                                        self.status = Status::Info(
                                            "Wallet removed successfully.".to_string(),
                                        );
                                    }
                                    Err(e) => {
                                        self.status = Status::Error(format!(
                                            "Failed to remove wallet: {}",
                                            e
                                        ));
                                    }
                                }
                            } else {
                                self.confirm_remove = true;
                            }
                        }
                    });

                    ui.add_space(12.0);

                    // Lock Wallet button
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let lock_btn = egui::Button::new(
                            egui::RichText::new("\u{1f512} Lock Wallet").size(12.0),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.0, self.colors.border))
                        .min_size(egui::vec2(194.0, 32.0));

                        if ui.add(lock_btn).clicked() {
                            self.lock_wallet();
                        }
                    });

                    ui.add_space(4.0);

                    // Divider
                    ui.horizontal(|ui| {
                        let rect = ui.available_rect_before_wrap();
                        ui.painter().line_segment(
                            [rect.left_top(), egui::pos2(rect.right(), rect.top())],
                            egui::Stroke::new(1.0, self.colors.border),
                        );
                        ui.add_space(1.0);
                    });
                });
            });

        // ── Main content area ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.colors.bg))
            .show(ctx, |ui| {
                self.draw_gradient_bg(ui);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(24.0);

                    match self.active_tab {
                        Tab::Dashboard => self.show_dashboard_tab(ui, frame),
                        Tab::Transfer => self.show_transfer_tab(ui),
                        Tab::DaoOperations => self.show_dao_tab(ui),
                        Tab::Accounts => self.show_accounts_tab(ui, frame),
                    }
                });
            });
    }
}
