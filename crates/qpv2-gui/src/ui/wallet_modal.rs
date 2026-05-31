//! Centered modal for Create Wallet / Import Wallet flows.

use eframe::egui;
use qpv2_core::types::SpxVariant;

use crate::types::WalletModal;
use crate::App;

impl App {
    pub(crate) fn show_wallet_modal(&mut self, ctx: &egui::Context) {
        if self.wallet_modal == WalletModal::None {
            return;
        }

        let is_import = self.wallet_modal == WalletModal::Import;
        let title = if is_import {
            "Import Wallet"
        } else {
            "Create Wallet"
        };

        // Semi-transparent backdrop that consumes clicks.
        let screen_rect = ctx.input(|i| i.viewport_rect());
        let backdrop_clicked = egui::Area::new(egui::Id::new("wallet_modal_backdrop"))
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

        let modal_width = 420.0;
        let modal_pos = egui::pos2(
            (screen_rect.width() - modal_width) / 2.0,
            screen_rect.height() * 0.22,
        );

        egui::Area::new(egui::Id::new("wallet_modal_area"))
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

                        // Title
                        ui.label(
                            egui::RichText::new(title)
                                .size(20.0)
                                .strong()
                                .color(self.colors.text),
                        );
                        ui.add_space(16.0);

                        // Variant selector
                        ui.label(
                            egui::RichText::new("SPHINCS+ Variant")
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(2.0);
                        egui::ComboBox::from_id_salt("wallet_modal_variant")
                            .selected_text(format!("{}", self.new_wallet_variant))
                            .width(modal_width)
                            .show_ui(ui, |ui| {
                                for variant in &[
                                    SpxVariant::Sha2128S,
                                    SpxVariant::Sha2128F,
                                    SpxVariant::Shake128S,
                                    SpxVariant::Shake128F,
                                    SpxVariant::Sha2192S,
                                    SpxVariant::Sha2192F,
                                    SpxVariant::Shake192S,
                                    SpxVariant::Shake192F,
                                    SpxVariant::Sha2256S,
                                    SpxVariant::Sha2256F,
                                    SpxVariant::Shake256S,
                                    SpxVariant::Shake256F,
                                ] {
                                    ui.selectable_value(
                                        &mut self.new_wallet_variant,
                                        *variant,
                                        format!("{}", variant),
                                    );
                                }
                            });

                        ui.add_space(20.0);

                        // Auth method section
                        ui.label(
                            egui::RichText::new("Authentication Method")
                                .size(11.0)
                                .color(self.colors.text_muted),
                        );
                        ui.add_space(8.0);

                        let verb = if is_import { "Import" } else { "Create" };

                        // Keychain button (primary)
                        let kc_btn = egui::Button::new(
                            egui::RichText::new(format!(
                                "{} with {}",
                                verb,
                                keychain::short_name()
                            ))
                            .size(13.0)
                            .color(self.colors.bg),
                        )
                        .fill(self.colors.accent)
                        .corner_radius(8.0)
                        .min_size(egui::vec2(modal_width, 36.0));

                        if ui.add(kc_btn).clicked() {
                            let v = self.new_wallet_variant;
                            if is_import {
                                self.import_seed_phrase_with_keychain(v);
                            } else {
                                self.create_wallet_with_keychain(v);
                            }
                            self.wallet_modal = WalletModal::None;
                        }

                        ui.add_space(6.0);

                        // FIDO2 button (secondary)
                        let fido_btn = egui::Button::new(
                            egui::RichText::new(format!("{} with Security Key", verb))
                                .size(13.0)
                                .color(self.colors.text),
                        )
                        .fill(self.colors.surface2)
                        .stroke(egui::Stroke::new(1.0, self.colors.accent2))
                        .corner_radius(8.0)
                        .min_size(egui::vec2(modal_width, 36.0));

                        if ui.add(fido_btn).clicked() {
                            let v = self.new_wallet_variant;
                            if is_import {
                                self.import_seed_phrase_with_fido2(v);
                            } else {
                                self.create_wallet_with_fido2(v);
                            }
                            self.wallet_modal = WalletModal::None;
                        }

                        ui.add_space(6.0);

                        // Password button (tertiary)
                        let pw_btn = egui::Button::new(
                            egui::RichText::new(format!("{} with Password", verb))
                                .size(13.0)
                                .color(self.colors.text_muted),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.0, self.colors.border2))
                        .corner_radius(8.0)
                        .min_size(egui::vec2(modal_width, 36.0));

                        if ui.add(pw_btn).clicked() {
                            let v = self.new_wallet_variant;
                            if is_import {
                                self.import_seed_phrase_with_password(v);
                            } else {
                                self.create_wallet_with_password(v);
                            }
                            self.wallet_modal = WalletModal::None;
                        }

                        ui.add_space(14.0);

                        // Cancel button
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
                                .min_size(egui::vec2(modal_width, 36.0));
                                ui.add(cancel).clicked()
                            })
                            .inner;

                        if cancel_clicked {
                            self.wallet_modal = WalletModal::None;
                            self.new_wallet_name.clear();
                        }
                    });
            });

        if backdrop_clicked {
            self.wallet_modal = WalletModal::None;
            self.new_wallet_name.clear();
        }
    }
}
