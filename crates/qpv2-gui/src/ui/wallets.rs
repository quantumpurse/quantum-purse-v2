//! Wallets tab rendering — create, inspect, and delete wallets.

use eframe::egui;
use qpv2_core::types::AuthMethod;
use qpv2_core::KeyVault;

use super::common::{paint_corner_accent, CardHover};
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

                // ── Action cards (3-column) ──
                ui.columns(3, |cols| {
                    // Create Wallet
                    let hover = CardHover::new(&cols[0], "wallet-create", &self.colors);

                    let create_card = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(14.0)
                        .inner_margin(egui::Margin::symmetric(20, 24))
                        .stroke(hover.stroke)
                        .show(&mut cols[0], |ui| {
                            ui.vertical_centered(|ui| {
                                hover.apply_lift(ui);
                                ui.label(egui::RichText::new("\u{2726}").size(26.0));
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new("Create Wallet")
                                        .size(14.0)
                                        .strong()
                                        .color(self.colors.text),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("Generate a new wallet with a fresh seed.")
                                        .size(11.0)
                                        .color(self.colors.text_muted),
                                );
                            });
                        })
                        .response;

                    paint_corner_accent(
                        cols[0].painter(),
                        create_card.rect,
                        14.0,
                        self.colors.accent,
                    );
                    hover.commit(&create_card);

                    if create_card.interact(egui::Sense::click()).clicked() {
                        self.wallet_modal = crate::types::WalletModal::Create;
                        self.new_wallet_name.clear();
                        self.new_wallet_variant = qpv2_core::types::SpxVariant::Sha2128S;
                    }

                    // Import Seed
                    let hover = CardHover::new(&cols[1], "wallet-import", &self.colors);

                    let import_card = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(14.0)
                        .inner_margin(egui::Margin::symmetric(20, 24))
                        .stroke(hover.stroke)
                        .show(&mut cols[1], |ui| {
                            ui.vertical_centered(|ui| {
                                hover.apply_lift(ui);
                                ui.label(egui::RichText::new("\u{2b07}").size(26.0));
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new("Import Seed")
                                        .size(14.0)
                                        .strong()
                                        .color(self.colors.text),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("Restore a wallet from a seed phrase.")
                                        .size(11.0)
                                        .color(self.colors.text_muted),
                                );
                            });
                        })
                        .response;

                    paint_corner_accent(
                        cols[1].painter(),
                        import_card.rect,
                        14.0,
                        self.colors.warn,
                    );
                    hover.commit(&import_card);

                    if import_card.interact(egui::Sense::click()).clicked() {
                        self.wallet_modal = crate::types::WalletModal::Import;
                        self.new_wallet_name.clear();
                        self.new_wallet_variant = qpv2_core::types::SpxVariant::Sha2128S;
                    }

                    // Export Seed
                    let hover = CardHover::new(&cols[2], "wallet-export", &self.colors);

                    let export_card = egui::Frame::new()
                        .fill(hover.fill)
                        .corner_radius(14.0)
                        .inner_margin(egui::Margin::symmetric(20, 24))
                        .stroke(hover.stroke)
                        .show(&mut cols[2], |ui| {
                            ui.vertical_centered(|ui| {
                                hover.apply_lift(ui);
                                ui.label(egui::RichText::new("\u{2b06}").size(26.0));
                                ui.add_space(6.0);
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

                    paint_corner_accent(
                        cols[2].painter(),
                        export_card.rect,
                        14.0,
                        self.colors.accent2,
                    );
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
                                tracing::error!("No authentication method set.");
                                self.status =
                                    Status::Error("No authentication method set.".to_string());
                            }
                        }
                    }
                });

                ui.add_space(20.0);

                // ── Saved Wallets section ──
                let wallet_count = self.wallet_cache.len();

                let pill =
                    |ui: &mut egui::Ui, fill: egui::Color32, text: String, color: egui::Color32| {
                        egui::Frame::new()
                            .fill(fill)
                            .corner_radius(4.0)
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
                    let mut delete_target: Option<u32> = None;
                    let mut rename_target: Option<(u32, String)> = None;
                    let mut switch_target: Option<(u32, String)> = None;

                    for i in 0..self.wallet_cache.len() {
                        let cw = &self.wallet_cache[i];
                        let is_active = cw.id == self.wallet_id;

                        let hover = CardHover::new(ui, ("wallet-row", i), &self.colors);

                        let cw_id = cw.id;
                        let cw_name = cw.name.clone();
                        let cw_variant = cw.spx_variant;
                        let cw_auth = cw.auth_method.clone();
                        let cw_acct_count = cw.account_count;
                        let cw_path = cw.path.clone();

                        let row_resp = egui::Frame::new()
                            .fill(hover.fill)
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(18, 16))
                            .stroke(hover.stroke)
                            .show(ui, |ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Min),
                                    |ui| {
                                        let (tile_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(48.0, 64.0),
                                            egui::Sense::hover(),
                                        );
                                        paint_wallet_tile(
                                            ui.painter(),
                                            tile_rect,
                                            &cw_name,
                                            self.colors.surface2,
                                            self.colors.border,
                                            self.colors.text_muted,
                                        );

                                        ui.add_space(12.0);

                                        // Info column
                                        let info_width = ui.available_width();
                                        ui.vertical(|ui| {
                                            ui.set_width(info_width);
                                            let is_renaming = self.rename_wallet_id == Some(cw_id);

                                            if is_renaming {
                                                ui.horizontal(|ui| {
                                                    let field = egui::TextEdit::singleline(
                                                        &mut self.rename_wallet_buf,
                                                    )
                                                    .desired_width(140.0)
                                                    .font(egui::TextStyle::Body);
                                                    let response = ui.add(field);

                                                    if response.lost_focus()
                                                        && ui.input(|i| {
                                                            i.key_pressed(egui::Key::Enter)
                                                        })
                                                    {
                                                        rename_target = Some((
                                                            cw_id,
                                                            self.rename_wallet_buf.clone(),
                                                        ));
                                                    }

                                                    let ok_btn = egui::Button::new(
                                                        egui::RichText::new("\u{2713}")
                                                            .size(13.0)
                                                            .color(self.colors.accent),
                                                    )
                                                    .fill(egui::Color32::TRANSPARENT);

                                                    if ui.add(ok_btn).clicked() {
                                                        rename_target = Some((
                                                            cw_id,
                                                            self.rename_wallet_buf.clone(),
                                                        ));
                                                    }

                                                    let cancel_btn = egui::Button::new(
                                                        egui::RichText::new("\u{2715}")
                                                            .size(13.0)
                                                            .color(self.colors.text_muted),
                                                    )
                                                    .fill(egui::Color32::TRANSPARENT);

                                                    if ui.add(cancel_btn).clicked() {
                                                        self.rename_wallet_id = None;
                                                        self.rename_wallet_buf.clear();
                                                    }
                                                });
                                            } else {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        egui::RichText::new(&cw_name)
                                                            .size(14.0)
                                                            .strong()
                                                            .color(self.colors.text),
                                                    );
                                                    let pen = ui.add(
                                                        egui::Button::new(
                                                            egui::RichText::new("\u{270f}")
                                                                .size(11.0)
                                                                .color(self.colors.text_muted),
                                                        )
                                                        .fill(egui::Color32::TRANSPARENT)
                                                        .frame(false),
                                                    );
                                                    if pen.clicked() {
                                                        self.rename_wallet_id = Some(cw_id);
                                                        self.rename_wallet_buf = cw_name.clone();
                                                    }

                                                    if is_active {
                                                        ui.add_space(4.0);
                                                        pill(
                                                            ui,
                                                            self.colors.accent_tint,
                                                            "ACTIVE".to_string(),
                                                            self.colors.accent,
                                                        );
                                                    }
                                                });
                                            }

                                            ui.add_space(4.0);

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
                                                    AuthMethod::Keychain => (
                                                        keychain::short_name(),
                                                        self.colors.accent2,
                                                    ),
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

                                            ui.add_space(4.0);

                                            ui.horizontal(|ui| {
                                                pill(
                                                    ui,
                                                    self.colors.surface2,
                                                    cw_path.clone(),
                                                    self.colors.text_muted,
                                                );

                                                // Hold-to-delete pill
                                                const HOLD_SECS: f64 = 2.0;
                                                let del_id = ui.id().with(("del-hold", cw_id));
                                                let press_start: Option<f64> =
                                                    ui.ctx().memory(|m| m.data.get_temp(del_id));

                                                let del_resp = egui::Frame::new()
                                                    .fill(egui::Color32::TRANSPARENT)
                                                    .stroke(egui::Stroke::new(
                                                        1.0,
                                                        egui::Color32::from_rgba_unmultiplied(
                                                            255, 77, 109, 77,
                                                        ),
                                                    ))
                                                    .corner_radius(4.0)
                                                    .inner_margin(egui::Margin::symmetric(8, 2))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            egui::RichText::new("\u{1f5d1} Delete")
                                                                .size(8.5)
                                                                .family(egui::FontFamily::Monospace)
                                                                .color(self.colors.danger),
                                                        );
                                                    })
                                                    .response
                                                    .interact(egui::Sense::click_and_drag());

                                                if del_resp.is_pointer_button_down_on() {
                                                    let now = ui.input(|i| i.time);
                                                    if press_start.is_none()
                                                        && del_resp.contains_pointer()
                                                        && ui.input(|i| i.pointer.any_pressed())
                                                    {
                                                        ui.ctx().memory_mut(|m| {
                                                            m.data.insert_temp(del_id, now)
                                                        });
                                                    }
                                                    let start = press_start.unwrap_or(now);
                                                    let progress = ((now - start) / HOLD_SECS)
                                                        .clamp(0.0, 1.0)
                                                        as f32;
                                                    paint_hold_border(
                                                        ui.painter(),
                                                        del_resp.rect,
                                                        4.0,
                                                        progress,
                                                        egui::Stroke::new(
                                                            1.0,
                                                            egui::Color32::from_rgb(255, 77, 109),
                                                        ),
                                                    );
                                                    if progress >= 1.0 {
                                                        delete_target = Some(cw_id);
                                                        ui.ctx().memory_mut(|m| {
                                                            m.data.remove::<f64>(del_id)
                                                        });
                                                    }
                                                    ui.ctx().request_repaint();
                                                } else if press_start.is_some() {
                                                    ui.ctx().memory_mut(|m| {
                                                        m.data.remove::<f64>(del_id)
                                                    });
                                                }
                                            });
                                        });
                                    },
                                );
                            });

                        hover.commit(&row_resp.response);
                        if !is_active {
                            let click = row_resp
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if click.clicked() {
                                switch_target = Some((cw_id, cw_name.clone()));
                            }
                        }
                        ui.add_space(6.0);
                    }

                    // Handle delete outside the iteration to avoid borrow issues.
                    if let Some(id) = delete_target {
                        let _ = keychain::delete_key(id);
                        let _ = ckb_node::wallet_helpers::lc::clear_all_scripts(&self.qp_client);

                        match KeyVault::remove_wallet(id) {
                            Ok(()) => {
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
                                let msg = format!("Failed to remove wallet: {}", e);
                                tracing::error!("{}", msg);
                                self.status = Status::Error(msg);
                            }
                        }
                    }

                    // Handle rename outside the iteration because
                    // `refresh_wallet_cache()` replaces the Vec being iterated.
                    if let Some((id, new_name)) = rename_target {
                        let trimmed = new_name.trim();
                        match qpv2_core::db::wallets::rename_wallet(id, trimmed) {
                            Ok(()) => {
                                if id == self.wallet_id {
                                    self.wallet_name = trimmed.to_string();
                                }
                                self.refresh_wallet_cache();
                                self.status =
                                    Status::Info(format!("Wallet renamed to '{}'.", trimmed,));
                            }
                            Err(e) => {
                                let msg = format!("Failed to rename wallet: {}", e,);
                                tracing::error!("{}", msg);
                                self.status = Status::Error(msg);
                            }
                        }
                        self.rename_wallet_id = None;
                        self.rename_wallet_buf.clear();
                    }

                    if let Some((id, name)) = switch_target {
                        self.switch_wallet(id, &name);
                    }
                }

                ui.add_space(16.0);
                self.show_status(ui);
            });
        });
    }
}

