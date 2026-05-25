//! Wallets tab rendering — create, inspect, and delete wallets.

use eframe::egui;
use qpv2_core::types::{AuthMethod, SpxVariant};
use qpv2_core::KeyVault;

use super::common::CardHover;
use crate::types::Status;
use crate::App;

impl App {
    pub(crate) fn show_wallets_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Wallets")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Create, manage, and inspect your wallets.")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                // ── Create Wallet card ──
                egui::Frame::new()
                    .fill(self.colors.surface2)
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(20, 18))
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("CREATE WALLET")
                                .size(8.5)
                                .family(egui::FontFamily::Monospace)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(10.0);

                        // Name + Variant row
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Name")
                                        .size(10.0)
                                        .color(self.colors.text_muted),
                                );
                                ui.add_space(2.0);
                                let name_field =
                                    egui::TextEdit::singleline(&mut self.new_wallet_name)
                                        .hint_text("Wallet name")
                                        .desired_width(180.0);
                                ui.add(name_field);
                            });

                            ui.add_space(16.0);

                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Variant")
                                        .size(10.0)
                                        .color(self.colors.text_muted),
                                );
                                ui.add_space(2.0);
                                egui::ComboBox::from_id_salt("wallets_tab_variant")
                                    .selected_text(format!("{}", self.new_wallet_variant))
                                    .width(160.0)
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
                                                &mut self.new_wallet_variant,
                                                *variant,
                                                format!("{}", variant),
                                            );
                                        }
                                    });
                            });
                        });

                        ui.add_space(12.0);

                        // Import mode toggle
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.import_mode, "");
                            ui.label(
                                egui::RichText::new("Import from seed phrase")
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                            );
                        });

                        ui.add_space(12.0);

                        // Auth method buttons
                        let verb = if self.import_mode { "Import" } else { "Create" };

                        ui.horizontal(|ui| {
                            let kc_btn = egui::Button::new(
                                egui::RichText::new(format!(
                                    "{} with {}",
                                    verb,
                                    keychain::short_name()
                                ))
                                .size(12.0)
                                .color(self.colors.bg),
                            )
                            .fill(self.colors.accent)
                            .corner_radius(6.0)
                            .min_size(egui::vec2(0.0, 30.0));

                            if ui.add(kc_btn).clicked() {
                                let v = self.new_wallet_variant;
                                if self.import_mode {
                                    self.import_seed_phrase_with_keychain(v);
                                } else {
                                    self.create_wallet_with_keychain(v);
                                }
                            }

                            let fido_btn = egui::Button::new(
                                egui::RichText::new(format!("{} with Security Key", verb))
                                    .size(12.0)
                                    .color(self.colors.text),
                            )
                            .fill(self.colors.surface)
                            .stroke(egui::Stroke::new(1.0, self.colors.accent2))
                            .corner_radius(6.0)
                            .min_size(egui::vec2(0.0, 30.0));

                            if ui.add(fido_btn).clicked() {
                                let v = self.new_wallet_variant;
                                if self.import_mode {
                                    self.import_seed_phrase_with_fido2(v);
                                } else {
                                    self.create_wallet_with_fido2(v);
                                }
                            }

                            let pw_btn = egui::Button::new(
                                egui::RichText::new(format!("{} with Password", verb))
                                    .size(12.0)
                                    .color(self.colors.text_muted),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, self.colors.border2))
                            .corner_radius(6.0)
                            .min_size(egui::vec2(0.0, 30.0));

                            if ui.add(pw_btn).clicked() {
                                let v = self.new_wallet_variant;
                                if self.import_mode {
                                    self.import_seed_phrase_with_password(v);
                                } else {
                                    self.create_wallet_with_password(v);
                                }
                            }
                        });
                    });

                ui.add_space(22.0);

                // ── Saved Wallets section ──
                let wallet_count = self.wallet_cache.len();

                let pill =
                    |ui: &mut egui::Ui, fill: egui::Color32, text: String, color: egui::Color32| {
                        egui::Frame::new()
                            .fill(fill)
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(8.5)
                                        .family(egui::FontFamily::Monospace)
                                        .color(color),
                                );
                            });
                    };

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Saved Wallets")
                            .size(15.0)
                            .strong()
                            .color(self.colors.text),
                    );
                    ui.add_space(10.0);
                    pill(
                        ui,
                        self.colors.accent_tint,
                        format!("{} total", wallet_count),
                        self.colors.accent,
                    );
                });

                ui.add_space(12.0);

                if self.wallet_cache.is_empty() {
                    ui.label(
                        egui::RichText::new("No wallets yet. Create one above.")
                            .color(self.colors.text_muted),
                    );
                } else {
                    let avatar_colors = [
                        (self.colors.accent, egui::Color32::from_rgb(5, 12, 10)),
                        (self.colors.accent3, egui::Color32::WHITE),
                        (self.colors.warn, egui::Color32::from_rgb(5, 12, 10)),
                    ];

                    let mut delete_target: Option<u32> = None;

                    for i in 0..self.wallet_cache.len() {
                        let cw = &self.wallet_cache[i];
                        let is_active = cw.id == self.wallet_id;
                        let (av_bg, av_fg) = avatar_colors[i % avatar_colors.len()];

                        let hover = CardHover::new(ui, ("wallet-row", i), &self.colors);

                        let cw_id = cw.id;
                        let cw_name = cw.name.clone();
                        let cw_variant = cw.spx_variant;
                        let cw_auth = cw.auth_method.clone();
                        let cw_acct_count = cw.account_count;
                        let cw_path = cw.path.clone();

                        let row_resp = egui::Frame::new()
                            .fill(hover.fill)
                            .corner_radius(9.0)
                            .inner_margin(egui::Margin::symmetric(18, 14))
                            .stroke(hover.stroke)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Avatar
                                    let (avatar_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(38.0, 38.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter()
                                        .circle_filled(avatar_rect.center(), 19.0, av_bg);
                                    let letter = cw_name
                                        .chars()
                                        .next()
                                        .unwrap_or('?')
                                        .to_uppercase()
                                        .next()
                                        .unwrap_or('?');
                                    ui.painter().text(
                                        avatar_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        letter.to_string(),
                                        egui::FontId::proportional(15.0),
                                        av_fg,
                                    );

                                    ui.add_space(10.0);

                                    // Info column
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(&cw_name)
                                                .size(14.0)
                                                .strong()
                                                .color(self.colors.text),
                                        );

                                        // Detail pills row
                                        ui.horizontal(|ui| {
                                            pill(
                                                ui,
                                                self.colors.surface2,
                                                format!("{}", cw_variant),
                                                self.colors.text_muted,
                                            );
                                            let (auth_label, auth_color) = match &cw_auth {
                                                AuthMethod::Password => {
                                                    ("Password", self.colors.text_muted)
                                                }
                                                AuthMethod::Keychain => {
                                                    (keychain::short_name(), self.colors.accent2)
                                                }
                                                AuthMethod::Fido2 { .. } => {
                                                    ("FIDO2 Key", self.colors.accent3)
                                                }
                                            };
                                            pill(
                                                ui,
                                                egui::Color32::from_rgba_unmultiplied(
                                                    auth_color.r(),
                                                    auth_color.g(),
                                                    auth_color.b(),
                                                    20,
                                                ),
                                                auth_label.to_string(),
                                                auth_color,
                                            );

                                            let acct_text = if cw_acct_count == 1 {
                                                "1 account".to_string()
                                            } else {
                                                format!("{} accounts", cw_acct_count)
                                            };
                                            pill(
                                                ui,
                                                self.colors.surface2,
                                                acct_text,
                                                self.colors.text_muted,
                                            );
                                        });

                                        // Path
                                        if !cw_path.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&cw_path)
                                                    .size(9.0)
                                                    .family(egui::FontFamily::Monospace)
                                                    .color(self.colors.text_muted),
                                            );
                                        }
                                    });

                                    // Right side: ACTIVE badge + Delete button
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let confirming =
                                                self.confirm_remove_id == Some(cw_id);
                                            let label = if confirming {
                                                "\u{26a0} Confirm delete?"
                                            } else {
                                                "\u{1f5d1} Delete"
                                            };
                                            let del_btn = egui::Button::new(
                                                egui::RichText::new(label)
                                                    .size(10.0)
                                                    .color(self.colors.danger),
                                            )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .stroke(egui::Stroke::new(
                                                1.0,
                                                egui::Color32::from_rgba_unmultiplied(
                                                        255, 77, 109, 77,
                                                    ),
                                                ))
                                                .corner_radius(6.0);

                                                if ui.add(del_btn).clicked() {
                                                    if confirming {
                                                        delete_target = Some(cw_id);
                                                    } else {
                                                        self.confirm_remove_id = Some(cw_id);
                                                    }
                                                }

                                            if is_active {
                                                pill(
                                                    ui,
                                                    self.colors.accent_tint,
                                                    "ACTIVE".to_string(),
                                                    self.colors.accent,
                                                );
                                            }
                                        },
                                    );
                                });
                            });

                        hover.commit(&row_resp.response);
                        ui.add_space(6.0);
                    }

                    // Handle delete outside the iteration to avoid borrow issues.
                    if let Some(id) = delete_target {
                        let _ = keychain::delete_key(id);
                        let _ = ckb_node::wallet_helpers::lc::clear_all_scripts(&self.qp_client);

                        match KeyVault::remove_wallet(id) {
                            Ok(()) => {
                                self.confirm_remove_id = None;
                                if id == self.wallet_id {
                                    self.lock_wallet();
                                    self.refresh_wallet_cache();
                                    if let Some(first) = self.wallet_cache.first() {
                                        let fid = first.id;
                                        let fname = first.name.clone();
                                        self.switch_wallet(fid, &fname);
                                    } else {
                                        self.wallet_id = 0;
                                        self.wallet_name.clear();
                                        self.screen = crate::types::Screen::Setup;
                                    }
                                } else {
                                    self.refresh_wallet_cache();
                                }
                                self.status =
                                    Status::Info("Wallet removed successfully.".to_string());
                            }
                            Err(e) => {
                                self.confirm_remove_id = None;
                                self.status =
                                    Status::Error(format!("Failed to remove wallet: {}", e));
                            }
                        }
                    }
                }

                ui.add_space(16.0);
                self.show_status(ui);
            });
        });
    }
}
