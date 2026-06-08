//! DAO Operations tab rendering.

use ckb_types::prelude::Unpack;
use eframe::egui;

use super::utils::{extract_ar, format_duration_ms, paint_corner_accent, CardHover};
use crate::types::{DaoView, TransactionStatus};
use crate::utils::{format_ckb, format_ckb_balance, format_ckb_with_decimals};
use crate::App;

impl App {
    pub(crate) fn show_dao_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("Nervos DAO")
                        .size(26.0)
                        .strong()
                        .color(self.colors.text),
                );
                ui.label(
                    egui::RichText::new("Deposit, withdraw, and manage DAO positions")
                        .size(13.0)
                        .color(self.colors.text_muted),
                );

                ui.add_space(22.0);

                // ── Stats Row ──
                let total_deposited: u64 = self
                    .dao_deposited_cells
                    .iter()
                    .map(|(_, c)| c.capacity)
                    .sum();
                let total_prepared_principal: u64 = self
                    .dao_prepared_cells
                    .iter()
                    .map(|(_, c)| c.capacity)
                    .sum();
                let total_earned: u64 = self
                    .dao_prepared_cells
                    .iter()
                    .map(|(_, c)| c.maximum_withdraw.saturating_sub(c.capacity))
                    .sum();
                let active_cells = self.dao_deposited_cells.len() + self.dao_prepared_cells.len();
                let total_locked = total_deposited + total_prepared_principal;

                ui.columns(4, |cols| {
                    let stats = [
                        (
                            format_ckb(total_locked),
                            "CKB Locked",
                            Some(self.colors.accent),
                        ),
                        (
                            format!("+{}", format_ckb(total_earned)),
                            "CKB Earned",
                            Some(self.colors.warn),
                        ),
                        (
                            self.compute_dao_apc(),
                            "Current APC",
                            Some(self.colors.accent2),
                        ),
                        (
                            active_cells.to_string(),
                            "Active Deposits",
                            Some(self.colors.accent3),
                        ),
                    ];

                    for (i, (value, label, color)) in stats.iter().enumerate() {
                        egui::Frame::new()
                            .fill(self.colors.surface)
                            .corner_radius(12.0)
                            .inner_margin(egui::Margin::symmetric(16, 14))
                            .stroke(egui::Stroke::new(1.0, self.colors.border))
                            .show(&mut cols[i], |ui| {
                                let text = egui::RichText::new(value).size(18.0).strong();
                                let text = if let Some(c) = color {
                                    text.color(*c)
                                } else {
                                    text
                                };
                                ui.label(text);
                                ui.label(
                                    egui::RichText::new(*label)
                                        .size(11.0)
                                        .color(self.colors.text_muted),
                                );
                            });
                    }
                });

                ui.add_space(22.0);

                // ── Action Cards ──
                let is_busy = !matches!(self.tx_status, TransactionStatus::Idle)
                    && !matches!(self.tx_status, TransactionStatus::Success(_))
                    && !matches!(self.tx_status, TransactionStatus::Error(_));

                // 3-column action cards
                ui.columns(3, |cols| {
                    // Deposit card
                    let hover = CardHover::new(&cols[0], "dao-deposit", &self.colors);

                    let deposit_resp = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(18.0)
                        .inner_margin(egui::Margin::symmetric(20, 22))
                        .stroke(hover.stroke)
                        .show(&mut cols[0], |ui| {
                            hover.apply_lift(ui);
                            ui.label(egui::RichText::new("\u{1f4e5}").size(26.0));
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("DAO Deposit")
                                    .size(14.0)
                                    .strong()
                                    .color(self.colors.text),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Lock CKB to earn compensation against secondary issuance inflation.",
                                )
                                .size(11.0)
                                .color(self.colors.text_muted),
                            );
                        });

                    paint_corner_accent(
                        cols[0].painter(),
                        deposit_resp.response.rect,
                        18.0,
                        self.colors.accent,
                    );
                    hover.commit(&deposit_resp.response);
                    if deposit_resp
                        .response
                        .interact(egui::Sense::click())
                        .clicked()
                        && !is_busy
                    {
                        self.dao_view = DaoView::Deposit;
                        self.tx_status = TransactionStatus::Idle;
                    }

                    // Request Withdrawal card
                    let hover = CardHover::new(&cols[1], "dao-request", &self.colors);

                    let request_resp = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(18.0)
                        .inner_margin(egui::Margin::symmetric(20, 22))
                        .stroke(hover.stroke)
                        .show(&mut cols[1], |ui| {
                            hover.apply_lift(ui);
                            ui.label(egui::RichText::new("\u{23f3}").size(26.0));
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Request Withdrawal")
                                    .size(14.0)
                                    .strong()
                                    .color(self.colors.text),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Begin the unlock process. Wait for an epoch boundary to complete.",
                                )
                                .size(11.0)
                                .color(self.colors.text_muted),
                            );
                        });

                    paint_corner_accent(
                        cols[1].painter(),
                        request_resp.response.rect,
                        18.0,
                        self.colors.warn,
                    );
                    hover.commit(&request_resp.response);

                    // Withdraw card
                    let hover = CardHover::new(&cols[2], "dao-withdraw", &self.colors);

                    let withdraw_resp = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(18.0)
                        .inner_margin(egui::Margin::symmetric(20, 22))
                        .stroke(hover.stroke)
                        .show(&mut cols[2], |ui| {
                            hover.apply_lift(ui);
                            ui.label(egui::RichText::new("\u{1f4e4}").size(26.0));
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Withdraw")
                                    .size(14.0)
                                    .strong()
                                    .color(self.colors.text),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Claim CKB + compensation after the epoch boundary is reached.",
                                )
                                .size(11.0)
                                .color(self.colors.text_muted),
                            );
                        });

                    paint_corner_accent(
                        cols[2].painter(),
                        withdraw_resp.response.rect,
                        18.0,
                        self.colors.accent2,
                    );
                    hover.commit(&withdraw_resp.response);
                });

                ui.add_space(22.0);

                // ── Active Deposits Table ──
                self.show_dao_positions_table(ui);

                ui.add_space(20.0);
            }); // vertical
        }); // horizontal
    }

    /// Renders the DAO deposit form as a centered modal overlay.
    pub(crate) fn show_dao_deposit_modal(&mut self, ctx: &egui::Context) {
        if self.dao_view != DaoView::Deposit {
            return;
        }

        let is_busy = !matches!(
            self.tx_status,
            TransactionStatus::Idle | TransactionStatus::Success(_) | TransactionStatus::Error(_)
        );

        // Semi-transparent backdrop that consumes clicks.
        let screen_rect = ctx.input(|i| i.viewport_rect());
        let backdrop_clicked = egui::Area::new(egui::Id::new("dao_deposit_backdrop"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
                );
                response.clicked()
            })
            .inner;

        let modal_width = 480.0;
        let modal_pos = egui::pos2(
            (screen_rect.width() - modal_width) / 2.0,
            screen_rect.height() * 0.12,
        );

        egui::Area::new(egui::Id::new("dao_deposit_area"))
            .fixed_pos(modal_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border2))
                    .corner_radius(18.0)
                    .inner_margin(egui::Margin::symmetric(28, 24))
                    .show(ui, |ui| {
                        ui.set_width(modal_width);

                        ui.label(
                            egui::RichText::new("DAO Deposit")
                                .size(20.0)
                                .strong()
                                .color(self.colors.text),
                        );
                        ui.label(
                            egui::RichText::new("Lock CKB to earn compensation against inflation")
                                .size(12.0)
                                .color(self.colors.text_muted),
                        );

                        ui.add_space(20.0);

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
                            let idx =
                                self.dao_deposit_from_account.min(self.accounts.len() - 1);
                            let lock_args = &self.accounts[idx].lock_args;
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

                        let prev_from_account = self.dao_deposit_from_account;
                        egui::ComboBox::from_id_salt("dao_deposit_from")
                            .selected_text(&from_text)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for (i, account) in self.accounts.iter().enumerate() {
                                    let bal = self
                                        .spendable_balances
                                        .get(&account.lock_args)
                                        .and_then(|b| b.as_ref())
                                        .copied();
                                    let label = match bal {
                                        Some(b) => {
                                            format!("Account #{} ({})", i, format_ckb_balance(b))
                                        }
                                        None => format!("Account #{}", i),
                                    };
                                    ui.selectable_value(
                                        &mut self.dao_deposit_from_account,
                                        i,
                                        label,
                                    );
                                }
                            });
                        if self.dao_deposit_from_account != prev_from_account
                            && self.dao_deposit_all
                        {
                            self.dao_deposit_all = false;
                            self.dao_deposit_amount.clear();
                        }

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
                                    if self.dao_deposit_all
                                        && !is_busy
                                        && ui.small_button("\u{2715}").clicked()
                                    {
                                        self.dao_deposit_all = false;
                                        self.dao_deposit_amount.clear();
                                    }

                                    let can_max = !is_busy
                                        && !self.dao_deposit_all
                                        && !self.accounts.is_empty();
                                    if ui
                                        .add_enabled(can_max, egui::Button::new("MAX").small())
                                        .clicked()
                                    {
                                        let idx = self
                                            .dao_deposit_from_account
                                            .min(self.accounts.len() - 1);
                                        if let Some(sh) = self
                                            .spendable_balances
                                            .get(&self.accounts[idx].lock_args)
                                            .copied()
                                            .flatten()
                                        {
                                            self.dao_deposit_amount = format_ckb(sh);
                                        }
                                        self.dao_deposit_all = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);

                        let amount_interactive = !is_busy && !self.dao_deposit_all;
                        let amount_edit =
                            egui::TextEdit::singleline(&mut self.dao_deposit_amount)
                                .hint_text("Min: 114 CKB")
                                .desired_width(ui.available_width())
                                .font(egui::FontId::monospace(13.0))
                                .interactive(amount_interactive);
                        ui.add(amount_edit);

                        if self.dao_deposit_all {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Fee will be deducted at deposit time.")
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
                            let fee_edit =
                                egui::TextEdit::singleline(&mut self.dao_deposit_fee_rate)
                                    .hint_text("1000")
                                    .desired_width(120.0)
                                    .font(egui::FontId::monospace(12.0))
                                    .interactive(!is_busy);
                            ui.add(fee_edit);
                        });

                        ui.add_space(12.0);

                        // ── Warning ──
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_premultiplied(255, 170, 0, 15))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(
                                        "Deposited CKB is locked until you request withdrawal and wait for an epoch boundary (~4 hours/epoch).",
                                    )
                                    .size(11.0)
                                    .color(self.colors.bg),
                                );
                            });

                        ui.add_space(20.0);

                        // ── Confirm button ──
                        let can_deposit = !is_busy
                            && !self.accounts.is_empty()
                            && !self.dao_deposit_amount.is_empty();

                        let btn_text = match &self.tx_status {
                            TransactionStatus::Building => "Building transaction...",
                            TransactionStatus::AwaitingSignature => "Waiting for Touch ID...",
                            TransactionStatus::Sending => "Broadcasting...",
                            _ => "Confirm Deposit",
                        };

                        let btn_fill = if can_deposit {
                            self.colors.accent
                        } else if is_busy {
                            self.colors.accent.linear_multiply(0.5)
                        } else {
                            self.colors.surface2
                        };
                        let deposit_btn = egui::Button::new(
                            egui::RichText::new(btn_text)
                                .size(15.0)
                                .strong()
                                .color(self.colors.bg),
                        )
                        .fill(btn_fill)
                        .min_size(egui::vec2(ui.available_width(), 44.0));

                        if ui.add_enabled(can_deposit, deposit_btn).clicked() {
                            self.dao_deposit_async();
                        }

                        // ── Status messages ──
                        match &self.tx_status {
                            TransactionStatus::Success(tx_hash) => {
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
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
                                });
                            }
                            TransactionStatus::Error(e) => {
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new(e)
                                        .size(12.0)
                                        .color(self.colors.danger),
                                );
                            }
                            _ => {}
                        }

                        ui.add_space(14.0);

                        // ── Cancel button ──
                        let avail = ui.available_width();
                        let cancel_clicked = ui
                            .vertical_centered(|ui| {
                                let cancel = egui::Button::new(
                                    egui::RichText::new("Cancel")
                                        .size(13.0)
                                        .color(self.colors.text_muted),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, self.colors.border2))
                                .corner_radius(8.0)
                                .min_size(egui::vec2(avail, 36.0));
                                ui.add(cancel).clicked()
                            })
                            .inner;

                        if cancel_clicked && !is_busy {
                            self.dao_view = DaoView::Overview;
                        }
                    });

            });

        if backdrop_clicked && !is_busy {
            self.dao_view = DaoView::Overview;
        }
    }

    /// Renders the Active Deposits table.
    pub(crate) fn show_dao_positions_table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Active Deposits")
                    .size(16.0)
                    .strong()
                    .color(self.colors.text_muted),
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
        ui.add_space(12.0);

        let is_busy = !matches!(self.tx_status, TransactionStatus::Idle)
            && !matches!(self.tx_status, TransactionStatus::Success(_))
            && !matches!(self.tx_status, TransactionStatus::Error(_));

        if self.dao_deposited_cells.is_empty() && self.dao_prepared_cells.is_empty() {
            if self.dao_cells_query_rx.is_some() {
                ui.label(
                    egui::RichText::new("Loading DAO cells...")
                        .size(12.0)
                        .color(self.colors.text_muted),
                );
            } else {
                ui.label(
                    egui::RichText::new("No active DAO positions.")
                        .size(12.0)
                        .color(self.colors.text_muted),
                );
            }

            return;
        }

        // Collect actions to perform after the table is rendered (avoid borrow conflicts).
        let mut prepare_action: Option<(ckb_types::packed::OutPoint, String)> = None;
        let mut withdraw_action: Option<(ckb_types::packed::OutPoint, String)> = None;

        // Helper: find the account index for a given lock_args.
        let account_index =
            |lock_args: &str, accounts: &[qpv2_core::types::SphincsPlusAccount]| -> usize {
                accounts
                    .iter()
                    .position(|a| a.lock_args == lock_args)
                    .unwrap_or(0)
            };

        let wide = ui.available_width() > 1100.0;
        let table_avail = ui.available_width();
        let frame_overhead = 26.0;
        let inner_avail = table_avail - frame_overhead;
        let data_width = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(egui::Id::new("dao_data_cols_width")))
            .unwrap_or(if wide { 950.0 } else { 750.0 });
        let filler_step = 72.0;
        let num_fillers = ((inner_avail - data_width) / filler_step).floor().max(0.0) as usize;

        let table_fill = egui::Color32::from_rgba_unmultiplied(
            self.colors.accent.r(),
            self.colors.accent.g(),
            self.colors.accent.b(),
            10,
        );
        egui::Frame::new()
            .fill(table_fill)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.0, self.colors.border))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let num_cols = (if wide { 8 } else { 7 }) + num_fillers;
                let grid_resp = egui::Grid::new("dao_positions")
                    .num_columns(num_cols)
                    .spacing(egui::vec2(12.0, 8.0))
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        let header = |ui: &mut egui::Ui, text: &str| {
                            ui.label(
                                egui::RichText::new(text)
                                    .size(10.5)
                                    .strong()
                                    .color(self.colors.text_muted),
                            );
                        };
                        header(ui, "Account");
                        if wide {
                            header(ui, "Tx Hash");
                            header(ui, "Index");
                        } else {
                            header(ui, "Cell");
                        }
                        header(ui, "Amount");
                        header(ui, "Earned");
                        header(ui, if wide { "Lock Duration" } else { "Duration" });
                        header(ui, "Status");
                        header(ui, "Action");
                        for _ in 0..num_fillers {
                            ui.add_sized(
                                [60.0, 14.0],
                                egui::Label::new(
                                    egui::RichText::new("--")
                                        .size(10.5)
                                        .strong()
                                        .color(self.colors.text_muted),
                                ),
                            );
                        }
                        ui.end_row();

                        // Deposited cells
                        for (lock_args, cell) in &self.dao_deposited_cells {
                            let acct_idx = account_index(lock_args, &self.accounts);
                            ui.label(
                                egui::RichText::new(format!("#{}", acct_idx))
                                    .size(10.5)
                                    .color(self.colors.accent2)
                                    .family(egui::FontFamily::Monospace),
                            );

                            let idx: u32 = cell.out_point.index().unpack();
                            if wide {
                                let tx_hash = format!("{:#x}", cell.out_point.tx_hash());
                                ui.label(
                                    egui::RichText::new(&tx_hash)
                                        .size(10.5)
                                        .color(self.colors.text_muted)
                                        .font(egui::FontId::monospace(10.0)),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{}", idx))
                                        .size(10.5)
                                        .color(self.colors.text_muted)
                                        .font(egui::FontId::monospace(10.0)),
                                );
                            } else {
                                let cell_id = format!("{:#x}/{}", cell.out_point.tx_hash(), idx);
                                ui.label(
                                    egui::RichText::new(&cell_id)
                                        .size(10.5)
                                        .color(self.colors.text_muted)
                                        .font(egui::FontId::monospace(10.0)),
                                );
                            }

                            ui.label(
                                egui::RichText::new(format!(
                                    "{} CKB",
                                    if wide {
                                        format_ckb_with_decimals(cell.capacity, 8)
                                    } else {
                                        format_ckb(cell.capacity)
                                    }
                                ))
                                .size(12.0)
                                .color(self.colors.text_muted)
                                .strong(),
                            );

                            // Estimated earned from cached deposit header + tip.
                            let estimated = self
                                .deposit_headers
                                .get(&cell.block_number)
                                .zip(self.node_status.tip_header.as_ref())
                                .map(|(dep_h, tip_h)| {
                                    let ar_dep = extract_ar(dep_h);
                                    let ar_tip = extract_ar(tip_h);
                                    let growth = ar_tip / ar_dep;
                                    ((cell.capacity as f64 * (growth - 1.0)) as u64, true)
                                });

                            match estimated {
                                Some((earned, _)) => {
                                    let earned_str = if wide {
                                        format_ckb_with_decimals(earned, 8)
                                    } else {
                                        format_ckb(earned)
                                    };
                                    ui.label(
                                        egui::RichText::new(format!("~+{} CKB", earned_str))
                                            .size(11.0)
                                            .color(self.colors.warn),
                                    );
                                }
                                None => {
                                    ui.label(
                                        egui::RichText::new("--")
                                            .size(11.0)
                                            .color(self.colors.text_muted),
                                    );
                                }
                            }

                            // Lock duration from deposit header timestamp to tip.
                            let duration_str = self
                                .deposit_headers
                                .get(&cell.block_number)
                                .zip(self.node_status.tip_header.as_ref())
                                .map(|(dep_h, tip_h)| {
                                    let ms = tip_h.timestamp().saturating_sub(dep_h.timestamp());
                                    format_duration_ms(ms, wide)
                                })
                                .unwrap_or_else(|| "--".to_string());
                            ui.label(
                                egui::RichText::new(duration_str)
                                    .size(10.5)
                                    .color(self.colors.text_muted),
                            );

                            ui.label(
                                egui::RichText::new("Earning")
                                    .size(10.5)
                                    .color(self.colors.accent),
                            );

                            if ui
                                .add_enabled(
                                    !is_busy,
                                    egui::Button::new(
                                        egui::RichText::new("Request")
                                            .size(10.5)
                                            .color(self.colors.text_muted),
                                    )
                                    .fill(self.colors.surface2),
                                )
                                .clicked()
                            {
                                prepare_action = Some((cell.out_point.clone(), lock_args.clone()));
                            }

                            for _ in 0..num_fillers {
                                ui.add_sized(
                                    [60.0, 14.0],
                                    egui::Label::new(
                                        egui::RichText::new("--")
                                            .size(10.5)
                                            .color(self.colors.text_muted),
                                    ),
                                );
                            }
                            ui.end_row();
                        }

                        // Prepared cells
                        for (lock_args, cell) in &self.dao_prepared_cells {
                            let acct_idx = account_index(lock_args, &self.accounts);
                            ui.label(
                                egui::RichText::new(format!("#{}", acct_idx))
                                    .size(10.5)
                                    .color(self.colors.accent2)
                                    .family(egui::FontFamily::Monospace),
                            );

                            let idx: u32 = cell.out_point.index().unpack();
                            if wide {
                                let tx_hash = format!("{:#x}", cell.out_point.tx_hash());
                                ui.label(
                                    egui::RichText::new(&tx_hash)
                                        .size(10.5)
                                        .color(self.colors.text_muted)
                                        .font(egui::FontId::monospace(10.0)),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{}", idx))
                                        .size(10.5)
                                        .color(self.colors.text_muted)
                                        .font(egui::FontId::monospace(10.0)),
                                );
                            } else {
                                let cell_id = format!("{:#x}/{}", cell.out_point.tx_hash(), idx);
                                ui.label(
                                    egui::RichText::new(&cell_id)
                                        .size(10.5)
                                        .color(self.colors.text_muted)
                                        .font(egui::FontId::monospace(10.0)),
                                );
                            }

                            ui.label(
                                egui::RichText::new(format!("{} CKB", format_ckb(cell.capacity)))
                                    .size(12.0)
                                    .color(self.colors.text_muted)
                                    .strong(),
                            );

                            let earned = cell.maximum_withdraw.saturating_sub(cell.capacity);
                            ui.label(
                                egui::RichText::new(format!(
                                    "+{} CKB",
                                    if wide {
                                        format_ckb_with_decimals(earned, 8)
                                    } else {
                                        format_ckb(earned)
                                    }
                                ))
                                .size(11.0)
                                .strong()
                                .color(self.colors.warn),
                            );

                            // Lock duration from deposit to prepare.
                            let ms = cell
                                .prepare_header
                                .timestamp()
                                .saturating_sub(cell.deposit_header.timestamp());
                            ui.label(
                                egui::RichText::new(format_duration_ms(ms, wide))
                                    .size(10.5)
                                    .color(self.colors.text_muted),
                            );

                            ui.label(
                                egui::RichText::new("Redeemed")
                                    .size(10.5)
                                    .color(self.colors.warn),
                            );

                            if ui
                                .add_enabled(
                                    !is_busy,
                                    egui::Button::new(
                                        egui::RichText::new("Withdraw")
                                            .size(10.5)
                                            .color(self.colors.accent2),
                                    )
                                    .fill(self.colors.surface2),
                                )
                                .clicked()
                            {
                                withdraw_action = Some((cell.out_point.clone(), lock_args.clone()));
                            }

                            for _ in 0..num_fillers {
                                ui.add_sized(
                                    [60.0, 14.0],
                                    egui::Label::new(
                                        egui::RichText::new("--")
                                            .size(10.5)
                                            .color(self.colors.text_muted),
                                    ),
                                );
                            }
                            ui.end_row();
                        }
                    });
                let grid_w = grid_resp.response.rect.width();
                let data_w = grid_w - num_fillers as f32 * filler_step;
                if data_w > 100.0 {
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(egui::Id::new("dao_data_cols_width"), data_w);
                    });
                }
            });

        // Handle deferred actions
        if let Some((out_point, lock_args)) = prepare_action {
            self.dao_withdraw_request_async(out_point, lock_args);
        }
        if let Some((out_point, lock_args)) = withdraw_action {
            self.dao_withdraw_async(out_point, lock_args);
        }
    }
}
