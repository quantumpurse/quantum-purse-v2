//! Setup and Locked screen rendering.

use crate::types::Tab;
use crate::App;
use ckb_node::NodeType;
use eframe::egui;
use qpv2_core::types::{AuthMethod, SpxVariant};

impl App {
    pub(crate) fn show_welcome(&mut self, ui: &mut egui::Ui) {
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

            // Setup card
            egui::Frame::new()
                .fill(self.colors.surface2)
                .corner_radius(16.0)
                .inner_margin(32.0)
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(ui, |ui| {
                    ui.set_max_width(400.0);

                    // Segmented toggle: New Wallet | Import Wallet
                    let seg_width = ui.available_width();
                    let seg_height = 36.0;
                    let seg_radius = 8.0;
                    let response = ui
                        .allocate_response(egui::vec2(seg_width, seg_height), egui::Sense::click());
                    let rect = response.rect;
                    let mid = rect.center().x;
                    let painter = ui.painter();

                    painter.rect_filled(rect, seg_radius, self.colors.surface);
                    painter.rect_stroke(
                        rect,
                        seg_radius,
                        egui::Stroke::new(1.0, self.colors.border),
                        egui::StrokeKind::Outside,
                    );

                    let left_rect =
                        egui::Rect::from_min_max(rect.left_top(), egui::pos2(mid, rect.bottom()));
                    let right_rect =
                        egui::Rect::from_min_max(egui::pos2(mid, rect.top()), rect.right_bottom());

                    if !self.import_mode {
                        painter.rect_filled(
                            left_rect.shrink(2.0),
                            seg_radius - 2.0,
                            self.colors.accent,
                        );
                    } else {
                        painter.rect_filled(
                            right_rect.shrink(2.0),
                            seg_radius - 2.0,
                            self.colors.accent,
                        );
                    }

                    let (left_text_color, right_text_color) = if !self.import_mode {
                        (self.colors.bg, self.colors.text_muted)
                    } else {
                        (self.colors.text_muted, self.colors.bg)
                    };

                    painter.text(
                        left_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "New Wallet",
                        egui::FontId::proportional(13.0),
                        left_text_color,
                    );
                    painter.text(
                        right_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Import Wallet",
                        egui::FontId::proportional(13.0),
                        right_text_color,
                    );

                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            self.import_mode = pos.x >= mid;
                        }
                    }

                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }

                    ui.add_space(20.0);

                    ui.label(
                        egui::RichText::new("WALLET NAME")
                            .size(10.0)
                            .color(self.colors.text_muted),
                    );
                    ui.add_space(6.0);
                    let name_field = egui::TextEdit::singleline(&mut self.new_wallet_name)
                        .hint_text("Enter a name for your wallet")
                        .desired_width(ui.available_width());
                    ui.add(name_field);

                    ui.add_space(16.0);

