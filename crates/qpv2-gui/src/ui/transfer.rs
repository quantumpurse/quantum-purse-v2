//! Transfer tab rendering.

use eframe::egui;

use crate::types::{format_ckb_balance, TransactionStatus};
use crate::App;

impl App {
    pub(crate) fn show_transfer_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Transfer CKB")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Send CKB to any Nervos address.")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                // Show success/error status from previous transfer
                match &self.tx_status {
                    TransactionStatus::Success(tx_hash) => {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_unmultiplied(0, 255, 136, 20))
                            .corner_radius(12.0)
                            .inner_margin(egui::Margin::symmetric(20, 14))
                            .show(ui, |ui| {
                                ui.set_max_width(560.0);
                                ui.label(
                                    egui::RichText::new("Transaction sent successfully!")
                                        .strong()
                                        .color(self.colors.accent),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Tx: 0x{}..{}",
                                            &tx_hash[..8],
                                            &tx_hash[tx_hash.len() - 8..]
                                        ))
                                        .size(11.0)
                                        .color(self.colors.text_muted)
                                        .family(egui::FontFamily::Monospace),
                                    );
                                    if ui.small_button("Copy").clicked() {
                                        ui.ctx().copy_text(format!("0x{}", tx_hash));
                                    }
                                });
                            });
                        ui.add_space(12.0);
                    }
                    TransactionStatus::Error(msg) => {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_unmultiplied(255, 70, 70, 20))
                            .corner_radius(12.0)
                            .inner_margin(egui::Margin::symmetric(20, 14))
                            .show(ui, |ui| {
                                ui.set_max_width(560.0);
                                ui.label(
                                    egui::RichText::new(format!("Error: {}", msg))
                                        .color(self.colors.danger),
                                );
                            });
                        ui.add_space(12.0);
                    }
                    _ => {}
                }

                egui::Frame::new()
                    .fill(self.colors.surface)
                    .corner_radius(20.0)
                    .inner_margin(egui::Margin::symmetric(30, 26))
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);

                        let is_busy = !matches!(
                            self.tx_status,
                            TransactionStatus::Idle
                                | TransactionStatus::Success(_)
                                | TransactionStatus::Error(_)
                        );
                        let is_calculating_max = self.spendable_capacity_rx.is_some();

                        // ── From Account ──
                        ui.label(
                            egui::RichText::new("From")
                                .size(12.0)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(4.0);

                        let from_text = if self.accounts.is_empty() {
                            "No accounts available".to_string()
                        } else {
                            let idx = self.transfer_from_account.min(self.accounts.len() - 1);
                            let lock_args = &self.accounts[idx];
                            let bal = self
                                .balances
                                .get(lock_args)
                                .and_then(|b| b.as_ref())
                                .copied();
                            let bal_str = match bal {
                                Some(b) => format_ckb_balance(b),
                                None => "--".to_string(),
                            };
                            format!("Account #{} ({})", idx, bal_str)
                        };

                        let prev_from_account = self.transfer_from_account;
                        egui::ComboBox::from_id_salt("transfer_from")
                            .selected_text(&from_text)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for (i, lock_args) in self.accounts.iter().enumerate() {
                                    let bal = self
                                        .balances
                                        .get(lock_args)
                                        .and_then(|b| b.as_ref())
                                        .copied();
                                    let label = match bal {
                                        Some(b) => {
                                            format!("Account #{} ({})", i, format_ckb_balance(b))
                                        }
                                        None => format!("Account #{}", i),
                                    };
                                    ui.selectable_value(&mut self.transfer_from_account, i, label);
                                }
                            });
                        // Clear send_all if the user switches accounts.
                        if self.transfer_from_account != prev_from_account && self.transfer_all
                        {
                            self.transfer_all = false;
                            self.transfer_amount.clear();
                        }

                        ui.add_space(16.0);

                        // ── Recipient Address ──
                        ui.label(
                            egui::RichText::new("To")
                                .size(12.0)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(4.0);

                        let recipient_edit =
                            egui::TextEdit::singleline(&mut self.transfer_recipient)
                                .hint_text("ckt1q... or ckb1q...")
                                .desired_width(ui.available_width())
                                .font(egui::FontId::monospace(13.0))
                                .interactive(!is_busy);
                        ui.add(recipient_edit);

                        ui.add_space(16.0);

                        // ── Amount ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Amount (CKB)")
                                    .size(12.0)
                                    .color(self.colors.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Clear button when send_all is active.
                                    if self.transfer_all && !is_busy {
                                        if ui.small_button("✕").clicked() {
                                            self.transfer_all = false;
                                            self.transfer_amount.clear();
                                        }
                                    }

                                    let can_calculate_max = !is_busy
                                        && !is_calculating_max
                                        && !self.transfer_all
                                        && !self.accounts.is_empty();
                                    let max_label = if is_calculating_max { "..." } else { "MAX" };
                                    if ui
                                        .add_enabled(
                                            can_calculate_max,
                                            egui::Button::new(max_label).small(),
                                        )
                                        .clicked()
                                    {
                                        self.fetch_spendable_capacity();
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);

                        let amount_interactive = !is_busy && !self.transfer_all;
                        let amount_edit = egui::TextEdit::singleline(&mut self.transfer_amount)
                            .hint_text("0.0")
                            .desired_width(ui.available_width())
                            .font(egui::FontId::monospace(13.0))
                            .interactive(amount_interactive);
                        ui.add(amount_edit);

                        if self.transfer_all {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Fee will be deducted at send time.")
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                            );
                        } else if is_calculating_max {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Fetching spendable balance...")
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                            );
                        }

                        ui.add_space(16.0);

                        // ── Fee Rate (collapsible) ──
                        egui::CollapsingHeader::new(
                            egui::RichText::new("Advanced")
                                .size(12.0)
                                .color(self.colors.text_muted),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Fee rate (shannons/KB)")
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                            );
                            ui.add_space(4.0);
                            let fee_edit = egui::TextEdit::singleline(&mut self.transfer_fee_rate)
                                .hint_text("1000")
                                .desired_width(120.0)
                                .font(egui::FontId::monospace(12.0))
                                .interactive(!is_busy);
                            ui.add(fee_edit);
                        });

                        ui.add_space(20.0);

                        // ── Send Button ──
                        let connected = self.rpc_client.is_some();
                        let has_accounts = !self.accounts.is_empty();
                        let can_send = connected
                            && has_accounts
                            && !is_busy
                            && !self.transfer_recipient.is_empty()
                            && !self.transfer_amount.is_empty();

                        let btn_text = match &self.tx_status {
                            TransactionStatus::Building => "Building transaction...",
                            TransactionStatus::AwaitingSignature => "Waiting for Touch ID...",
                            TransactionStatus::Sending => "Sending...",
                            _ => "Send",
                        };

                        let send_btn =
                            egui::Button::new(egui::RichText::new(btn_text).size(15.0).strong())
                                .fill(if can_send {
                                    self.colors.accent
                                } else {
                                    self.colors.surface2
                                })
                                .min_size(egui::vec2(ui.available_width(), 44.0));

                        if ui.add_enabled(can_send, send_btn).clicked() {
                            self.transfer_async();
                        }

                        if !connected {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Not connected to node.")
                                    .size(11.0)
                                    .color(self.colors.warn),
                            );
                        }
                    });
            }); // vertical
        }); // horizontal
    }
}
