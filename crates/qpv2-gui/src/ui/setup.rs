//! Setup and Locked screen rendering.

use eframe::egui;
use qpv2_core::types::SpxVariant;

use crate::App;

impl App {
    pub(crate) fn show_setup(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            // Logo
            ui.heading(
                egui::RichText::new("\u{1f52e} Quantum Purse")
                    .size(32.0)
                    .color(self.colors.accent)
                    .strong(),
            );

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Post-Quantum Secure Wallet")
                    .size(14.0)
                    .color(self.colors.text_muted),
            );

            ui.add_space(40.0);

            // Setup card
            egui::Frame::new()
                .fill(self.colors.surface2)
                .corner_radius(16.0)
                .inner_margin(32.0)
                .stroke(egui::Stroke::new(1.0, self.colors.border))
                .show(ui, |ui| {
                    ui.set_max_width(400.0);

                    ui.label(egui::RichText::new("Create New Wallet").size(20.0).strong());

                    ui.add_space(24.0);

                    ui.label("Select SPHINCS+ variant:");
                    ui.add_space(8.0);

                    egui::ComboBox::from_id_salt("variant")
                        .selected_text(format!("{}", self.selected_variant))
                        .width(350.0)
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
                                    &mut self.selected_variant,
                                    *variant,
                                    format!("{}", variant),
                                );
                            }
                        });

                    ui.add_space(32.0);

                    #[cfg(target_os = "macos")]
                    let is_busy = self.pending_op.is_some();
                    #[cfg(not(target_os = "macos"))]
                    let is_busy = false;

                    let button = egui::Button::new(
                        egui::RichText::new(if is_busy {
                            "Creating wallet..."
                        } else {
                            "Create with Touch ID"
                        })
                        .size(16.0),
                    )
                    .fill(self.colors.accent)
                    .min_size(egui::vec2(350.0, 48.0));

                    if ui.add_enabled(!is_busy, button).clicked() {
                        self.start_registration(frame);
                    }
                });

            ui.add_space(24.0);
            self.show_status(ui);
        });
    }

    pub(crate) fn show_locked(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);

            // Lock icon
            ui.label(egui::RichText::new("\u{1f512}").size(64.0));

            ui.add_space(24.0);

            ui.heading(
                egui::RichText::new("Wallet Locked")
                    .size(28.0)
                    .color(self.colors.text),
            );

            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("Authenticate to access your wallet")
                    .color(self.colors.text_muted),
            );

            ui.add_space(40.0);

            #[cfg(target_os = "macos")]
            let is_busy = self.pending_op.is_some();
            #[cfg(not(target_os = "macos"))]
            let is_busy = false;

            let button = egui::Button::new(
                egui::RichText::new(if is_busy {
                    "Waiting for Touch ID..."
                } else {
                    "Unlock with Touch ID"
                })
                .size(16.0),
            )
            .fill(self.colors.accent2)
            .min_size(egui::vec2(280.0, 48.0));

            if ui.add_enabled(!is_busy, button).clicked() {
                self.start_unlock(frame);
            }

            ui.add_space(24.0);
            self.show_status(ui);
        });
    }
}
