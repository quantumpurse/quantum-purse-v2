//! Setup and Locked screen rendering.

use crate::types::Tab;
use crate::App;
use ckb_node::NodeType;
use eframe::egui;
use qpv2_core::types::{AuthMethod, SpxVariant};

impl App {
    pub(crate) fn show_welcome(&mut self, ui: &mut egui::Ui) {
        let variants = [
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
        ];

        let row_h = 22.0;
        let group_gap = 14.0;
        let group_h = 4.0 * row_h;
        let thread_h = 3.0 * group_h + 2.0 * group_gap;
        let btn_h = 40.0;
        let btn_gap = 8.0;
        let btn_w = 190.0;
        let center_w = 170.0;
        let buttons_h = 3.0 * btn_h + 2.0 * btn_gap;
        let btn_top_pad = (thread_h - buttons_h) / 2.0;

        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            ui.label(
                egui::RichText::new("QUANTUM PURSE")
                    .size(24.0)
                    .color(self.colors.accent)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Post-quantum secure wallet for Nervos Network.")
                    .size(13.0)
                    .color(self.colors.text_muted),
            );

            ui.add_space(32.0);

            let spacing_x = ui.spacing().item_spacing.x;
            let total_w = btn_w * 2.0 + center_w + spacing_x * 2.0;
            let left_pad = (ui.available_width() - total_w) / 2.0;

            ui.horizontal(|ui| {
                ui.add_space(left_pad.max(0.0));
                // ── Left column: CREATE ──
                ui.allocate_ui_with_layout(
                    egui::vec2(btn_w, thread_h),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(btn_top_pad);

                        for group_idx in 0..3u8 {
                            let label = match group_idx {
                                0 => format!("Create with {}", keychain::short_name()),
                                1 => "Create with Security Key".to_string(),
                                _ => "Create with Password".to_string(),
                            };
                            let (fill, text_color, stroke) = self.auth_button_style(group_idx);

                            let btn = egui::Button::new(
                                egui::RichText::new(&label).size(12.0).color(text_color),
                            )
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(8.0)
                            .min_size(egui::vec2(btn_w, btn_h));

                            if ui.add(btn).clicked() {
                                let v = self.selected_variant;
                                match group_idx {
                                    0 => self.create_wallet_with_keychain(v),
                                    1 => self.create_wallet_with_fido2(v),
                                    _ => self.create_wallet_with_password(v),
                                }
                            }

                            if group_idx < 2 {
                                ui.add_space(btn_gap);
                            }
                        }
                    },
                );

                // ── Center column: Variant thread ──
                ui.allocate_ui_with_layout(
                    egui::vec2(center_w, thread_h),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let (response, painter) = ui
                            .allocate_painter(egui::vec2(center_w, thread_h), egui::Sense::click());
                        let rect = response.rect;
                        let line_x = rect.center().x;

                        let first_y = rect.top();
                        let last_y = rect.bottom();

                        painter.line_segment(
                            [egui::pos2(line_x, first_y), egui::pos2(line_x, last_y)],
                            egui::Stroke::new(1.0, self.colors.border),
                        );

                        for (i, variant) in variants.iter().enumerate() {
                            let group = i / 4;
                            let in_group = i % 4;
                            let y = rect.top()
                                + group as f32 * (group_h + group_gap)
                                + in_group as f32 * row_h
                                + row_h / 2.0;

                            let is_selected = *variant == self.selected_variant;
                            let (dot_color, dot_r, text_color) = if is_selected {
                                (self.colors.accent, 5.0, self.colors.text)
                            } else {
                                (self.colors.border2, 3.0, self.colors.text_muted)
                            };

                            let (hash, param) = variant_parts(*variant);

                            painter.circle_filled(egui::pos2(line_x, y), dot_r, dot_color);
                            painter.text(
                                egui::pos2(line_x - 12.0, y),
                                egui::Align2::RIGHT_CENTER,
                                hash,
                                egui::FontId::proportional(11.0),
                                text_color,
                            );
                            painter.text(
                                egui::pos2(line_x + 12.0, y),
                                egui::Align2::LEFT_CENTER,
                                param,
                                egui::FontId::proportional(11.0),
                                text_color,
                            );
                        }

                        if response.clicked() {
                            if let Some(pos) = response.interact_pointer_pos() {
                                for (i, variant) in variants.iter().enumerate() {
                                    let group = i / 4;
                                    let in_group = i % 4;
                                    let y = rect.top()
                                        + group as f32 * (group_h + group_gap)
                                        + in_group as f32 * row_h
                                        + row_h / 2.0;
                                    if (pos.y - y).abs() < row_h / 2.0 {
                                        self.selected_variant = *variant;
                                        break;
                                    }
                                }
                            }
                        }

                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    },
                );

                // ── Right column: IMPORT ──
                ui.allocate_ui_with_layout(
                    egui::vec2(btn_w, thread_h),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(btn_top_pad);

                        for group_idx in 0..3u8 {
                            let label = match group_idx {
                                0 => format!("Import with {}", keychain::short_name()),
                                1 => "Import with Security Key".to_string(),
                                _ => "Import with Password".to_string(),
                            };
                            let (fill, text_color, stroke) = self.auth_button_style(group_idx);

                            let btn = egui::Button::new(
                                egui::RichText::new(&label).size(12.0).color(text_color),
                            )
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(8.0)
                            .min_size(egui::vec2(btn_w, btn_h));

                            if ui.add(btn).clicked() {
                                let v = self.selected_variant;
                                match group_idx {
                                    0 => self.import_seed_phrase_with_keychain(v),
                                    1 => self.import_seed_phrase_with_fido2(v),
                                    _ => self.import_seed_phrase_with_password(v),
                                }
                            }

                            if group_idx < 2 {
                                ui.add_space(btn_gap);
                            }
                        }
                    },
                );
            });

            ui.add_space(24.0);
            ui.vertical_centered(|ui| self.show_status(ui));
        });
    }

    pub(crate) fn show_unlocked(&mut self, ctx: &egui::Context) {
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
                            egui::RichText::new("Quantum Purse")
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
                ui.add_space(6.0);
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
                        egui::StrokeKind::Inside,
                    );

                    // Content layout
                    let inner = rect.shrink2(egui::vec2(12.0, 5.0));

                    // "ACTIVE NODE" label
                    painter.text(
                        inner.left_top() + egui::vec2(0.0, 0.0),
                        egui::Align2::LEFT_TOP,
                        "ACTIVE NODE",
                        egui::FontId::proportional(8.0),
                        self.colors.text,
                    );

                    // Node info row
                    let row_y = inner.top() + 14.0;

                    let t = ui.input(|i| i.time);
                    let blink_alpha = ((t * std::f64::consts::TAU).sin() * 0.5 + 0.5) as f32;
                    let dot_color = if self.node_status.online {
                        if self.node_status_reconnected_at.is_some() {
                            self.colors.accent.linear_multiply(0.3 + 0.7 * blink_alpha)
                        } else {
                            self.colors.accent
                        }
                    } else {
                        self.colors.danger.linear_multiply(0.3 + 0.7 * blink_alpha)
                    };
                    painter.circle_filled(
                        egui::pos2(inner.left() + 4.0, row_y + 7.0),
                        3.0,
                        dot_color,
                    );
                    if !self.node_status.online || self.node_status_reconnected_at.is_some() {
                        ctx.request_repaint();
                    }

                    // Node type
                    let node_name = match self.qp_client.config().node_type {
                        NodeType::PublicRpc => "Remote RPC",
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
                        egui::pos2(inner.right() - 28.0, row_y - 13.0),
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
                        egui::pos2(inner.right() - 5.0, row_y - 13.0),
                        egui::Align2::RIGHT_TOP,
                        network,
                        egui::FontId::proportional(8.0),
                        network_color,
                    );

                    // Handle click
                    if response.clicked() {
                        self.node_selector_open = !self.node_selector_open;
                        self.wallet_selector_open = false;
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

                // ── Wallet selector ──
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);

                    let (response, painter) =
                        ui.allocate_painter(egui::vec2(208.0, 42.0), egui::Sense::click());

                    let rect = response.rect;
                    let is_hovered = response.hovered();

                    self.wallet_selector_rect = Some(rect);

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
                        egui::StrokeKind::Inside,
                    );

                    let inner = rect.shrink2(egui::vec2(12.0, 5.0));

                    painter.text(
                        inner.left_top(),
                        egui::Align2::LEFT_TOP,
                        "ACTIVE WALLET",
                        egui::FontId::proportional(8.0),
                        self.colors.text,
                    );

                    let row_y = inner.top() + 14.0;

                    painter.circle_filled(
                        egui::pos2(inner.left() + 4.0, row_y + 7.0),
                        3.0,
                        self.colors.accent,
                    );

                    painter.text(
                        egui::pos2(inner.left() + 14.0, row_y),
                        egui::Align2::LEFT_TOP,
                        &self.wallet_name,
                        egui::FontId::proportional(13.0),
                        self.colors.text,
                    );

                    painter.text(
                        egui::pos2(inner.right() - 45.0, row_y - 13.0),
                        egui::Align2::RIGHT_TOP,
                        "\u{25bc}",
                        egui::FontId::proportional(9.0),
                        self.colors.text_muted,
                    );

                    // Variant badge (from cache — no disk I/O).
                    if let Some(cw) = self.wallet_cache.iter().find(|w| w.id == self.wallet_id) {
                        painter.text(
                            egui::pos2(inner.right() - 5.0, row_y - 13.0),
                            egui::Align2::RIGHT_TOP,
                            format!("{}", cw.spx_variant),
                            egui::FontId::proportional(8.0),
                            self.colors.accent2,
                        );
                    }

                    if response.clicked() {
                        self.wallet_selector_open = !self.wallet_selector_open;
                        self.node_selector_open = false;
                    }

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
                            .strong()
                            .color(self.colors.text),
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
                        egui::RichText::new("DAO")
                            .size(8.0)
                            .strong()
                            .color(self.colors.text)
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::DaoOperations, "\u{2b21}", "Nervos DAO");

                ui.add_space(10.0);

                // Section: Setting
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("SETTING")
                            .size(8.0)
                            .strong()
                            .color(self.colors.text),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::NodeManager, "\u{25c9}", "Networks");
                self.draw_nav_item(ui, Tab::Wallets, "\u{25EB}", "Wallets");
                self.draw_nav_item(ui, Tab::Accounts, "\u{25ce}", "Accounts");
                self.draw_nav_item(ui, Tab::Multisig, "\u{1f512}", "Multisig");

                // ── Bottom: Lock Wallet ──
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(30.0);

                    if matches!(
                        self.auth_method,
                        Some(AuthMethod::Keychain) | Some(AuthMethod::Fido2 { .. })
                    ) {
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            let lock_btn = egui::Button::new(
                                egui::RichText::new("\u{1f512} Lock Wallet")
                                    .size(12.0)
                                    .color(self.colors.accent3),
                            )
                            .fill(egui::Color32::from_rgba_unmultiplied(
                                self.colors.accent3.r(),
                                self.colors.accent3.g(),
                                self.colors.accent3.b(),
                                12,
                            ))
                            .stroke(egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(
                                    self.colors.accent3.r(),
                                    self.colors.accent3.g(),
                                    self.colors.accent3.b(),
                                    40,
                                ),
                            ))
                            .corner_radius(8.0)
                            .min_size(egui::vec2(208.0, 34.0));

                            if ui.add(lock_btn).clicked() {
                                self.lock_wallet();
                            }
                        });
                    }

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

                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_space(24.0);

                        match self.active_tab {
                            Tab::Dashboard => self.show_dashboard_tab(ui),
                            Tab::Transfer => self.show_transfer_tab(ui),
                            Tab::DaoOperations => self.show_dao_tab(ui),
                            Tab::NodeManager => self.show_node_manager_tab(ui),
                            Tab::Accounts => self.show_accounts_tab(ui),
                            Tab::Multisig => self.show_multisig_tab(ui),
                            Tab::Wallets => self.show_wallets_tab(ui),
                        }
                    });
            });
    }

    pub(crate) fn show_locked(&mut self, ui: &mut egui::Ui) {
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

            match &self.auth_method {
                Some(AuthMethod::Fido2 { credential_id }) => {
                    let cred_id = credential_id.clone();
                    let button = egui::Button::new(
                        egui::RichText::new("Unlock with FIDO2")
                            .size(16.0)
                            .color(self.colors.bg),
                    )
                    .fill(self.colors.accent2)
                    .min_size(egui::vec2(280.0, 48.0));

                    if ui.add(button).clicked() {
                        self.unlock_with_fido2(&cred_id);
                    }
                }
                Some(AuthMethod::Keychain) => {
                    let label = format!("Unlock with {}", keychain::short_name());
                    let button = egui::Button::new(
                        egui::RichText::new(label).size(16.0).color(self.colors.bg),
                    )
                    .fill(self.colors.accent2)
                    .min_size(egui::vec2(280.0, 48.0));

                    if ui.add(button).clicked() {
                        self.unlock_with_keychain();
                    }
                }
                _ => {}
            }

            ui.add_space(24.0);
            ui.vertical_centered(|ui| self.show_status(ui));
        });
    }

    fn auth_button_style(&self, idx: u8) -> (egui::Color32, egui::Color32, egui::Stroke) {
        match idx {
            0 => (self.colors.accent, self.colors.bg, egui::Stroke::NONE),
            1 => (
                self.colors.surface,
                self.colors.text,
                egui::Stroke::new(1.0, self.colors.accent2),
            ),
            _ => (
                egui::Color32::TRANSPARENT,
                self.colors.text_muted,
                egui::Stroke::new(1.0, self.colors.border2),
            ),
        }
    }
}

fn variant_parts(v: SpxVariant) -> (&'static str, &'static str) {
    match v {
        SpxVariant::Sha2128S => ("SHA2", "128S"),
        SpxVariant::Sha2128F => ("SHA2", "128F"),
        SpxVariant::Shake128S => ("SHAKE", "128S"),
        SpxVariant::Shake128F => ("SHAKE", "128F"),
        SpxVariant::Sha2192S => ("SHA2", "192S"),
        SpxVariant::Sha2192F => ("SHA2", "192F"),
        SpxVariant::Shake192S => ("SHAKE", "192S"),
        SpxVariant::Shake192F => ("SHAKE", "192F"),
        SpxVariant::Sha2256S => ("SHA2", "256S"),
        SpxVariant::Sha2256F => ("SHA2", "256F"),
        SpxVariant::Shake256S => ("SHAKE", "256S"),
        SpxVariant::Shake256F => ("SHAKE", "256F"),
    }
}
