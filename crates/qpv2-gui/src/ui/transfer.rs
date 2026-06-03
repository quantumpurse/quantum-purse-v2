//! Transfer tab rendering.

use std::collections::HashSet;

use eframe::egui;

use crate::types::{TransactionStatus, TxKind};
use crate::utils::{format_ckb, format_ckb_balance};
use crate::App;

impl App {
    pub(crate) fn show_transfer_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Transfer")
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

                let is_busy = !matches!(
                    self.tx_status,
                    TransactionStatus::Idle
                        | TransactionStatus::Success(_)
                        | TransactionStatus::Error(_)
                );

                let accent_tint = egui::Color32::from_rgba_unmultiplied(
                    self.colors.accent.r(),
                    self.colors.accent.g(),
                    self.colors.accent.b(),
                    10,
                );
                egui::Frame::new()
                    .fill(accent_tint)
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(28, 26))
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .show(ui, |ui| {

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
                                .spendable_balances
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
                                        .spendable_balances
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
                        if self.transfer_from_account != prev_from_account && self.transfer_all {
                            self.transfer_all = false;
                            self.transfer_amount.clear();
                        }

                        ui.add_space(16.0);

                        // ── Recipient Address + prefix-based network badge ──
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("To")
                                    .size(12.0)
                                    .color(self.colors.text_muted),
                            );
                            // Lightweight prefix check (avoids parsing every frame).
                            let trimmed = self.transfer_recipient.trim();
                            if !trimmed.is_empty() {
                                let (label, fill, color) = if trimmed.starts_with("ckb1q") {
                                    ("mainnet", self.colors.accent_tint, self.colors.accent)
                                } else if trimmed.starts_with("ckt1q") {
                                    ("testnet", self.colors.accent2_tint, self.colors.accent2)
                                } else {
                                    (
                                        "invalid prefix",
                                        egui::Color32::from_rgba_unmultiplied(255, 70, 70, 20),
                                        self.colors.danger,
                                    )
                                };
                                ui.add_space(8.0);
                                egui::Frame::new()
                                    .fill(fill)
                                    .corner_radius(10.0)
                                    .inner_margin(egui::Margin::symmetric(8, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(label)
                                                .size(8.5)
                                                .family(egui::FontFamily::Monospace)
                                                .color(color),
                                        );
                                    });
                            }
                        });
                        ui.add_space(4.0);

                        let recipient_edit =
                            egui::TextEdit::singleline(&mut self.transfer_recipient)
                                .hint_text("Input recipient address")
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
                                    if self.transfer_all && !is_busy
                                        && ui.small_button("✕").clicked()
                                    {
                                        self.transfer_all = false;
                                        self.transfer_amount.clear();
                                    }

                                    let can_calculate_max = !is_busy
                                        && !self.transfer_all
                                        && !self.accounts.is_empty();
                                    if ui
                                        .add_enabled(
                                            can_calculate_max,
                                            egui::Button::new("MAX").small(),
                                        )
                                        .clicked()
                                    {
                                        // Fill the displayed amount from the cached
                                        // spendable balance; send-all recomputes the
                                        // exact amount from fresh cells at build time.
                                        let idx = self
                                            .transfer_from_account
                                            .min(self.accounts.len() - 1);
                                        if let Some(sh) = self
                                            .spendable_balances
                                            .get(&self.accounts[idx])
                                            .copied()
                                            .flatten()
                                        {
                                            self.transfer_amount = format_ckb(sh);
                                        }
                                        self.transfer_all = true;
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

                        ui.add_space(12.0);

                        // ── Irreversibility warning (mirrors DAO Deposit's lock warning) ──
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_premultiplied(255, 170, 0, 15))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(
                                        "Transfers are final. Double-check the recipient address before sending.",
                                    )
                                    .size(11.0)
                                    .color(self.colors.bg),
                                );
                            });

                        ui.add_space(16.0);

                        // ── Send Button ──
                        let has_accounts = !self.accounts.is_empty();
                        let can_send = has_accounts
                            && !is_busy
                            && !self.transfer_recipient.is_empty()
                            && !self.transfer_amount.is_empty();

                        let btn_text = match &self.tx_status {
                            TransactionStatus::Building => "Building transaction...",
                            TransactionStatus::AwaitingSignature => "Waiting for Touch ID...",
                            TransactionStatus::Sending => "Broadcasting...",
                            _ => "Confirm Send",
                        };

                        let btn_fill = if can_send {
                            self.colors.accent
                        } else if is_busy {
                            self.colors.accent.linear_multiply(0.5)
                        } else {
                            self.colors.surface2
                        };
                        let send_btn =
                            egui::Button::new(egui::RichText::new(btn_text).size(15.0).strong().color(self.colors.bg))
                                .fill(btn_fill)
                                .min_size(egui::vec2(ui.available_width(), 44.0));

                        if ui.add_enabled(can_send, send_btn).clicked() {
                            self.transfer_async();
                        }

                    });


                // ── Address Book ──
                let mut seen: HashSet<String> = HashSet::new();
                let entries: Vec<String> = self
                    .tx_history
                    .iter()
                    .filter(|r| matches!(r.tx_kind, TxKind::Outgoing))
                    .filter_map(|r| r.external_recipient_address.as_ref())
                    .filter(|a| seen.insert((*a).clone()))
                    .take(5)
                    .cloned()
                    .collect();

                ui.add_space(20.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Address Book")
                            .size(15.0)
                            .strong()
                            .color(self.colors.text),
                    );

                    match &self.tx_status {
                        TransactionStatus::Success(tx_hash) => {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new("Sent:")
                                    .size(11.0)
                                    .color(self.colors.accent),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "0x{}..{}",
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
                        }
                        TransactionStatus::Error(msg) => {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new(msg)
                                    .size(11.0)
                                    .color(self.colors.danger),
                            );
                        }
                        _ => {}
                    }
                });
                ui.add_space(8.0);

                if entries.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "No recent recipients yet. Sent addresses will appear here.",
                        )
                        .size(11.0)
                        .color(self.colors.text_muted),
                    );
                } else {
                    let avatar_palette = [
                        self.colors.accent,
                        self.colors.accent2,
                        self.colors.accent3,
                        self.colors.warn,
                    ];
                    for (i, addr) in entries.iter().enumerate() {
                        let fill = avatar_palette[i % avatar_palette.len()];
                        let letter = {
                            let sum: u32 = addr.bytes().map(u32::from).sum();
                            ((b'A' + (sum % 26) as u8) as char).to_string()
                        };

                        ui.horizontal(|ui| {
                            egui::Frame::new()
                                .fill(fill)
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(letter)
                                            .size(12.0)
                                            .strong()
                                            .color(self.colors.bg),
                                    );
                                });
                            ui.add_space(10.0);
                            let short_addr = if addr.len() > 60 {
                                format!("{}...{}", &addr[..30], &addr[addr.len() - 30..])
                            } else {
                                addr.clone()
                            };
                            ui.label(
                                egui::RichText::new(short_addr)
                                    .size(11.0)
                                    .family(egui::FontFamily::Monospace)
                                    .color(self.colors.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let use_btn = egui::Button::new(
                                        egui::RichText::new("Use \u{2192}")
                                            .size(11.0)
                                            .color(self.colors.accent),
                                    )
                                    .fill(egui::Color32::TRANSPARENT);
                                    if ui.add_enabled(!is_busy, use_btn).clicked() {
                                        self.transfer_recipient = addr.clone();
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                    }
                }

                ui.add_space(20.0);
            }); // vertical
        }); // horizontal
    }
}
