//! Setup and Locked screen rendering.

use crate::types::{Screen, Status, Tab};
use crate::App;
use ckb_node::NodeType;
use eframe::egui;
use qpv2_core::{types::SpxVariant, KeyVault};

impl App {
    pub(crate) fn show_welcome(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            // Logo
            ui.heading(
                egui::RichText::new("\u{1f52e} Quantum Purse")
                    .size(32.0)
                    .color(self.colors.accent)
                    .strong(),
            );

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Post-Quantum Secure Wallet")
                    .size(14.0)
                    .color(self.colors.text_muted),
            );

            ui.add_space(40.0);

            // Setup card
            egui::Frame::new()
                .fill(self.colors.surface2)
                .corner_radius(16.0)
                .inner_margin(32.0)
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(ui, |ui| {
                    ui.set_max_width(400.0);

                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Create New Wallet").size(20.0).strong());
                    });

                    ui.add_space(24.0);

                    ui.label("Select SPHINCS+ variant:");
                    ui.add_space(8.0);

                    let field_width = ui.available_width();

                    egui::ComboBox::from_id_salt("variant")
                        .selected_text(format!("{}", self.selected_variant))
                        .width(field_width)
                        .show_ui(ui, |ui| {
                            for variant in &[
                                SpxVariant::Sha2128S,
                                SpxVariant::Sha2128F,
                                SpxVariant::Shake128S,
                                SpxVariant::Shake128F,
                                SpxVariant::Sha2192S,
                                SpxVariant::Sha2192F,
                                SpxVariant::Shake192S,
                                SpxVariant::Shake192F,
                                SpxVariant::Sha2256S,
                                SpxVariant::Sha2256F,
                                SpxVariant::Shake256S,
                                SpxVariant::Shake256F,
                            ] {
                                ui.selectable_value(
                                    &mut self.selected_variant,
                                    *variant,
                                    format!("{}", variant),
                                );
                            }
                        });

                    ui.add_space(32.0);

                    #[cfg(target_os = "macos")]
                    let is_busy = self.passkey_op.is_some();
                    #[cfg(not(target_os = "macos"))]
                    let is_busy = false;

                    // Passkey button — kicks off async registration with Touch ID.
                    let pk_button = egui::Button::new(
                        egui::RichText::new(if is_busy {
                            "Creating wallet..."
                        } else {
                            "Create with Touch ID"
                        })
                        .size(16.0)
                        .color(self.colors.bg),
                    )
                    .fill(self.colors.accent)
                    .min_size(egui::vec2(field_width, 48.0));

                    if ui.add_enabled(!is_busy, pk_button).clicked() {
                        self.create_wallet_with_passkey_start(frame);
                    }

                    ui.add_space(10.0);

