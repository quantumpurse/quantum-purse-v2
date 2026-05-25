//! Wallet selector popup — quick switch between wallets.

use eframe::egui;
use qpv2_core::types::AuthMethod;

use crate::App;

impl App {
    pub(crate) fn show_wallet_selector_popup(&mut self, ctx: &egui::Context) {
        if !self.wallet_selector_open {
            return;
        }

        let Some(selector_rect) = self.wallet_selector_rect else {
            return;
        };

        let dropdown_pos = egui::pos2(selector_rect.left(), selector_rect.bottom() + 4.0);

        let area_response = egui::Area::new(egui::Id::new("wallet_selector_dropdown"))
            .fixed_pos(dropdown_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border))
                    .corner_radius(8.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_width(selector_rect.width() - 24.0);

                        ui.label(
                            egui::RichText::new("WALLETS")
                                .size(8.5)
                                .family(egui::FontFamily::Monospace)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(6.0);

                        let mut switch_target: Option<(u32, String)> = None;

                        for cw in &self.wallet_cache {
                            let selected = cw.id == self.wallet_id;
                            let row_bg = if selected {
                                self.colors.accent_tint
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let response = egui::Frame::new()
                                .fill(row_bg)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let name_color = if selected {
                                            self.colors.accent
                                        } else {
                                            self.colors.text
                                        };
                                        ui.label(
                                            egui::RichText::new(&cw.name)
                                                .size(12.5)
                                                .color(name_color),
                                        );

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let (auth_text, auth_color) = match &cw.auth_method
                                                {
                                                    AuthMethod::Password => {
                                                        ("PWD", self.colors.text_muted)
                                                    }
                                                    AuthMethod::Keychain => {
                                                        ("KEY", self.colors.accent2)
                                                    }
                                                    AuthMethod::Fido2 { .. } => {
                                                        ("FIDO", self.colors.accent3)
                                                    }
                                                };
                                                egui::Frame::new()
                                                    .fill(egui::Color32::from_rgba_unmultiplied(
                                                        auth_color.r(),
                                                        auth_color.g(),
                                                        auth_color.b(),
                                                        20,
                                                    ))
                                                    .corner_radius(4.0)
                                                    .inner_margin(egui::Margin::symmetric(6, 1))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            egui::RichText::new(auth_text)
                                                                .size(8.5)
                                                                .family(egui::FontFamily::Monospace)
                                                                .color(auth_color),
                                                        );
                                                    });

                                                ui.add_space(4.0);

                                                egui::Frame::new()
                                                    .fill(self.colors.surface2)
                                                    .corner_radius(4.0)
                                                    .inner_margin(egui::Margin::symmetric(6, 1))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                "{}",
                                                                cw.spx_variant
                                                            ))
                                                            .size(8.5)
                                                            .family(egui::FontFamily::Monospace)
                                                            .color(self.colors.text_muted),
                                                        );
                                                    });
                                            },
                                        );
                                    });
                                })
                                .response;

                            let click = response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if click.clicked() && !selected {
                                switch_target = Some((cw.id, cw.name.clone()));
                            }
                        }

                        if let Some((id, name)) = switch_target {
                            self.switch_wallet(id, &name);
                        }
                    });
            });

        // Click outside to close.
        if ctx.input(|i| i.pointer.any_click()) {
            let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
            if let Some(pos) = pointer_pos {
                let dropdown_rect = area_response.response.rect;
                if !dropdown_rect.contains(pos) && !selector_rect.contains(pos) {
                    self.wallet_selector_open = false;
                }
            }
        }
    }
}
