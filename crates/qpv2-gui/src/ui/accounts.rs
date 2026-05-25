//! Accounts tab rendering.

use eframe::egui;
use qpv2_core::types::AuthMethod;
use qpv2_core::KeyVault;

use super::common::CardHover;
use crate::types::{format_ckb_balance, Status};
use crate::App;

impl App {
    pub(crate) fn show_accounts_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Accounts")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Manage wallets, keys, and authentication")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                // ── Action cards (3-column grid) ──
                ui.columns(3, |cols| {
                    // New Account
                    let hover = CardHover::new(&cols[0], "acct-new", &self.colors);

                    let new_card = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(18.0)
                        .inner_margin(egui::Margin::symmetric(20, 24))
                        .stroke(hover.stroke)
                        .show(&mut cols[0], |ui| {
                            ui.vertical_centered(|ui| {
                                hover.apply_lift(ui);
                                ui.label(egui::RichText::new("\u{2726}").size(30.0));
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("New Account")
                                        .size(14.0)
                                        .strong()
                                        .color(self.colors.text),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(
                                        "Derive a new account from your wallet seed.",
                                    )
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                                );
                            });
                        })
                        .response;

                    hover.commit(&new_card);

                    if new_card.interact(egui::Sense::click()).clicked() {
                        match &self.auth_method {
                            Some(AuthMethod::Password) => {
                                self.create_new_account_with_password();
                            }
                            Some(AuthMethod::Keychain) => {
                                self.create_new_account_with_keychain();
                            }
                            Some(AuthMethod::Fido2 { credential_id }) => {
                                let cred_id = credential_id.clone();
                                self.create_new_account_with_fido2(&cred_id);
                            }
                            None => {
                                self.status =
                                    Status::Error("No authentication method set.".to_string());
                            }
                        }
                    }

                    // Import (CLI only)
                    let hover = CardHover::new(&cols[1], "acct-import", &self.colors);

                    let import_card = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(18.0)
                        .inner_margin(egui::Margin::symmetric(20, 24))
                        .stroke(hover.stroke)
                        .show(&mut cols[1], |ui| {
                            ui.vertical_centered(|ui| {
                                hover.apply_lift(ui);
                                ui.label(egui::RichText::new("\u{2b07}").size(30.0));
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Import Seed")
                                        .size(14.0)
                                        .strong()
                                        .color(self.colors.text_muted),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("CLI only: qpv2 import-seed")
                                        .size(11.0)
                                        .color(self.colors.text_muted),
                                );
                            });
                        })
                        .response;

                    hover.commit(&import_card);

                    // Export Seed
                    let hover = CardHover::new(&cols[2], "acct-export", &self.colors);

                    let export_card = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(18.0)
                        .inner_margin(egui::Margin::symmetric(20, 24))
                        .stroke(hover.stroke)
                        .show(&mut cols[2], |ui| {
                            ui.vertical_centered(|ui| {
                                hover.apply_lift(ui);
                                ui.label(egui::RichText::new("\u{2b06}").size(30.0));
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Export Seed")
                                        .size(14.0)
                                        .strong()
                                        .color(self.colors.text),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("Make sure no one is watching.")
                                        .size(11.0)
                                        .color(self.colors.warn),
                                );
                            });
                        })
                        .response;

                    hover.commit(&export_card);

                    if export_card.interact(egui::Sense::click()).clicked() {
                        match &self.auth_method {
                            Some(AuthMethod::Password) => {
                                self.export_seed_phrase_with_password();
                            }
                            Some(AuthMethod::Keychain) => {
                                self.export_seed_phrase_with_keychain();
                            }
                            Some(AuthMethod::Fido2 { credential_id }) => {
                                let cred_id = credential_id.clone();
                                self.export_seed_phrase_with_fido2(&cred_id);
                            }
                            None => {
                                self.status =
                                    Status::Error("No authentication method set.".to_string());
                            }
                        }
                    }
                });

                ui.add_space(18.0);

                // ── Section title ──
                // Shared pill builder: identical shape/size/font across all badges.
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
                        egui::RichText::new("Saved Accounts")
                            .size(15.0)
                            .strong()
                            .color(self.colors.text),
                    );
                    ui.add_space(10.0);
                    pill(
                        ui,
                        self.colors.accent_tint,
                        format!("{} total", self.accounts.len()),
                        self.colors.accent,
                    );

                    if let Ok(info) = KeyVault::read_wallet_info(self.wallet_id) {
                        ui.add_space(6.0);
                        pill(
                            ui,
                            self.colors.surface2,
                            format!("SPHINCS+ {}", info.spx_variant),
                            self.colors.text_muted,
                        );
                        ui.add_space(6.0);
                        pill(
                            ui,
                            self.colors.accent2_tint,
                            match info.auth_method {
                                AuthMethod::Keychain => keychain::short_name().into(),
                                AuthMethod::Password => "Password".into(),
                                AuthMethod::Fido2 { .. } => "FIDO2 Key".into(),
                            },
                            self.colors.accent2,
                        );
                    }
                });

                ui.add_space(10.0);

                // ── Account list ──
                if self.accounts.is_empty() {
                    ui.label(
                        egui::RichText::new("No accounts yet. Create one to get started.")
                            .color(self.colors.text_muted),
                    );
                } else {
                    let avatar_colors = [
                        (self.colors.accent, egui::Color32::from_rgb(5, 12, 10)),
                        (self.colors.accent3, egui::Color32::WHITE),
                        (self.colors.warn, egui::Color32::from_rgb(5, 12, 10)),
                    ];

                    for (i, lock_args) in self.accounts.clone().iter().enumerate() {
                        let address_text = match crate::ckb::lock_args_to_address(
                            lock_args,
                            self.qp_client.is_mainnet(),
                        ) {
                            Ok(addr) => addr,
                            Err(_) => format!("0x{}", lock_args),
                        };

                        let balance_text = match self.balances.get(lock_args) {
                            Some(Some(shannons)) => format_ckb_balance(*shannons),
                            Some(None) => "Loading...".to_string(),
                            None => "--".to_string(),
                        };

                        let (av_bg, av_fg) = avatar_colors[i % avatar_colors.len()];

                        let hover = CardHover::new(ui, ("acct-row", i), &self.colors);

                        let row_resp = egui::Frame::new()
                            .fill(hover.fill)
                            .corner_radius(9.0)
                            .inner_margin(egui::Margin::symmetric(18, 14))
                            .stroke(hover.stroke)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Avatar circle
                                    let (avatar_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(38.0, 38.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter()
                                        .circle_filled(avatar_rect.center(), 19.0, av_bg);
                                    let letter = (b'A' + (i as u8 % 26)) as char;
                                    ui.painter().text(
                                        avatar_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        letter.to_string(),
                                        egui::FontId::proportional(15.0),
                                        av_fg,
                                    );

                                    ui.add_space(10.0);

                                    // Info
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("Account #{}", i))
                                                .size(13.0),
                                        );
                                        ui.label(
                                            egui::RichText::new(&address_text)
                                                .size(9.0)
                                                .color(self.colors.text_muted)
                                                .family(egui::FontFamily::Monospace),
                                        );
                                    });

                                    // Balance + copy (right-aligned)
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .button("\u{1f4cb}")
                                                .on_hover_text("Copy address")
                                                .clicked()
                                            {
                                                ui.ctx().copy_text(address_text.clone());
                                                self.status =
                                                    Status::Info("Address copied!".to_string());
                                            }

                                            ui.add_space(8.0);

                                            ui.vertical(|ui| {
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Min),
                                                    |ui| {
                                                        ui.label(
                                                            egui::RichText::new(&balance_text)
                                                                .size(15.0)
                                                                .strong()
                                                                .color(self.colors.text_muted)
                                                                .family(
                                                                    egui::FontFamily::Monospace,
                                                                ),
                                                        );
                                                    },
                                                );
                                            });
                                        },
                                    );
                                });
                            });

                        hover.commit(&row_resp.response);

                        ui.add_space(6.0);
                    }
                }

                ui.add_space(16.0);

                self.show_status(ui);
            }); // vertical
        }); // horizontal
    }
}