                    // Password button — opens the pinentry dialog
                    // synchronously (with a confirm field). Blocks
                    // the egui frame for the duration of the dialog;
                    // see `pinentry::prompt_password_with_confirmation`.
                    let pw_btn = egui::Button::new(
                        egui::RichText::new("Create with Password")
                            .size(16.0)
                            .color(self.colors.text),
                    )
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border2))
                    .min_size(egui::vec2(field_width, 48.0));
                    if ui.add_enabled(!is_busy, pw_btn).clicked() {
                        self.create_wallet_with_password(self.selected_variant);
                    }
                });

            ui.add_space(24.0);
            // Center the status row to match the rest of the page.
            ui.vertical_centered(|ui| self.show_status(ui));
        });
    }

    pub(crate) fn show_unlocked(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Left sidebar matching the mockup layout.
        // The SidePanel's built-in right-edge separator is disabled
        // (`show_separator_line(false)`) — its sub-pixel anti-aliasing
        // makes a 1 px low-alpha stroke read as bright white against
        // the dark bg. We paint our own vline at the bottom of the
        // closure using the same `Stroke::new(1.0, colors.border)`
        // every in-sidebar divider uses, so they all look identical.
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .show_separator_line(false)
            .exact_width(236.0)
            .frame(egui::Frame::new().fill(self.colors.surface))
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
                        ui.allocate_painter(egui::vec2(208.0, 42.0), egui::Sense::click());

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
                    let inner = rect.shrink2(egui::vec2(12.0, 5.0));

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

                    // TODO: replace with a real liveness probe (periodic
                    // tip-header ping). The node manager is always constructed
                    // at startup, so this dot is currently purely cosmetic.
                    let dot_color = self.colors.accent;
                    painter.circle_filled(
                        egui::pos2(inner.left() + 4.0, row_y + 7.0),
                        3.0,
                        dot_color,
                    );

                    // Node type
                    let node_name = match self.qp_client.config().node_type {
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

                    // Dropdown arrow. The network badge's right edge is at `inner.right() - 5.0`
                    // and "MAIN"/"TEST" at 8pt is ~24px wide, so its left edge sits ~-29.
                    // Anchor the arrow's RIGHT edge at -36 to leave ~7px clear of the badge.
                    painter.text(
                        egui::pos2(inner.right() - 28.0, row_y),
                        egui::Align2::RIGHT_TOP,
                        "\u{25bc}",
                        egui::FontId::proportional(9.0),
                        self.colors.text_muted,
                    );

                    // Network badge
                    let network = match self.qp_client.network() {
                        ckb_node::NetworkType::Mainnet => "MAIN",
                        ckb_node::NetworkType::Testnet => "TEST",
                    };
                    let network_color = if self.qp_client.is_mainnet() {
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
                        // Seed the popup draft from the committed config
                        // each time it opens so stale selections from a
                        // previous (un-applied) session don't leak through.
                        if self.node_selector_open {
                            let cfg = self.qp_client.config();
                            self.temp_network = cfg.network;
                            self.temp_node_type = cfg.node_type;
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
                self.draw_nav_item(ui, Tab::Transfer, "\u{2191}\u{2193}", "Transfer");

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

                // Section: Network
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("NETWORK")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::NodeManager, "\u{25c9}", "Node Manager");

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
                                // Stop the local node before wiping its data
                                // directory — otherwise a still-running child
                                // would race against `clear_database()` and
                                // leave scraps behind.
                                self.local_node.stop();

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

                // Hand-painted right-edge separator. Same stroke as
                // every in-sidebar divider so the boundary tone
                // matches the "QPV2 / NERVOS NETWORK" line. See the
                // top of this fn for why the SidePanel's auto
                // separator is disabled.
                let panel_rect = ui.clip_rect();
                ui.painter().vline(
                    panel_rect.right() - 0.5,
                    panel_rect.y_range(),
                    egui::Stroke::new(1.0, self.colors.border),
                );
            });

        // ── Main content area ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.colors.bg))
            .show(ctx, |ui| {
                self.draw_unlocked_bg(ui);

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(24.0);

                    match self.active_tab {
                        Tab::Dashboard => self.show_dashboard_tab(ui, frame),
                        Tab::Transfer => self.show_transfer_tab(ui),
                        Tab::DaoOperations => self.show_dao_tab(ui),
                        Tab::NodeManager => self.show_node_manager_tab(ui),
                        Tab::Accounts => self.show_accounts_tab(ui, frame),
                    }
                });
            });
    }

    pub(crate) fn show_locked(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);

            // Lock icon
            ui.label(egui::RichText::new("\u{1f512}").size(64.0));

            ui.add_space(24.0);

            ui.heading(
                egui::RichText::new("Wallet Locked")
                    .size(28.0)
                    .color(self.colors.text),
            );

            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("Authenticate to access your wallet")
                    .color(self.colors.text_muted),
            );

            ui.add_space(40.0);

            #[cfg(target_os = "macos")]
            let is_busy = self.passkey_op.is_some();
            #[cfg(not(target_os = "macos"))]
            let is_busy = false;

            let button = egui::Button::new(
                egui::RichText::new(if is_busy {
                    "Waiting for Touch ID..."
                } else {
                    "Unlock with Touch ID"
                })
                .size(16.0)
                .color(self.colors.bg),
            )
            .fill(self.colors.accent2)
            .min_size(egui::vec2(280.0, 48.0));

            if ui.add_enabled(!is_busy, button).clicked() {
                self.unlock_with_passkey_start(frame);
            }

            ui.add_space(24.0);
            // Nest in a centered layout so the status row lines up with
            // the rest of the page instead of flushing left.
            ui.vertical_centered(|ui| self.show_status(ui));
        });
    }
}
