//! Multisig tab rendering — multisig accounts only.

use eframe::egui;

use super::utils::{paint_corner_accent, CardHover};
use crate::types::Status;
use crate::utils::format_ckb_balance;
use crate::App;

impl App {
    pub(crate) fn show_multisig_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Multisig")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Create and manage multi-signature accounts.")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                // ── New Multisig card ──
                let hover = CardHover::new(ui, "acct-multisig", &self.colors);

                let multisig_card = egui::Frame::new()
                    .fill(hover.fill)
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(20, 24))
                    .stroke(hover.stroke)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            hover.apply_lift(ui);
                            ui.label(egui::RichText::new("\u{1f512}").size(26.0));
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Create Multi-sig Account")
                                    .size(14.0)
                                    .strong()
                                    .color(self.colors.text),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Set up an M-of-N multisig address.")
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                            );
                        });
                    })
                    .response;

                paint_corner_accent(ui.painter(), multisig_card.rect, 18.0, self.colors.accent2);
                hover.commit(&multisig_card);

                if multisig_card.interact(egui::Sense::click()).clicked() {
                    self.multisig_local_signer_idx = 0;
                    self.multisig_threshold = 2;
                    self.multisig_required_first_n = 0;
                    self.multisig_co_signers = vec![];
                    self.multisig_modal_open = true;
                }

                ui.add_space(20.0);

                // ── Section title ──
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

                let multisig_accounts: Vec<_> = self
                    .accounts
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.config.signers.len() > 1)
                    .collect();

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Multisig Accounts")
                            .size(15.0)
                            .strong()
                            .color(self.colors.text),
                    );
                    ui.add_space(10.0);
                    pill(
                        ui,
                        self.colors.accent2_tint,
                        format!("{} total", multisig_accounts.len()),
                        self.colors.accent2,
                    );

                    ui.add_space(10.0);
                    self.show_status(ui);
                });

                ui.add_space(10.0);

                // ── Account list (multisig only) ──
                if multisig_accounts.is_empty() {
                    ui.label(
                        egui::RichText::new("No multisig accounts yet. Create one to get started.")
                            .color(self.colors.text_muted),
                    );
                } else {
                    for (i, account) in multisig_accounts {
                        let lock_args = &account.lock_args;
                        let address_text = match crate::utils::lock_args_to_address(
                            lock_args,
                            self.qp_client.is_mainnet(),
                        ) {
                            Ok(addr) => addr.to_string(),
                            Err(_) => format!("0x{}", lock_args),
                        };

                        let balance_text = match self.spendable_balances.get(lock_args) {
                            Some(Some(shannons)) => format_ckb_balance(*shannons),
                            Some(None) => "Loading...".to_string(),
                            None => "--".to_string(),
                        };

                        let n_signers = account.config.signers.len();

                        let hover = CardHover::new(ui, ("msig-row", i), &self.colors);

                        let row_resp = egui::Frame::new()
                            .fill(hover.fill)
                            .corner_radius(9.0)
                            .inner_margin(egui::Margin::symmetric(18, 14))
                            .stroke(hover.stroke)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Avatar: wedge per signer
                                    let (avatar_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(38.0, 38.0),
                                        egui::Sense::hover(),
                                    );
                                    let center = avatar_rect.center();
                                    let radius = 19.0;

                                    let wedge_colors = [
                                        self.colors.accent,
                                        self.colors.accent2,
                                        self.colors.accent3,
                                        self.colors.warn,
                                    ];
                                    let wedge_fg = [
                                        egui::Color32::from_rgb(5, 12, 10),
                                        egui::Color32::from_rgb(5, 12, 10),
                                        egui::Color32::WHITE,
                                        egui::Color32::from_rgb(5, 12, 10),
                                    ];
                                    let n = n_signers.min(wedge_colors.len());
                                    let angle_step = std::f32::consts::TAU / n as f32;
                                    let start_offset = -std::f32::consts::FRAC_PI_2;

                                    for s in 0..n {
                                        let a0 = start_offset + s as f32 * angle_step;
                                        let a1 = a0 + angle_step;
                                        let color = wedge_colors[s % wedge_colors.len()];

                                        let segments = 16;
                                        let mut points = vec![center];
                                        for seg in 0..=segments {
                                            let a = a0 + (a1 - a0) * seg as f32 / segments as f32;
                                            points.push(
                                                center
                                                    + egui::vec2(
                                                        a.cos() * radius,
                                                        a.sin() * radius,
                                                    ),
                                            );
                                        }
                                        ui.painter().add(egui::Shape::convex_polygon(
                                            points,
                                            color,
                                            egui::Stroke::NONE,
                                        ));

                                        let letter = if let Some(signer) =
                                            account.config.signers.get(s)
                                        {
                                            let byte = signer.pubkey.first().copied().unwrap_or(0);
                                            (b'A' + (byte % 26)) as char
                                        } else {
                                            '?'
                                        };
                                        let mid_angle = (a0 + a1) / 2.0;
                                        let text_r = radius * 0.55;
                                        let text_pos = center
                                            + egui::vec2(
                                                mid_angle.cos() * text_r,
                                                mid_angle.sin() * text_r,
                                            );
                                        ui.painter().text(
                                            text_pos,
                                            egui::Align2::CENTER_CENTER,
                                            letter.to_string(),
                                            egui::FontId::proportional(10.0),
                                            wedge_fg[s % wedge_fg.len()],
                                        );
                                    }

                                    ui.painter().circle_stroke(
                                        center,
                                        radius,
                                        egui::Stroke::new(1.0, self.colors.border2),
                                    );

                                    ui.add_space(10.0);

                                    // Info
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(format!("Account #{}", i))
                                                    .size(13.0),
                                            );
                                            egui::Frame::new()
                                                .fill(self.colors.accent2_tint)
                                                .corner_radius(8.0)
                                                .inner_margin(egui::Margin::symmetric(6, 1))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "{}-of-{}",
                                                            account.config.threshold, n_signers
                                                        ))
                                                        .size(9.0)
                                                        .color(self.colors.accent2)
                                                        .family(egui::FontFamily::Monospace),
                                                    );
                                                });
                                            ui.label(
                                                egui::RichText::new(&balance_text)
                                                    .size(13.0)
                                                    .strong()
                                                    .color(self.colors.text_muted)
                                                    .family(egui::FontFamily::Monospace),
                                            );
                                        });
                                        ui.label(
                                            egui::RichText::new(address_text.clone())
                                                .size(9.0)
                                                .color(self.colors.text_muted)
                                                .family(egui::FontFamily::Monospace),
                                        );

                                        // Signer list
                                        ui.add_space(4.0);
                                        for (si, signer) in
                                            account.config.signers.iter().enumerate()
                                        {
                                            let pk_hex = hex::encode(&signer.pubkey);
                                            let pk_short = if pk_hex.len() > 40 {
                                                format!(
                                                    "{}...{}",
                                                    &pk_hex[..20],
                                                    &pk_hex[pk_hex.len() - 20..]
                                                )
                                            } else {
                                                pk_hex
                                            };
                                            let is_local = account
                                                .initiating_signer_lock_args
                                                .as_ref()
                                                .and_then(|la| {
                                                    self.accounts.iter().find(|a| {
                                                        a.lock_args == *la
                                                            && a.config.is_single_sig()
                                                    })
                                                })
                                                .is_some_and(|a| {
                                                    a.config.signers[0].pubkey == signer.pubkey
                                                });
                                            let label = if is_local {
                                                format!(
                                                    "  {} {} {} (you)",
                                                    si, signer.variant, pk_short
                                                )
                                            } else {
                                                format!("  {} {} {}", si, signer.variant, pk_short)
                                            };
                                            ui.label(
                                                egui::RichText::new(label)
                                                    .size(9.0)
                                                    .color(if is_local {
                                                        self.colors.accent
                                                    } else {
                                                        self.colors.text_muted
                                                    })
                                                    .family(egui::FontFamily::Monospace),
                                            );
                                        }
                                    });

                                    // Copy address button (right-aligned)
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
                                        },
                                    );
                                });
                            });

                        hover.commit(&row_resp.response);

                        ui.add_space(6.0);
                    }
                }

                // ── Co-signer signing flow ──
                self.show_sign_request_ui(ui);

                ui.add_space(20.0);
            }); // vertical
        }); // horizontal
    }
}
