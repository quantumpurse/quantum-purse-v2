//! Dashboard tab rendering.

use eframe::egui;

use crate::types::{format_ckb_balance, Tab};
use crate::App;

impl App {
    pub(crate) fn show_dashboard_tab(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Padded content wrapper — used for sections that need horizontal margins.
        let content_padding = 30.0;

        // Top bar: title + subtitle (padded)
        ui.horizontal(|ui| {
            ui.add_space(content_padding);
            ui.vertical(|ui| {
                ui.heading(
                    egui::RichText::new("Dashboard")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Portfolio overview & activity")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );
            });
        });

        ui.add_space(22.0);

        // ── Balance hero card (full width) ──
        egui::Frame::new()
            .fill(egui::Color32::from_rgba_unmultiplied(0, 255, 180, 8))
            .corner_radius(20.0)
            .outer_margin(egui::Margin::symmetric(30, 0))
            .inner_margin(egui::Margin::symmetric(34, 30))
            .stroke(egui::Stroke::new(1.0, self.colors.border2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.label(
                    egui::RichText::new("TOTAL BALANCE")
                        .size(10.0)
                        .color(self.colors.text_muted)
                        .family(egui::FontFamily::Monospace),
                );
                ui.add_space(6.0);

                // Sum all balances
                let total_shannons: u64 = self
                    .balances
                    .values()
                    .filter_map(|b| b.as_ref().copied())
                    .sum();

                ui.label(
                    egui::RichText::new(format_ckb_balance(total_shannons))
                        .size(42.0)
                        .strong()
                        .color(self.colors.text),
                );

                ui.add_space(16.0);

                // Meta row separator
                ui.horizontal(|ui| {
                    let rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [rect.left_top(), egui::pos2(rect.right(), rect.top())],
                        egui::Stroke::new(1.0, self.colors.border),
                    );
                });
                ui.add_space(12.0);

                // DAO Locked — sum of deposited + prepared cell capacities across all accounts.
                let dao_locked: u64 = self
                    .dao_deposited_cells
                    .iter()
                    .map(|(_, c)| c.capacity)
                    .chain(self.dao_prepared_cells.iter().map(|(_, c)| c.capacity))
                    .sum();
                let available = total_shannons.saturating_sub(dao_locked);

                ui.horizontal(|ui| {
                    // Available (total minus DAO-locked)
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("AVAILABLE")
                                .size(9.0)
                                .color(self.colors.text_muted)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(format_ckb_balance(available))
                                .size(15.0)
                                .strong()
                                .color(self.colors.accent),
                        );
                    });

                    ui.add_space(30.0);

                    // Accounts
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("ACCOUNTS")
                                .size(9.0)
                                .color(self.colors.text_muted)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(format!("{}", self.accounts.len()))
                                .size(15.0)
                                .strong()
                                .color(self.colors.accent2),
                        );
                    });

                    ui.add_space(30.0);

                    // DAO Locked
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("DAO LOCKED")
                                .size(9.0)
                                .color(self.colors.text_muted)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(
                            egui::RichText::new(format_ckb_balance(dao_locked))
                                .size(15.0)
                                .strong()
                                .color(self.colors.accent3),
                        );
                    });
                });
            });

        ui.add_space(16.0);

        // Remaining content is padded.
        ui.horizontal(|ui| {
            ui.add_space(content_padding);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - content_padding);

                // ── Quick actions ──
                ui.columns(4, |cols| {
                    let actions = [
                        ("\u{2191}", "Send", Tab::Transfer),
                        ("\u{2193}", "Receive", Tab::Accounts),
                        ("\u{2b21}", "DAO", Tab::DaoOperations),
                        ("\u{25ce}", "Accounts", Tab::Accounts),
                    ];

                    for (i, (icon, label, target_tab)) in actions.iter().enumerate() {
                        let response = egui::Frame::new()
                            .fill(self.colors.surface)
                            .corner_radius(16.0)
                            .inner_margin(egui::Margin::symmetric(10, 16))
                            .stroke(egui::Stroke::new(1.0, self.colors.border))
                            .show(&mut cols[i], |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new(*icon)
                                            .size(20.0)
                                            .color(self.colors.text_muted),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(*label)
                                            .size(12.0)
                                            .color(self.colors.text_muted),
                                    );
                                });
                            })
                            .response;

                        if response.interact(egui::Sense::click()).clicked() {
                            self.active_tab = *target_tab;
                        }
                    }
                });

                ui.add_space(20.0);

                // ── Status messages ──
                self.show_status(ui);
            });
        });

        let _ = frame; // Suppress unused warning.
    }
}