fn paint_hold_border(
    painter: &egui::Painter,
    rect: egui::Rect,
    cr: f32,
    progress: f32,
    stroke: egui::Stroke,
) {
    use std::f32::consts::{FRAC_PI_2, PI};

    let w = rect.width();
    let h = rect.height();
    let cx = rect.center().x;

    let seg_lengths: [f32; 9] = [
        w / 2.0 - cr,
        FRAC_PI_2 * cr,
        h - 2.0 * cr,
        FRAC_PI_2 * cr,
        w - 2.0 * cr,
        FRAC_PI_2 * cr,
        h - 2.0 * cr,
        FRAC_PI_2 * cr,
        w / 2.0 - cr,
    ];
    let total: f32 = seg_lengths.iter().sum();
    let mut budget = total * progress;

    let mut points = Vec::with_capacity(72);
    points.push(egui::pos2(cx, rect.min.y));

    for (i, &seg_len) in seg_lengths.iter().enumerate() {
        if budget <= 0.0 {
            break;
        }
        let usable = budget.min(seg_len);
        let frac = if seg_len > 0.0 { usable / seg_len } else { 0.0 };

        match i {
            0 => points.push(egui::pos2(cx + usable, rect.min.y)),
            1 => {
                let c = egui::pos2(rect.max.x - cr, rect.min.y + cr);
                let n = (8.0 * frac).ceil().max(1.0) as usize;
                for s in 1..=n {
                    let a = -FRAC_PI_2 + (s as f32 / n as f32) * frac * FRAC_PI_2;
                    points.push(c + egui::vec2(cr * a.cos(), cr * a.sin()));
                }
            }
            2 => points.push(egui::pos2(rect.max.x, rect.min.y + cr + usable)),
            3 => {
                let c = egui::pos2(rect.max.x - cr, rect.max.y - cr);
                let n = (8.0 * frac).ceil().max(1.0) as usize;
                for s in 1..=n {
                    let a = (s as f32 / n as f32) * frac * FRAC_PI_2;
                    points.push(c + egui::vec2(cr * a.cos(), cr * a.sin()));
                }
            }
            4 => points.push(egui::pos2(rect.max.x - cr - usable, rect.max.y)),
            5 => {
                let c = egui::pos2(rect.min.x + cr, rect.max.y - cr);
                let n = (8.0 * frac).ceil().max(1.0) as usize;
                for s in 1..=n {
                    let a = FRAC_PI_2 + (s as f32 / n as f32) * frac * FRAC_PI_2;
                    points.push(c + egui::vec2(cr * a.cos(), cr * a.sin()));
                }
            }
            6 => points.push(egui::pos2(rect.min.x, rect.max.y - cr - usable)),
            7 => {
                let c = egui::pos2(rect.min.x + cr, rect.min.y + cr);
                let n = (8.0 * frac).ceil().max(1.0) as usize;
                for s in 1..=n {
                    let a = PI + (s as f32 / n as f32) * frac * FRAC_PI_2;
                    points.push(c + egui::vec2(cr * a.cos(), cr * a.sin()));
                }
            }
            8 => points.push(egui::pos2(rect.min.x + cr + usable, rect.min.y)),
            _ => {}
        }

        budget -= usable;
    }

    if points.len() >= 2 {
        painter.add(egui::Shape::line(points, stroke));
    }
}

fn paint_wallet_tile(
    painter: &egui::Painter,
    rect: egui::Rect,
    name: &str,
    fill: egui::Color32,
    border: egui::Color32,
    text_color: egui::Color32,
) {
    let cr = 8.0;
    painter.rect_filled(rect, cr, fill);
    painter.rect_stroke(
        rect,
        cr,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );

    let stripe = egui::Color32::from_rgba_unmultiplied(border.r(), border.g(), border.b(), 40);
    let step = 8.0;
    let clip = rect.shrink(1.0);
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        let p0 = egui::pos2(x, rect.bottom());
        let p1 = egui::pos2(x + rect.height(), rect.top());
        let p0c = egui::pos2(
            p0.x.clamp(clip.left(), clip.right()),
            p0.y.clamp(clip.top(), clip.bottom()),
        );
        let p1c = egui::pos2(
            p1.x.clamp(clip.left(), clip.right()),
            p1.y.clamp(clip.top(), clip.bottom()),
        );
        painter.line_segment([p0c, p1c], egui::Stroke::new(0.5, stripe));
        x += step;
    }

    let letter = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?');
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter.to_string(),
        egui::FontId::new(14.0, egui::FontFamily::Monospace),
        text_color,
    );
}
