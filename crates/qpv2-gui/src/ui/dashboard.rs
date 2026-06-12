//! Dashboard tab: the flagship telemetry grid — balance readout,
//! quick actions, and the transaction tape.

use eframe::egui;

use super::utils::{
    breathing_dot, ckb_split, ghost_button, lerp_color, panel_frame, row_hover, section_header,
    value_flash,
};
use crate::types::{display_font, label_font, AppColors, Status, Tab, TxKind, TxRecord};
use crate::utils::format_relative_time;
use crate::App;

/// Horizontal padding for the whole screen.
const PAD: f32 = 24.0;

/// Meta value below the total: integer part + 2 decimals keeps the
/// strip readable; the hero number above carries full precision.
fn meta_ckb(shannons: u64) -> String {
    let (int, frac) = ckb_split(shannons);
    format!("{}.{}", int, &frac[..2])
}

impl App {
    pub(crate) fn show_dashboard_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(PAD);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - PAD);

                // ── Screen header ──
                ui.label(
                    egui::RichText::new("DASHBOARD")
                        .font(display_font(16.0))
                        .color(self.colors.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Portfolio overview and activity.")
                        .size(11.0)
                        .color(self.colors.text_muted),
                );
                ui.add_space(14.0);

                self.draw_balance_panel(ui);
                ui.add_space(12.0);
                self.draw_quick_actions(ui);
                ui.add_space(16.0);
                self.draw_transaction_tape(ui);

                ui.add_space(12.0);
                self.show_status(ui);
                ui.add_space(20.0);
            });
        });
    }

    /// The hero readout: full-precision total, then a five-column meta
    /// strip under a hairline rule.
    fn draw_balance_panel(&mut self, ui: &mut egui::Ui) {
        // Sum all balances + DAO earned interest.
        let base_balance: u64 = self
            .balances
            .values()
            .filter_map(|b| b.as_ref().copied())
            .sum();
        let dao_interest: u64 = self
            .dao_prepared_cells
            .iter()
            .map(|(_, c)| c.maximum_withdraw.saturating_sub(c.capacity))
            .sum();
        let total_shannons = base_balance + dao_interest;

        // DAO Locked — sum of deposited + prepared cell capacities
        // across all accounts.
        let dao_locked: u64 = self
            .dao_deposited_cells
            .iter()
            .map(|(_, c)| c.capacity)
            .chain(self.dao_prepared_cells.iter().map(|(_, c)| c.capacity))
            .sum();
        // Spendable cells only — no type script, no data.
        let available: u64 = self
            .spendable_balances
            .values()
            .filter_map(|b| b.as_ref().copied())
            .sum();

        panel_frame(&self.colors).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_header(ui, &self.colors, "01", "Total Balance");
            ui.add_space(12.0);

            // Display numerals, integer part flashing accent on change.
            let flash = value_flash(ui, egui::Id::new("dash-total-flash"), total_shannons);
            let int_color = lerp_color(self.colors.text, self.colors.accent, flash);
            let (int, frac) = ckb_split(total_shannons);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(
                    egui::RichText::new(int)
                        .font(display_font(34.0))
                        .color(int_color),
                );
                ui.label(
                    egui::RichText::new(format!(".{}", frac))
                        .font(display_font(20.0))
                        .color(self.colors.text_muted),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("CKB")
                        .font(label_font(10.0))
                        .color(self.colors.accent),
                );
            });

            ui.add_space(12.0);
            let (rule, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().hline(
                rule.x_range(),
                rule.center().y,
                egui::Stroke::new(1.0, self.colors.border),
            );
            ui.add_space(10.0);

            // Semantic colors: spendable in the accent, locked capital
            // in caution yellow, yield in green; the plain count stays
            // neutral.
            let metas = [
                ("AVAILABLE", meta_ckb(available), self.colors.accent),
                (
                    "ACCOUNTS",
                    format!("{}", self.accounts.len()),
                    self.colors.text,
                ),
                ("DAO LOCKED", meta_ckb(dao_locked), self.colors.warn),
                (
                    "DAO EARNED",
                    format!("+{}", meta_ckb(dao_interest)),
                    self.colors.accent2,
                ),
                ("APC", self.compute_dao_apc(), self.colors.accent2),
            ];
            let gap = 25.0;
            let col_w = (ui.available_width() - 4.0 * gap) / 5.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, (label, value, color)) in metas.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(12.0);
                        let (div, _) =
                            ui.allocate_exact_size(egui::vec2(1.0, 30.0), egui::Sense::hover());
                        ui.painter().vline(
                            div.center().x,
                            div.y_range(),
                            egui::Stroke::new(1.0, self.colors.border),
                        );
                        ui.add_space(12.0);
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(col_w, 30.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.label(
                                egui::RichText::new(*label)
                                    .font(label_font(9.0))
                                    .color(self.colors.text_muted),
                            );
                            ui.add_space(3.0);
                            ui.label(egui::RichText::new(value).size(13.0).color(*color));
                        },
                    );
                }
            });
        });
    }

    /// One row of equal-width module shortcuts.
    fn draw_quick_actions(&mut self, ui: &mut egui::Ui) {
        let actions = [
            ("SEND", Tab::Transfer),
            ("RECEIVE", Tab::Accounts),
            ("DAO", Tab::DaoOperations),
            ("NODES", Tab::NodeManager),
            ("WALLETS", Tab::Wallets),
        ];
        let gap = 8.0;
        let btn_w = (ui.available_width() - 4.0 * gap) / 5.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (label, target_tab) in actions {
                let btn = ghost_button(&self.colors, label, egui::vec2(btn_w, 32.0));
                if ui.add(btn).clicked() {
                    self.active_tab = target_tab;
                }
            }
        });
    }

    /// The transaction tape: a hairline table of resolved history.
    fn draw_transaction_tape(&mut self, ui: &mut egui::Ui) {
        let count_text = if self.tx_history.is_empty() && self.tx_history_rx.is_some() {
            "SYNCING".to_string()
        } else {
            format!("{}", self.tx_history.len())
        };

        panel_frame(&self.colors).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_header(
                ui,
                &self.colors,
                "02",
                &format!("Transaction Tape // {}", count_text),
            );
            ui.add_space(10.0);

            if self.tx_history.is_empty() {
                ui.label(
                    egui::RichText::new("No transactions yet.")
                        .size(11.5)
                        .color(self.colors.text_muted),
                );
                return;
            }

            let records: Vec<TxRecord> = self.tx_history.clone();
            let accounts = &self.accounts;
            let t = ui.input(|i| i.time) as f32;
            let w = ui.available_width();

            // Fixed column offsets relative to the row's left edge;
            // AMOUNT is right-aligned against the STATUS column.
            let time_x = 6.0;
            let type_x = 84.0;
            let hash_x = 140.0;
            // AMOUNT starts past the longest hash + routing note so the
            // left-aligned columns never collide at the minimum width.
            let amount_x = 620.0;
            let status_x = w - 104.0;

            // Header row.
            let (hrect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
            let painter = ui.painter();
            let hcols = [
                (time_x, "TIME"),
                (type_x, "TYPE"),
                (hash_x, "HASH"),
                (amount_x, "AMOUNT"),
                (status_x, "STATUS"),
            ];
            for (x, label) in hcols {
                painter.text(
                    egui::pos2(hrect.left() + x, hrect.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    label_font(9.0),
                    self.colors.text_muted,
                );
            }
            painter.hline(
                hrect.x_range(),
                hrect.bottom() - 0.5,
                egui::Stroke::new(1.0, self.colors.border),
            );

            let mut any_pending = false;
            // Copy is deferred past the loop: the rows borrow
            // `self.accounts`, so `self.status` can't be set inside it.
            let mut copied_hash: Option<String> = None;
            for record in &records {
                let owner_idx = accounts
                    .iter()
                    .position(|a| a.lock_args == record.owner_lock_args);
                let counterparty_idx = record
                    .internal_counterparty_lock_args
                    .as_ref()
                    .and_then(|args| accounts.iter().position(|a| a.lock_args == *args));
                any_pending |= record.is_pending;

                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::click());
                let response = response
                    .on_hover_text("Click to copy transaction hash")
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    copied_hash = Some(record.tx_hash.clone());
                }
                let painter = ui.painter();
                if response.hovered() {
                    row_hover(painter, rect, &self.colors);
                }
                draw_tape_row(
                    painter,
                    rect,
                    &self.colors,
                    record,
                    owner_idx,
                    counterparty_idx,
                    (time_x, type_x, hash_x, amount_x, status_x),
                    t,
                );
                painter.hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    egui::Stroke::new(1.0, self.colors.border),
                );
            }

            if let Some(hash) = copied_hash {
                ui.ctx().copy_text(hash);
                self.status = Status::Info("Transaction hash copied!".to_string());
            }

            // Keep pending dots breathing without a per-frame repaint.
            if any_pending {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
        });
    }
}

