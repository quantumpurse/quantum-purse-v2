//! Governance module: bind this wallet's addresses to a DAO v2 account, and
//! the address picker that confirms which ones.

use eframe::egui;

use super::utils::{
    accent_button, data_row, device_waiting_line, ghost_button, panel_frame, section_header,
    truncate_middle, unsaved_file_row, FIDO2_WAITING_TEXT,
};
use crate::types::{display_font, label_font};
use crate::App;

impl App {
    pub(crate) fn show_governance_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 24.0);

                ui.label(
                    egui::RichText::new("GOVERNANCE")
                        .font(display_font(16.0))
                        .color(self.colors.text),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "Bind this wallet's addresses to your DAO v2 account, so their Nervos \
                         DAO deposits count as your voting power.",
                    )
                    .size(11.0)
                    .color(self.colors.text_muted),
                );
                ui.add_space(16.0);

                panel_frame(&self.colors).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section_header(ui, &self.colors, "01", "Address Binding");
                    ui.add_space(10.0);
                    self.draw_binding_form(ui);
                    self.draw_receipt_row(ui);
                });
            });
        });
    }

    fn draw_binding_form(&mut self, ui: &mut egui::Ui) {
        if let Some(reason) = self.governance_blocker() {
            ui.label(
                egui::RichText::new(reason)
                    .size(11.5)
                    .color(self.colors.danger),
            );
            return;
        }
        let busy = self.governance.busy() || self.governance.picker.is_some();

        field_label(ui, &self.colors.text_muted, "DAO SERVER");
        ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut self.governance.server_url)
                .desired_width(ui.available_width())
                .font(egui::FontId::monospace(11.5)),
        );
        ui.add_space(10.0);

        field_label(ui, &self.colors.text_muted, "API KEY");
        ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut self.governance.api_key)
                .password(true)
                .hint_text("dao_…")
                .desired_width(ui.available_width())
                .font(egui::FontId::monospace(11.5)),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Generate a binding API key on the DAO website. It is valid for 60 seconds \
                 and works for one binding.",
            )
            .size(11.0)
            .color(self.colors.text_muted),
        );
        ui.add_space(12.0);

        // A new binding's receipt would replace an unsaved one, so the last
        // receipt must be saved first.
        let receipt_unsaved = self.governance.unsaved_receipt.is_some();
        let can_start = !busy && !receipt_unsaved && !self.governance.api_key.trim().is_empty();
        if ui
            .add_enabled(
                can_start,
                accent_button(&self.colors, "Start", egui::vec2(140.0, 30.0)),
            )
            .clicked()
        {
            self.start_governance_binding();
        }
        if receipt_unsaved {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Save your last binding receipt before binding again.")
                    .size(11.0)
                    .color(self.colors.text_muted),
            );
        }
        if self.governance.prepare_rx.is_some() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Reading your DAO account…")
                    .font(label_font(10.0))
                    .color(self.colors.text_muted),
            );
        }
    }

    /// The last binding's receipt: a Save row while it is unsaved, its path
    /// once saved. Drawn whatever the form shows, because an unsaved receipt
    /// outlives a switch to a wallet that cannot bind.
    fn draw_receipt_row(&mut self, ui: &mut egui::Ui) {
        if self.governance.unsaved_receipt.is_some() {
            ui.add_space(12.0);
            if unsaved_file_row(ui, &self.colors, "binding receipt") {
                self.save_governance_receipt("", false);
            }
        } else if let Some(receipt) = &self.governance.last_receipt {
            ui.add_space(12.0);
            data_row(
                ui,
                &self.colors,
                "Last receipt",
                &receipt.display().to_string(),
            );
        }
    }

    /// The address picker, drawn over everything while a binding is open.
    pub(crate) fn show_governance_modal(&mut self, ctx: &egui::Context) {
        if self.governance.picker.is_none() {
            return;
        }
        let binding = self.governance.bind_rx.is_some();

        let screen_rect = ctx.input(|i| i.viewport_rect());
        let backdrop_clicked = egui::Area::new(egui::Id::new("governance_backdrop"))
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

        let modal_width = 520.0;
        let modal_pos = egui::pos2(
            (screen_rect.width() - modal_width) / 2.0,
            screen_rect.height() * 0.12,
        );

        let mut confirm = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("governance_area"))
            .fixed_pos(modal_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border2))
                    .inner_margin(egui::Margin::symmetric(24, 20))
                    .show(ui, |ui| {
                        ui.set_width(modal_width);
                        let colors = &self.colors;
                        let Some(picker) = &mut self.governance.picker else {
                            return;
                        };

                        ui.label(
                            egui::RichText::new("CONFIRM ADDRESS BINDING")
                                .font(display_font(16.0))
                                .color(colors.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(
                                "Each selected address signs a challenge proving you own it.",
                            )
                            .size(11.0)
                            .color(colors.text_muted),
                        );
                        ui.add_space(14.0);

                        let account = &picker.prepared.account;
                        let none = "—".to_string();
                        data_row(
                            ui,
                            colors,
                            "Username",
                            account.username.as_ref().unwrap_or(&none),
                        );
                        data_row(ui, colors, "Email", account.email.as_ref().unwrap_or(&none));
                        data_row(ui, colors, "User ID", &account.user_id);
                        ui.add_space(12.0);

                        let total = picker.candidates.len();
                        let selected = picker.candidates.iter().filter(|c| c.selected).count();
                        ui.horizontal(|ui| {
                            field_label(
                                ui,
                                &colors.text_muted,
                                &format!("ADDRESSES ({} OF {})", selected, total),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let all = selected == total;
                                    let text = if all { "Deselect All" } else { "Select All" };
                                    if ui
                                        .add_enabled(
                                            !binding,
                                            ghost_button(colors, text, egui::vec2(110.0, 22.0)),
                                        )
                                        .clicked()
                                    {
                                        for c in &mut picker.candidates {
                                            c.selected = !all;
                                        }
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                        for candidate in &mut picker.candidates {
                            ui.add_enabled(
                                !binding,
                                egui::Checkbox::new(
                                    &mut candidate.selected,
                                    egui::RichText::new(truncate_middle(
                                        &candidate.address,
                                        16,
                                        10,
                                    ))
                                    .size(11.5)
                                    .color(colors.text),
                                ),
                            )
                            .on_hover_text(&candidate.address);
                        }
                        ui.add_space(14.0);

                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !binding,
                                    ghost_button(colors, "Cancel", egui::vec2(100.0, 30.0)),
                                )
                                .clicked()
                            {
                                cancel = true;
                            }
                            let label = format!("Confirm & Sign ({})", selected);
                            if ui
                                .add_enabled(
                                    !binding && selected > 0,
                                    accent_button(colors, &label, egui::vec2(200.0, 30.0)),
                                )
                                .clicked()
                            {
                                confirm = true;
                            }
                        });
                        if binding {
                            ui.add_space(10.0);
                            let text = if self.governance.bind_uses_fido2 {
                                FIDO2_WAITING_TEXT
                            } else {
                                "Checking the server's event, signing and binding…"
                            };
                            device_waiting_line(ui, colors, text);
                        }
                    });
            });

        if confirm {
            self.confirm_governance_binding();
        } else if cancel || (backdrop_clicked && !binding) {
            self.cancel_governance_binding();
        }
    }
}

fn field_label(ui: &mut egui::Ui, color: &egui::Color32, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(label_font(9.5))
            .color(*color),
    );
}
