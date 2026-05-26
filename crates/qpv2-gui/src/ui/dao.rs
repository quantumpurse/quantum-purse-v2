//! DAO Operations tab rendering.

use ckb_types::prelude::Unpack;
use eframe::egui;

use super::common::{paint_corner_accent, CardHover};
use crate::types::{
    format_ckb, format_ckb_balance, DaoView, SpendableCapacityTarget, TransactionStatus,
};
use crate::App;

impl App {
    pub(crate) fn show_dao_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 30.0);

                ui.heading(
                    egui::RichText::new("NervosDAO")
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
                            "~2-3%".to_string(),
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

                match self.dao_view {
                    DaoView::Overview => {
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
                    }

                    DaoView::Deposit => {
                        self.show_dao_deposit_form(ui, is_busy);
                    }
                }
            }); // vertical
        }); // horizontal
    }

    /// Renders the deposit form sub-view within the DAO tab.
    pub(crate) fn show_dao_deposit_form(&mut self, ui: &mut egui::Ui, is_busy: bool) {
        // Back button
        if ui.small_button("< Back to overview").clicked() && !is_busy {
            self.dao_view = DaoView::Overview;
        }

        ui.add_space(12.0);

        // Aurora panel: same composition as the dashboard hero and
        // Transfer card. Reserve indices for gradient + spotlight +
        // corner bloom under the form widgets, fill them after
        // `Frame::show` returns when the card's rect is known.
        let mut gradient_idx = None;
        let mut spotlight_idx = None;
        let mut glow_idx = None;
        let frame_response = egui::Frame::new()
            .fill(self.colors.surface)
            .corner_radius(18.0)
            .inner_margin(egui::Margin::symmetric(28, 26))
            .stroke(egui::Stroke::new(1.0, self.colors.border))
            .show(ui, |ui| {
                gradient_idx = Some(ui.painter().add(egui::Shape::Noop));
                spotlight_idx = Some(ui.painter().add(egui::Shape::Noop));
                glow_idx = Some(ui.painter().add(egui::Shape::Noop));

                ui.label(egui::RichText::new("DAO Deposit").size(20.0).strong().color(self.colors.text));
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
                    let idx = self.dao_deposit_from_account.min(self.accounts.len() - 1);
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

                let prev_from_account = self.dao_deposit_from_account;
                egui::ComboBox::from_id_salt("dao_deposit_from")
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
                                Some(b) => format!("Account #{} ({})", i, format_ckb_balance(b)),
                                None => format!("Account #{}", i),
                            };
                            ui.selectable_value(
                                &mut self.dao_deposit_from_account,
                                i,
                                label,
                            );
                        }
                    });
                // Clear deposit_all if the user switches accounts.
                if self.dao_deposit_from_account != prev_from_account && self.dao_deposit_all {
                    self.dao_deposit_all = false;
                    self.dao_deposit_amount.clear();
                }

                ui.add_space(16.0);

                let is_calculating_max = matches!(
                    self.spendable_capacity_rx,
                    Some((SpendableCapacityTarget::DaoDeposit, _))
                );

                // Amount input
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Amount (CKB)")
                            .size(12.0)
                            .color(self.colors.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Clear button when deposit_all is active.
                        if self.dao_deposit_all && !is_busy
                            && ui.small_button("✕").clicked()
                        {
                            self.dao_deposit_all = false;
                            self.dao_deposit_amount.clear();
                        }

                        let can_calculate_max = !is_busy
                            && !is_calculating_max
                            && !self.dao_deposit_all
                            && !self.accounts.is_empty();
                        let max_label = if is_calculating_max { "..." } else { "MAX" };
                        if ui
                            .add_enabled(
                                can_calculate_max,
                                egui::Button::new(max_label).small(),
                            )
                            .clicked()
                        {
                            self.fetch_spendable_capacity(SpendableCapacityTarget::DaoDeposit);
                        }
                    });
                });
                ui.add_space(4.0);

                let amount_interactive = !is_busy && !self.dao_deposit_all;
                let amount_edit = egui::TextEdit::singleline(&mut self.dao_deposit_amount)
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
                } else if is_calculating_max {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Fetching spendable balance...")
                            .size(11.0)
                            .color(self.colors.text_muted),
                    );
                }

                ui.add_space(16.0);

                // Fee Rate (collapsible)
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
                    let fee_edit = egui::TextEdit::singleline(&mut self.dao_deposit_fee_rate)
                        .hint_text("1000")
                        .desired_width(120.0)
                        .font(egui::FontId::monospace(12.0))
                        .interactive(!is_busy);
                    ui.add(fee_edit);
                });

                ui.add_space(12.0);

                // Warning
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

                // Action buttons
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
                let deposit_btn =
                    egui::Button::new(egui::RichText::new(btn_text).size(15.0).strong().color(self.colors.bg))
                        .fill(btn_fill)
                        .min_size(egui::vec2(ui.available_width(), 44.0));

                if ui.add_enabled(can_deposit, deposit_btn).clicked() {
                    self.dao_deposit_async();
                }

                // Status messages
                match &self.tx_status {
                    TransactionStatus::Success(hash) => {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Deposit broadcast: ")
                                    .size(12.0)
                                    .color(self.colors.accent),
                            );
                            ui.label(
                                egui::RichText::new(hash)
                                    .size(11.0)
                                    .color(self.colors.text_muted)
                                    .font(egui::FontId::monospace(11.0)),
                            );
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
            });

        // Aurora layers, painted under the form via the indices we
        // reserved above. No `outer_margin` on this Frame, so
        // `response.rect` is the actual card outline.
        let card_rect = frame_response.response.rect;
        let painter = ui.painter_at(card_rect);

        if let Some(idx) = gradient_idx {
            let tl = egui::Color32::from_rgba_unmultiplied(0, 255, 180, 18);
            let tr = egui::Color32::from_rgba_unmultiplied(0, 200, 255, 10);
            let brc = egui::Color32::from_rgba_unmultiplied(123, 94, 167, 13);
            let bl = egui::Color32::from_rgba_unmultiplied(0, 200, 255, 10);
            let mesh =
                crate::ui::common::rounded_rect_gradient_mesh(card_rect, 18.0, tl, tr, brc, bl);
            painter.set(idx, egui::Shape::mesh(mesh));
        }

        if let Some(idx) = spotlight_idx {
            let spot_center = egui::pos2(card_rect.left() + 120.0, card_rect.top() + 80.0);
            let mut mesh =
                crate::ui::common::smooth_glow_mesh(spot_center, 170.0, self.colors.accent, 26);
            crate::ui::common::clamp_mesh_to_rounded_rect(&mut mesh, card_rect, 18.0);
            painter.set(idx, egui::Shape::mesh(mesh));
        }

        if let Some(idx) = glow_idx {
            let glow_center = egui::pos2(card_rect.right() - 60.0, card_rect.top() + 60.0);
            let mut mesh =
                crate::ui::common::smooth_glow_mesh(glow_center, 100.0, self.colors.accent, 20);
            crate::ui::common::clamp_mesh_to_rounded_rect(&mut mesh, card_rect, 18.0);
            painter.set(idx, egui::Shape::mesh(mesh));
        }
    }

    /// Renders the Active Deposits table.
    pub(crate) fn show_dao_positions_table(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Active Deposits")
                .size(16.0)
                .strong()
                .color(self.colors.text_muted),
        );
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
        let account_index = |lock_args: &str, accounts: &[String]| -> usize {
            accounts.iter().position(|a| a == lock_args).unwrap_or(0)
        };

        egui::Frame::new()
            .fill(self.colors.surface)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.0, self.colors.border))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("dao_positions")
                    .num_columns(7)
                    .min_col_width(60.0)
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
                        header(ui, "Cell");
                        header(ui, "Deposited");
                        header(ui, "Since Block");
                        header(ui, "Earned");
                        header(ui, "Status");
                        header(ui, "Action");
                        ui.end_row();

                        // Deposited cells
                        for (lock_args, cell) in &self.dao_deposited_cells {
                            let acct_idx = account_index(lock_args, &self.accounts);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("#{}", acct_idx))
                                        .size(10.5)
                                        .color(self.colors.accent2)
                                        .family(egui::FontFamily::Monospace),
                                );
                            });

                            let idx: u32 = cell.out_point.index().unpack();
                            let cell_id = format!("{:#x}/{}", cell.out_point.tx_hash(), idx);
                            ui.label(
                                egui::RichText::new(&cell_id)
                                    .size(10.0)
                                    .color(self.colors.text_muted)
                                    .font(egui::FontId::monospace(10.0)),
                            );

                            ui.label(
                                egui::RichText::new(format!("{} CKB", format_ckb(cell.capacity)))
                                    .size(12.0)
                                    .color(self.colors.text_muted)
                                    .strong(),
                            );

                            ui.label(
                                egui::RichText::new(format!("{}", cell.block_number))
                                    .size(10.5)
                                    .font(egui::FontId::monospace(10.5)),
                            );

                            ui.label(
                                egui::RichText::new("--")
                                    .size(11.0)
                                    .color(self.colors.text_muted),
                            );

                            ui.label(
                                egui::RichText::new("Active")
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

                            ui.end_row();
                        }

                        // Prepared cells
                        for (lock_args, cell) in &self.dao_prepared_cells {
                            let acct_idx = account_index(lock_args, &self.accounts);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("#{}", acct_idx))
                                        .size(10.5)
                                        .color(self.colors.accent2)
                                        .family(egui::FontFamily::Monospace),
                                );
                            });

                            let idx: u32 = cell.out_point.index().unpack();
                            let cell_id = format!("{:#x}/{}", cell.out_point.tx_hash(), idx);
                            ui.label(
                                egui::RichText::new(&cell_id)
                                    .size(10.0)
                                    .color(self.colors.text_muted)
                                    .font(egui::FontId::monospace(10.0)),
                            );

                            ui.label(
                                egui::RichText::new(format!("{} CKB", format_ckb(cell.capacity)))
                                    .size(12.0)
                                    .color(self.colors.text_muted)
                                    .strong(),
                            );

                            ui.label(
                                egui::RichText::new(format!("{}", cell.deposit_block_number))
                                    .size(10.5)
                                    .font(egui::FontId::monospace(10.5)),
                            );

                            let earned = cell.maximum_withdraw.saturating_sub(cell.capacity);
                            ui.label(
                                egui::RichText::new(format!("+{} CKB", format_ckb(earned)))
                                    .size(11.0)
                                    .strong()
                                    .color(self.colors.warn),
                            );

                            ui.label(
                                egui::RichText::new("Pending")
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

                            ui.end_row();
                        }
                    });
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