                    // Divider
                    let divider_rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [
                            divider_rect.left_top(),
                            egui::pos2(divider_rect.right(), divider_rect.top()),
                        ],
                        egui::Stroke::new(1.0, self.colors.border),
                    );
                    ui.add_space(1.0);

                    ui.add_space(20.0);

                    ui.label(
                        egui::RichText::new("SPHINCS+ VARIANT")
                            .size(10.0)
                            .color(self.colors.text_muted),
                    );
                    ui.add_space(6.0);

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

                    ui.add_space(8.0);

                    // Variant info pills
                    let (security, speed) = variant_info(self.selected_variant);
                    ui.horizontal(|ui| {
                        let pill = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
                            let galley = ui.painter().layout_no_wrap(
                                text.to_string(),
                                egui::FontId::proportional(10.0),
                                color,
                            );
                            let pad = egui::vec2(8.0, 3.0);
                            let size = galley.size() + pad * 2.0;
                            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            let tint = egui::Color32::from_rgba_unmultiplied(
                                color.r(),
                                color.g(),
                                color.b(),
                                20,
                            );
                            ui.painter().rect_filled(rect, 4.0, tint);
                            ui.painter().galley(rect.min + pad, galley, color);
                        };
                        pill(ui, security, self.colors.accent);
                        pill(ui, speed, self.colors.accent2);
                        if self.import_mode {
                            let word_count =
                                self.selected_variant.required_bip39_size_in_word_total();
                            pill(
                                ui,
                                &format!("Requires {} words", word_count),
                                self.colors.text_muted,
                            );
                        }
                    });

                    ui.add_space(24.0);

                    // Divider
                    let divider_rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [
                            divider_rect.left_top(),
                            egui::pos2(divider_rect.right(), divider_rect.top()),
                        ],
                        egui::Stroke::new(1.0, self.colors.border),
                    );
                    ui.add_space(1.0);

                    ui.add_space(20.0);

                    ui.label(
                        egui::RichText::new("AUTHENTICATION")
                            .size(10.0)
                            .color(self.colors.text_muted),
                    );
                    ui.add_space(10.0);

                    let verb = if self.import_mode { "Import" } else { "Create" };

                    {
                        let label = format!("{} with {}", verb, keychain::short_name());
                        let pk_button = egui::Button::new(
                            egui::RichText::new(label).size(15.0).color(self.colors.bg),
                        )
                        .fill(self.colors.accent)
                        .corner_radius(10.0)
                        .min_size(egui::vec2(field_width, 44.0));

                        if ui.add(pk_button).clicked() {
                            if self.import_mode {
                                self.import_seed_phrase_with_keychain(self.selected_variant);
                            } else {
                                self.create_wallet_with_keychain(self.selected_variant);
                            }
                        }

                        ui.add_space(8.0);
                    }

                    {
                        let fido2_btn = egui::Button::new(
                            egui::RichText::new(format!("{} with Security Key", verb))
                                .size(15.0)
                                .color(self.colors.text),
                        )
                        .fill(self.colors.surface)
                        .stroke(egui::Stroke::new(1.0, self.colors.accent2))
                        .corner_radius(10.0)
                        .min_size(egui::vec2(field_width, 44.0));

                        if ui.add(fido2_btn).clicked() {
                            if self.import_mode {
                                self.import_seed_phrase_with_fido2(self.selected_variant);
                            } else {
                                self.create_wallet_with_fido2(self.selected_variant);
                            }
                        }

                        ui.add_space(8.0);
                    }

                    let pw_btn = egui::Button::new(
                        egui::RichText::new(format!("{} with Password", verb))
                            .size(15.0)
                            .color(self.colors.text_muted),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, self.colors.border2))
                    .corner_radius(10.0)
                    .min_size(egui::vec2(field_width, 44.0));
                    if ui.add(pw_btn).clicked() {
                        if self.import_mode {
                            self.import_seed_phrase_with_password(self.selected_variant);
                        } else {
                            self.create_wallet_with_password(self.selected_variant);
                        }
                    }
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
                        egui::StrokeKind::Outside,
                    );

                    let inner = rect.shrink2(egui::vec2(12.0, 5.0));

                    painter.text(
                        inner.left_top(),
                        egui::Align2::LEFT_TOP,
                        "ACTIVE WALLET",
                        egui::FontId::proportional(8.0),
                        self.colors.text_muted,
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
                        egui::pos2(inner.right() - 45.0, row_y),
                        egui::Align2::RIGHT_TOP,
                        "\u{25bc}",
                        egui::FontId::proportional(9.0),
                        self.colors.text_muted,
                    );

                    // Variant badge (from cache — no disk I/O).
                    if let Some(cw) = self.wallet_cache.iter().find(|w| w.id == self.wallet_id) {
                        painter.text(
                            egui::pos2(inner.right() - 5.0, row_y),
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
                        egui::RichText::new("SETTING")
                            .size(8.0)
                            .color(self.colors.text_muted),
                    );
                });
                ui.add_space(4.0);
                self.draw_nav_item(ui, Tab::Accounts, "\u{25ce}", "Accounts");
                self.draw_nav_item(ui, Tab::Wallets, "\u{2318}", "Wallets");

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
                                egui::RichText::new("\u{1f512} Lock Wallet").size(12.0),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, self.colors.border))
                            .min_size(egui::vec2(194.0, 32.0));

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

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(24.0);

                    match self.active_tab {
                        Tab::Dashboard => self.show_dashboard_tab(ui),
                        Tab::Transfer => self.show_transfer_tab(ui),
                        Tab::DaoOperations => self.show_dao_tab(ui),
                        Tab::NodeManager => self.show_node_manager_tab(ui),
                        Tab::Accounts => self.show_accounts_tab(ui),
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
}

fn variant_info(v: SpxVariant) -> (&'static str, &'static str) {
    let security = match v {
        SpxVariant::Sha2128S
        | SpxVariant::Sha2128F
        | SpxVariant::Shake128S
        | SpxVariant::Shake128F => "128-bit security",
        SpxVariant::Sha2192S
        | SpxVariant::Sha2192F
        | SpxVariant::Shake192S
        | SpxVariant::Shake192F => "192-bit security",
        SpxVariant::Sha2256S
        | SpxVariant::Sha2256F
        | SpxVariant::Shake256S
        | SpxVariant::Shake256F => "256-bit security",
    };
    let speed = match v {
        SpxVariant::Sha2128S
        | SpxVariant::Sha2192S
        | SpxVariant::Sha2256S
        | SpxVariant::Shake128S
        | SpxVariant::Shake192S
        | SpxVariant::Shake256S => "Compact signatures",
        _ => "Fast signing",
    };
    (security, speed)
}
