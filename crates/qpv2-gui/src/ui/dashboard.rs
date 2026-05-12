//! Dashboard tab rendering.

use eframe::egui;

use crate::types::{
    format_ckb_balance, format_relative_time, format_with_commas, Tab, TxKind, TxRecord,
};
use crate::App;

impl App {
    pub(crate) fn show_dashboard_tab(&mut self, ui: &mut egui::Ui) {
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
        // Base fill is `colors.surface` — the mockup paints a subtle
        // 3-stop diagonal gradient on top (accent → accent2 →
        // accent3) plus a top-right corner glow.
        //
        // Index trick: inside the Frame closure we don't yet know the
        // card's final rect (Frame sizes itself after content lays
        // out), so we *reserve* shape indices for the gradient and
        // glow and fill them in after `Frame::show` returns — at
        // which point `frame_response.response.rect` is the real
        // card outline. Rendering order is preserved by the indices,
        // so the gradient + glow stay underneath the labels even
        // though we set them later.
        let mut gradient_idx = None;
        let mut spotlight_idx = None;
        let mut glow_idx = None;
        let frame_response = egui::Frame::new()
            .fill(self.colors.surface)
            .corner_radius(20.0)
            .outer_margin(egui::Margin::symmetric(30, 0))
            .inner_margin(egui::Margin::symmetric(34, 30))
            .stroke(egui::Stroke::new(1.0, self.colors.border2))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Reserve slots for gradient + spotlight + glow
                // before any label shapes are added, so they paint
                // underneath. Order: gradient (deepest layer) →
                // top-left spotlight (lights the balance number) →
                // top-right corner bloom.
                gradient_idx = Some(ui.painter().add(egui::Shape::Noop));
                spotlight_idx = Some(ui.painter().add(egui::Shape::Noop));
                glow_idx = Some(ui.painter().add(egui::Shape::Noop));

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

                // Render balance with the fractional part in accent green
                // to match the mockup style (e.g. "142,840." white + "50" green + " CKB" white).
                let syne = egui::FontFamily::Name("syne".into());

                let whole = total_shannons / crate::types::CKB_DECIMAL_PLACES;
                let frac = total_shannons % crate::types::CKB_DECIMAL_PLACES;
                let bal_size = 46.0;

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if frac == 0 {
                        ui.label(
                            egui::RichText::new(format!("{} CKB", format_with_commas(whole)))
                                .size(bal_size)
                                .family(syne.clone())
                                .color(self.colors.text),
                        );
                    } else {
                        let frac_str = format!("{:08}", frac);
                        ui.label(
                            egui::RichText::new(format!("{}.", format_with_commas(whole)))
                                .size(bal_size)
                                .family(syne.clone())
                                .color(self.colors.text),
                        );
                        ui.label(
                            egui::RichText::new(&frac_str[..2])
                                .size(bal_size)
                                .family(syne.clone())
                                .color(self.colors.accent),
                        );
                        ui.label(
                            egui::RichText::new(" CKB")
                                .size(bal_size)
                                .family(syne.clone())
                                .color(self.colors.text),
                        );
                    }
                });

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
                                .family(syne.clone())
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
                                .family(syne.clone())
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
                                .family(syne.clone())
                                .color(self.colors.accent3),
                        );
                    });
                });
            });

        // With the Frame finalized, `response.rect` includes the
        // outer_margin (30 px horizontal). Shrink it by that margin
        // to land on the actual painted card outline; otherwise the
        // gradient mesh and clip rect end up 30 px wider on each
        // side and bleed past the card horizontally. (egui can't do
        // rounded clipping; at the four rounded corners the mesh /
        // glow alpha is already near zero, so the sliver of leakage
        // past `corner_radius` is imperceptible.)
        let card_rect = frame_response.response.rect.shrink2(egui::vec2(30.0, 0.0));
        let painter = ui.painter_at(card_rect);

        if let Some(idx) = gradient_idx {
            // 135deg gradient: TL accent .07 → BR accent3 .05, with
            // accent2 .04 on the off-diagonal to bend the sweep.
            // Built as a rounded-rect mesh so it follows the card's
            // 20-px corner outline instead of leaving sharp corners
            // bleeding past the rounded fill.
            let tl = egui::Color32::from_rgba_unmultiplied(0, 255, 180, 18);
            let tr = egui::Color32::from_rgba_unmultiplied(0, 200, 255, 10);
            let brc = egui::Color32::from_rgba_unmultiplied(123, 94, 167, 13);
            let bl = egui::Color32::from_rgba_unmultiplied(0, 200, 255, 10);
            let mesh =
                crate::ui::common::rounded_rect_gradient_mesh(card_rect, 20.0, tl, tr, brc, bl);
            painter.set(idx, egui::Shape::mesh(mesh));
        }

        if let Some(idx) = spotlight_idx {
            // Top-left spotlight — lights the balance number from
            // behind. Not part of the mockup CSS strictly (the
            // mockup only has the top-right ::after bloom), but the
            // reference screenshot shows a clear accent halo around
            // the balance, which gives the panel its sense of
            // depth. Centered roughly behind the start of the
            // balance digits, broader than the corner glow so it
            // reads as ambient illumination rather than a localized
            // highlight.
            let spot_center = egui::pos2(card_rect.left() + 120.0, card_rect.top() + 80.0);
            let mesh =
                crate::ui::common::smooth_glow_mesh(spot_center, 170.0, self.colors.accent, 26);
            painter.set(idx, egui::Shape::mesh(mesh));
        }

        if let Some(idx) = glow_idx {
            // Top-right corner glow — `.balance-hero::after`:
            // 200×200 box at top:-40, right:-40, radial-gradient
            // peaking at accent .08 and going transparent at 70%.
            // Decoded geometry: glow center sits at
            // `(card.right - 60, card.top + 60)` *inside* the card,
            // visible radius ≈100 px, peak alpha 0.08 ≈ 20/255.
            let glow_center = egui::pos2(card_rect.right() - 60.0, card_rect.top() + 60.0);
            let mesh =
                crate::ui::common::smooth_glow_mesh(glow_center, 100.0, self.colors.accent, 20);
            painter.set(idx, egui::Shape::mesh(mesh));
        }

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

                // ── Recent Transactions ──
                if !self.tx_history.is_empty() || self.tx_history_rx.is_some() {
                    let syne = egui::FontFamily::Name("syne".into());

                    // Section header with pill badge.
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Recent Transactions")
                                .size(15.0)
                                .family(syne)
                                .strong()
                                .color(self.colors.text),
                        );
                        ui.add_space(10.0);

                        let badge_text = if self.tx_history_rx.is_some() {
                            "loading...".to_string()
                        } else {
                            format!("{} total", self.tx_history.len())
                        };
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_unmultiplied(0, 255, 180, 20))
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(badge_text)
                                        .size(8.5)
                                        .family(egui::FontFamily::Monospace)
                                        .color(self.colors.accent),
                                );
                            });
                    });

                    ui.add_space(12.0);

                    // Transaction cards.
                    // Clone to avoid borrow conflict with self in draw_tx_card.
                    let records: Vec<TxRecord> = self.tx_history.clone();
                    let accounts = &self.accounts;
                    for record in &records {
                        let owner_idx = accounts.iter().position(|a| *a == record.owner_lock_args);
                        let counterparty_idx = record
                            .internal_counterparty_lock_args
                            .as_ref()
                            .and_then(|args| accounts.iter().position(|a| a == args));
                        Self::draw_tx_card(ui, &self.colors, record, owner_idx, counterparty_idx);
                        ui.add_space(7.0);
                    }
                }

                ui.add_space(12.0);

                // ── Status messages ──
                self.show_status(ui);
            });
        });
    }

    /// Render a single transaction card
    fn draw_tx_card(
        ui: &mut egui::Ui,
        colors: &crate::types::AppColors,
        record: &TxRecord,
        account_index: Option<usize>,
        counterparty_index: Option<usize>,
    ) {
        let syne = egui::FontFamily::Name("syne".into());

        // Pick icon and icon background color based on transaction type.
        let (icon, icon_bg) = match record.tx_kind {
            TxKind::Outgoing => (
                "\u{2191}",
                egui::Color32::from_rgba_unmultiplied(255, 77, 109, 31),
            ),
            TxKind::Incoming => (
                "\u{2193}",
                egui::Color32::from_rgba_unmultiplied(0, 255, 180, 26),
            ),
            TxKind::DaoDeposit | TxKind::DaoPrepare | TxKind::DaoWithdraw => (
                "\u{2b21}",
                egui::Color32::from_rgba_unmultiplied(155, 127, 212, 38),
            ),
        };

        let id = ui.make_persistent_id(&record.tx_hash);
        let margin = egui::Margin::symmetric(17, 13);

        // ── Hover effect (cosmetic only) ──
        // egui::Frame decides fill/stroke before layout, so we read the hover
        // state from the *previous* frame's rect (stored in egui temp data).
        // The one-frame delay is imperceptible and avoids double-painting.
        let last_rect: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(id));
        let is_hovered = last_rect.is_some_and(|r| ui.rect_contains_pointer(r));

        let fill = if is_hovered {
            colors.surface2
        } else {
            colors.surface
        };
        let stroke_color = if is_hovered {
            colors.border2
        } else {
            colors.border
        };

        let content_response = egui::Frame::new()
            .fill(fill)
            .corner_radius(12.0)
            .inner_margin(margin)
            .stroke(egui::Stroke::new(1.0, stroke_color))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 13.0;

                    // Icon with colored background.
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                    ui.painter().rect_filled(icon_rect, 9.0, icon_bg);
                    ui.painter().text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        icon,
                        egui::FontId::proportional(16.0),
                        colors.text,
                    );

                    // Middle: name + tx hash.
                    let name = match record.tx_kind {
                        TxKind::DaoDeposit => "DAO Deposit".to_string(),
                        TxKind::DaoPrepare => "DAO Request Withdrawal".to_string(),
                        TxKind::DaoWithdraw => "DAO Withdraw".to_string(),
                        TxKind::Incoming => "Received".to_string(),
                        TxKind::Outgoing => "Sent".to_string(),
                    };

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(name).size(13.0).color(colors.text));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                egui::RichText::new(&record.tx_hash)
                                    .size(9.0)
                                    .family(egui::FontFamily::Monospace)
                                    .color(colors.text_muted),
                            );
                            // Account index badge: "#N" for external,
                            // "#N → #M" for outgoing internal, "#N ← #M" for incoming internal.
                            if let Some(idx) = account_index {
                                let badge_text = if let Some(cp_idx) = counterparty_index {
                                    let arrow = match record.tx_kind {
                                        TxKind::Incoming => "\u{2190}", // ←
                                        _ => "\u{2192}",                // →
                                    };
                                    format!("#{} {} #{}", idx, arrow, cp_idx)
                                } else {
                                    format!("#{}", idx)
                                };
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgba_unmultiplied(0, 200, 255, 25))
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(5, 1))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(badge_text)
                                                .size(8.0)
                                                .family(egui::FontFamily::Monospace)
                                                .color(colors.accent2),
                                        );
                                    });
                            }
                        });
                    });

                    // Right side: amount + time.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if record.is_pending {
                                    ui.label(
                                        egui::RichText::new("Pending")
                                            .size(14.0)
                                            .family(syne.clone())
                                            .color(colors.warn),
                                    );
                                } else {
                                    let is_internal = counterparty_index.is_some();
                                    let (prefix, color) = if is_internal {
                                        // Internal transfer: neutral color, no +/-.
                                        ("", colors.text_muted)
                                    } else {
                                        match record.tx_kind {
                                            TxKind::Incoming => ("+", colors.accent),
                                            _ => ("\u{2212}", colors.danger),
                                        }
                                    };
                                    let whole = record.amount / crate::types::CKB_DECIMAL_PLACES;
                                    let frac = record.amount % crate::types::CKB_DECIMAL_PLACES;
                                    let amount_str = if frac == 0 {
                                        format!("{}{} CKB", prefix, format_with_commas(whole))
                                    } else {
                                        let frac_str = format!("{:08}", frac);
                                        format!(
                                            "{}{}.{} CKB",
                                            prefix,
                                            format_with_commas(whole),
                                            &frac_str[..2]
                                        )
                                    };
                                    ui.label(
                                        egui::RichText::new(amount_str)
                                            .size(14.0)
                                            .family(syne.clone())
                                            .color(color),
                                    );
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                let time_str = if record.timestamp > 0 {
                                    format_relative_time(record.timestamp)
                                } else {
                                    "...".to_string()
                                };
                                ui.label(
                                    egui::RichText::new(time_str)
                                        .size(10.0)
                                        .color(colors.text_muted),
                                );
                            });
                        });
                    });
                });
            });

        // Store this frame's rect for next frame's hover detection.
        let card_rect = content_response.response.rect;
        ui.ctx().data_mut(|d| d.insert_temp(id, card_rect));

        if is_hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
}