/// Paint one tape row: TIME | TYPE | HASH | AMOUNT | STATUS.
#[allow(clippy::too_many_arguments)]
fn draw_tape_row(
    painter: &egui::Painter,
    rect: egui::Rect,
    colors: &AppColors,
    record: &TxRecord,
    account_index: Option<usize>,
    counterparty_index: Option<usize>,
    (time_x, type_x, hash_x, amount_x, status_x): (f32, f32, f32, f32, f32),
    t: f32,
) {
    let cy = rect.center().y;
    let body = egui::FontId::proportional(11.5);

    // TIME.
    let time_str = if record.timestamp > 0 {
        format_relative_time(record.timestamp)
    } else {
        "...".to_string()
    };
    painter.text(
        egui::pos2(rect.left() + time_x, cy),
        egui::Align2::LEFT_CENTER,
        time_str,
        body.clone(),
        colors.text_muted,
    );

    // TYPE badge: short codes, semantic color.
    let (code, code_color) = match record.tx_kind {
        TxKind::Incoming => ("IN", colors.accent2),
        TxKind::Outgoing => ("OUT", colors.danger),
        TxKind::DaoDeposit => ("DEP", colors.accent),
        TxKind::DaoPrepare => ("WD", colors.accent),
        TxKind::DaoWithdraw => ("UNLK", colors.accent),
    };
    paint_badge(
        painter,
        egui::pos2(rect.left() + type_x, cy),
        code,
        code_color,
    );

    // HASH, in full — block explorers and co-signers need the whole
    // thing; the smaller font keeps it clear of the amount column at
    // the minimum window width. Internal transfers keep their
    // account-routing note ("#0 → #2") beside it.
    let hash_end = painter
        .text(
            egui::pos2(rect.left() + hash_x, cy),
            egui::Align2::LEFT_CENTER,
            &record.tx_hash,
            egui::FontId::proportional(10.0),
            colors.text_muted,
        )
        .right();
    if let Some(idx) = account_index {
        let route = if let Some(cp_idx) = counterparty_index {
            let arrow = match record.tx_kind {
                TxKind::Incoming => "\u{2190}",
                _ => "\u{2192}",
            };
            format!("#{} {} #{}", idx, arrow, cp_idx)
        } else {
            format!("#{}", idx)
        };
        painter.text(
            egui::pos2(hash_end + 10.0, cy),
            egui::Align2::LEFT_CENTER,
            route,
            label_font(8.5),
            colors.text_muted,
        );
    }

    // AMOUNT: signed integer part in semantic color, fraction dim.
    // Internal transfers are neutral — money moved between our own
    // accounts, not in or out.
    let is_internal = counterparty_index.is_some();
    let (prefix, amount_color) = if is_internal {
        ("", colors.text)
    } else {
        match record.tx_kind {
            TxKind::Incoming => ("+", colors.accent2),
            _ => ("\u{2212}", colors.danger),
        }
    };
    let (int, frac) = ckb_split(record.amount);
    let int_rect = painter.text(
        egui::pos2(rect.left() + amount_x, cy),
        egui::Align2::LEFT_CENTER,
        format!("{}{}.", prefix, int),
        body.clone(),
        amount_color,
    );
    painter.text(
        egui::pos2(int_rect.right(), cy),
        egui::Align2::LEFT_CENTER,
        frac,
        body,
        colors.text_muted,
    );

    // STATUS.
    if record.is_pending {
        breathing_dot(
            painter,
            egui::pos2(rect.left() + status_x + 3.0, cy),
            colors.accent,
            t,
            false,
        );
        painter.text(
            egui::pos2(rect.left() + status_x + 12.0, cy),
            egui::Align2::LEFT_CENTER,
            "PENDING",
            label_font(9.0),
            colors.accent,
        );
    } else {
        painter.text(
            egui::pos2(rect.left() + status_x, cy),
            egui::Align2::LEFT_CENTER,
            "CONFIRMED",
            label_font(9.0),
            colors.accent2.gamma_multiply(0.7),
        );
    }
}

/// Painter-space twin of `utils::badge` for rows laid out by hand.
fn paint_badge(painter: &egui::Painter, left_center: egui::Pos2, text: &str, color: egui::Color32) {
    let galley = painter.layout_no_wrap(text.to_string(), label_font(8.5), color);
    let rect = egui::Rect::from_min_size(
        egui::pos2(left_center.x, left_center.y - galley.size().y / 2.0 - 2.0),
        galley.size() + egui::vec2(10.0, 4.0),
    );
    let tint = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 24);
    painter.rect_filled(rect, 0.0, tint);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90),
        ),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
}
