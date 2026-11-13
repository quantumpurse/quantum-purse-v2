//! Centered modal for Create Wallet / Import Wallet flows, in the
//! Flight Deck idiom: sharp hairline-framed surface, indexed sections.
//! Seed phrase entry/reveal happens in the external pinentry prompt,
//! so the modal only collects name, parameter set, and auth method.
//! Its form, `create_import_wallet_form`, is also the setup screen's.

use eframe::egui;
use qpv2_core::types::{SingleSigConvention, SpxVariant};

use crate::types::{label_font, WalletModal};
use crate::ui::utils::{
    device_waiting_line, fido2_waiting_line, ghost_button, pin_popup_above_modal, section_header,
    v1_import_checkbox, SectionCounter, TREZOR_WAITING_TEXT,
};
use crate::App;

/// Width of the create / import form, shared by the New Wallet dialog and
/// the setup screen.
pub(crate) const WALLET_FORM_W: f32 = 440.0;

impl App {
    pub(crate) fn show_wallet_modal(&mut self, ctx: &egui::Context) {
        if self.wallet_modal == WalletModal::None {
            return;
        }

        let is_import = self.wallet_modal == WalletModal::Import;

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

        let modal_pos = egui::pos2(
            (screen_rect.width() - WALLET_FORM_W) / 2.0,
            screen_rect.height() * 0.1,
        );

        egui::Area::new(egui::Id::new("wallet_modal_area"))
            .fixed_pos(modal_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border2))
                    .inner_margin(egui::Margin::symmetric(20, 18))
                    .show(ui, |ui| {
                        ui.set_width(WALLET_FORM_W);
                        if let Some(import) = self.create_import_wallet_form(ui, is_import) {
                            self.wallet_modal = if import {
                                WalletModal::Import
                            } else {
                                WalletModal::Create
                            };
                        }

                        // Cancel belongs to the dialog; the setup screen shows
                        // the form without it.
                        ui.add_space(14.0);
                        let (rule, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 1.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().hline(
                            rule.x_range(),
                            rule.center().y,
                            egui::Stroke::new(1.0, self.colors.border),
                        );
                        ui.add_space(10.0);
                        let cancel = ghost_button(
                            &self.colors,
                            "Cancel",
                            egui::vec2(ui.available_width(), 34.0),
                        );
                        if ui
                            .add_enabled(!self.external_wallet_setup_in_flight(), cancel)
                            .clicked()
                        {
                            self.wallet_modal = WalletModal::None;
                            self.new_wallet_name.clear();
                            self.import_from_v1 = false;
                        }
                    });
            });

        // A device setup keeps the dialog open; see `create_import_wallet_form`.
        if backdrop_clicked && !self.external_wallet_setup_in_flight() {
            self.wallet_modal = WalletModal::None;
            self.new_wallet_name.clear();
            self.import_from_v1 = false;
        }
    }

    /// The create / import form, shared by the New Wallet dialog and the setup
    /// screen. Returns the mode picked on its toggle; each caller stores it.
    pub(crate) fn create_import_wallet_form(
        &mut self,
        ui: &mut egui::Ui,
        is_import: bool,
    ) -> Option<bool> {
        let mut picked = None;
        let c_accent = self.colors.accent;
        let c_muted = self.colors.text_muted;
        let c_border = self.colors.border;
        // A Trezor connect or security-key setup started here locks the
        // inputs and exits until it ends.
        let connecting = self.external_wallet_setup_in_flight();

        // ── Title row + create / import toggle + hairline ──
        let subtitle = if is_import {
            "// IMPORT SEED"
        } else {
            "// INITIALIZE"
        };
        let toggle_size = egui::vec2(86.0, 26.0);
        // Start the row at the toggle's height so the title centres on it.
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), toggle_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("WALLET")
                        .font(label_font(11.0))
                        .color(c_accent),
                );
                ui.label(
                    egui::RichText::new(subtitle)
                        .font(label_font(11.0))
                        .color(c_muted),
                );
                // Right to left, so the modes are listed in reverse.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (label, import) in [("IMPORT", true), ("CREATION", false)] {
                        let mut cell = ghost_button(&self.colors, label, toggle_size);
                        if import == is_import {
                            cell = cell
                                .fill(self.colors.accent_tint)
                                .stroke(egui::Stroke::new(1.0, self.colors.accent));
                        }
                        if ui.add_enabled(!connecting, cell).clicked() {
                            picked = Some(import);
                        }
                    }
                });
            },
        );
        ui.add_space(8.0);
        let (rule, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().hline(
            rule.x_range(),
            rule.center().y,
            egui::Stroke::new(1.0, c_border),
        );
        ui.add_space(14.0);

        // Sections are numbered in render order because Source only
        // appears on import; fixed codes would gap on the create flow.
        let mut sec = SectionCounter::new();

        // ── Name ──
        section_header(ui, &self.colors, &sec.next_code(), "Name");
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(self.colors.surface2)
            .stroke(egui::Stroke::new(1.0, c_border))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.add_enabled(
                    !connecting,
                    egui::TextEdit::singleline(&mut self.new_wallet_name)
                        .hint_text("auto-generated if empty")
                        .desired_width(ui.available_width())
                        .font(egui::FontId::monospace(12.5))
                        .frame(egui::Frame::NONE),
                );
            });

        ui.add_space(14.0);

        // ── Parameter Set ──
        section_header(ui, &self.colors, &sec.next_code(), "Parameter Set");
        ui.add_space(6.0);
        ui.add_enabled_ui(!connecting, |ui| {
            let variant_combo = egui::ComboBox::from_id_salt("wallet_modal_variant")
                .selected_text(
                    egui::RichText::new(format!("SPHINCS+ {}", self.new_wallet_variant))
                        .size(12.0)
                        .color(c_accent),
                )
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for variant in &[
                        SpxVariant::Sha2128F,
                        SpxVariant::Sha2128S,
                        SpxVariant::Shake128F,
                        SpxVariant::Shake128S,
                        SpxVariant::Sha2192F,
                        SpxVariant::Sha2192S,
                        SpxVariant::Shake192F,
                        SpxVariant::Shake192S,
                        SpxVariant::Sha2256F,
                        SpxVariant::Sha2256S,
                        SpxVariant::Shake256F,
                        SpxVariant::Shake256S,
                    ] {
                        let selected = self.new_wallet_variant == *variant;
                        let text = egui::RichText::new(format!("{}", variant))
                            .size(12.0)
                            .color(if selected { c_accent } else { self.colors.text });
                        ui.selectable_value(&mut self.new_wallet_variant, *variant, text);
                    }
                });
            pin_popup_above_modal(ui, &variant_combo.response);
        });

        ui.add_space(14.0);

        // ── Source (import only) ──
        // v1 web-wallet mnemonics derive the same keys but need the v1
        // single-sig address convention for existing funds to stay visible.
        if is_import {
            section_header(ui, &self.colors, &sec.next_code(), "Source");
            ui.add_space(6.0);
            ui.add_enabled_ui(!connecting, |ui| {
                v1_import_checkbox(ui, &self.colors, &mut self.import_from_v1);
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("v1 used a different address format. Check this so the funds on your v1 addresses stay visible.")
                    .size(10.0)
                    .color(c_muted),
            );
            ui.add_space(14.0);
        }

        // ── Authentication ──
        section_header(ui, &self.colors, &sec.next_code(), "Authentication");
        ui.add_space(8.0);

        let full_w = ui.available_width();
        let btn_size = egui::vec2(full_w, 34.0);
        // Any of our Trezor operations also disables the create buttons,
        // including a reconnect or a signing for the open wallet: creating a
        // wallet switches to it and would drop that operation the same way,
        // and a second connect would only fail as busy. A security-key request
        // for a new account does the same, for the same reasons.
        let device_busy =
            connecting || self.trezor_operation_in_flight() || self.fido2_operation_in_flight();

        let single_sig_convention = SingleSigConvention::new(self.import_from_v1);

        // The buttons name only the method: the create / import toggle above
        // already says which one they do.
        ui.add_enabled_ui(!device_busy, |ui| {
            if ui
                .add(ghost_button(&self.colors, keychain::short_name(), btn_size))
                .clicked()
            {
                let v = self.new_wallet_variant;
                if is_import {
                    self.import_seed_phrase_with_keychain(v, single_sig_convention);
                } else {
                    self.create_wallet_with_keychain(v);
                }
                self.wallet_modal = WalletModal::None;
                self.import_from_v1 = false;
            }

            // Unlike the buttons around it, this one and the Trezor button
            // leave the dialog open while the device works: success closes
            // it by switching to the new wallet, while a failure leaves it
            // open with the inputs kept for a retry, and reports the error
            // in the status line.
            ui.add_space(6.0);
            if ui
                .add(ghost_button(&self.colors, "FIDO2", btn_size))
                .clicked()
            {
                let v = self.new_wallet_variant;
                if is_import {
                    self.start_import_seed_phrase_with_fido2(v, single_sig_convention);
                } else {
                    self.start_create_wallet_with_fido2(v);
                }
            }

            ui.add_space(6.0);
            if ui
                .add(ghost_button(&self.colors, "Password", btn_size))
                .clicked()
            {
                let v = self.new_wallet_variant;
                if is_import {
                    self.import_seed_phrase_with_password(v, single_sig_convention);
                } else {
                    self.create_wallet_with_password(v);
                }
                self.wallet_modal = WalletModal::None;
                self.import_from_v1 = false;
            }

            // A Trezor holds its own seed, so it has nothing to restore from a
            // phrase and is offered only when creating. The device checks the
            // chosen parameter set against its seed and names the ones that fit
            // if it doesn't. Like FIDO2, it leaves the dialog open.
            if !is_import {
                ui.add_space(6.0);
                if ui
                    .add(ghost_button(&self.colors, "Trezor", btn_size))
                    .clicked()
                {
                    self.start_create_trezor_watch_only_wallet(self.new_wallet_variant, 1);
                }
            }
        });
        // Outside the group so it isn't greyed out with the buttons.
        if self.trezor_connect_rx.is_some() {
            ui.add_space(8.0);
            device_waiting_line(ui, &self.colors, TREZOR_WAITING_TEXT);
        } else if self.fido2_connect_rx.is_some() {
            ui.add_space(8.0);
            fido2_waiting_line(ui, &self.colors);
        }

        picked
    }
}
