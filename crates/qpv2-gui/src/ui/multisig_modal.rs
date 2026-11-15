//! Modal for creating a multisig account, in the Flight Deck idiom: the
//! threshold is read out large at the top, beside the required count and a
//! line naming who that count requires; below, one row per signer, whose
//! label fills in when that signer must always sign.

use std::ops::RangeInclusive;

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use qpv2_core::types::SpxVariant;

use crate::types::{display_font, label_font, AppColors};
use crate::ui::utils::{accent_button, ghost_button, pin_popup_above_modal};
use crate::App;

/// The dialog's content width.
const MODAL_W: f32 = 560.0;
/// The parameter set a new signer row starts on.
pub(crate) const NEW_SIGNER_VARIANT: SpxVariant = SpxVariant::Sha2128F;
/// Every signer row has this height, so the list's height is known before
/// it is drawn.
const ROW_H: f32 = 34.0;
/// Past this many rows the list scrolls. A short window scrolls it sooner.
const LIST_MAX_ROWS: usize = 8;
/// The side of each move and remove button at the end of a row.
const ACTION_W: f32 = 22.0;
/// The gap between those buttons.
const ACTION_GAP: f32 = 2.0;

impl App {
    pub(crate) fn show_multisig_modal(&mut self, ctx: &egui::Context) {
        if !self.multisig_modal_open {
            return;
        }

        // Semi-transparent backdrop that consumes clicks.
        let screen_rect = ctx.input(|i| i.viewport_rect());
        let backdrop_clicked = egui::Area::new(egui::Id::new("multisig_modal_backdrop"))
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
            (screen_rect.width() - MODAL_W) / 2.0,
            screen_rect.height() * 0.1,
        );
        let margin = egui::Margin::symmetric(20, 18);

        // Only the signer list scrolls, so the title and the buttons always
        // show. The list gets what the window has left once the rest of the
        // dialog is drawn, measured on the last frame, so the dialog ends as
        // far from the bottom as it starts from the top. Zooming in shrinks
        // the window and grows the rest, and the list gives way to both.
        let rest_id = egui::Id::new("multisig_modal_rest_height");
        let rest_h = ctx
            .memory(|m| m.data.get_temp::<f32>(rest_id))
            .unwrap_or(0.0);
        let list_cap = (screen_rect.height() * 0.8 - rest_h)
            .min(LIST_MAX_ROWS as f32 * ROW_H)
            .max(2.0 * ROW_H);

        let shown = egui::Area::new(egui::Id::new("multisig_modal_area"))
            .fixed_pos(modal_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.colors.surface)
                    .stroke(egui::Stroke::new(1.0, self.colors.border2))
                    .inner_margin(margin)
                    .show(ui, |ui| {
                        ui.set_width(MODAL_W);
                        self.multisig_modal_contents(ui, list_cap)
                    })
                    .inner
            });
        let rest = shown.response.rect.height() - shown.inner;
        ctx.memory_mut(|m| m.data.insert_temp(rest_id, rest));

        if backdrop_clicked {
            self.multisig_modal_open = false;
        }
    }

    /// Draws the dialog's contents, giving the signer list at most
    /// `list_cap` points of height. Returns the height the list took.
    fn multisig_modal_contents(&mut self, ui: &mut egui::Ui, list_cap: f32) -> f32 {
        let c_accent = self.colors.accent;
        let c_text = self.colors.text;
        let c_muted = self.colors.text_muted;
        let c_border = self.colors.border;

        // The required count never exceeds the threshold, so lowering the
        // threshold takes it down too.
        self.multisig_required_first_n =
            self.multisig_required_first_n.min(self.multisig_threshold);
        let threshold = self.multisig_threshold;
        let signer_count = self.multisig_signers.len();

        // ── Title row + hairline ──
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("NEW MULTISIG")
                    .font(label_font(11.0))
                    .color(c_accent),
            );
            ui.label(
                egui::RichText::new("// CONFIGURE")
                    .font(label_font(11.0))
                    .color(c_muted),
            );
        });
        ui.add_space(8.0);
        hairline(ui, c_border);
        ui.add_space(16.0);

        // ── The policy: the threshold read out large, then the required
        // count and what the two mean for these signers. ──
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let mut figure = LayoutJob::default();
                    figure.append(
                        &threshold.to_string(),
                        0.0,
                        TextFormat {
                            font_id: display_font(46.0),
                            color: c_accent,
                            ..Default::default()
                        },
                    );
                    figure.append(
                        &format!("/{}", signer_count),
                        4.0,
                        TextFormat {
                            font_id: display_font(24.0),
                            color: c_muted,
                            ..Default::default()
                        },
                    );
                    ui.label(figure);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = ACTION_GAP;
                        let raise = step_button(&self.colors, "+");
                        if ui.add_enabled(threshold < u8::MAX, raise).clicked() {
                            self.multisig_threshold += 1;
                        }
                        let lower = step_button(&self.colors, "\u{2212}");
                        if ui.add_enabled(threshold > 1, lower).clicked() {
                            self.multisig_threshold -= 1;
                        }
                    });
                });
                ui.label(
                    egui::RichText::new("THRESHOLD (M) \u{b7} SIGNATURES TO SPEND")
                        .font(label_font(9.5))
                        .color(c_muted),
                );
            });
            ui.add_space(28.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("REQUIRED FIRST N (R)")
                        .font(label_font(9.5))
                        .color(c_muted),
                );
                ui.add_space(4.0);
                stepper(
                    ui,
                    &self.colors,
                    &mut self.multisig_required_first_n,
                    0..=threshold,
                );
                let pieces =
                    policy_sentence(threshold, self.multisig_required_first_n, signer_count);
                if !pieces.is_empty() {
                    ui.add_space(8.0);
                    ui.label(policy_line(&pieces, &self.colors, ui.available_width()));
                }
            });
        });
        ui.add_space(16.0);
        hairline(ui, c_border);
        ui.add_space(12.0);

        // ── The signers ──
        // Every signer is a key, your own included: paste it from the
        // Accounts tab. The row order is the signers' order.
        ui.label(
            egui::RichText::new("SIGNERS")
                .font(label_font(9.5))
                .color(c_muted),
        );
        ui.add_space(6.0);

        // The required count counts from the top, so the first rows are the
        // ones it means.
        let required = usize::from(self.multisig_required_first_n).min(signer_count);
        // A multisig needs at least two signers, so the last two rows can't
        // be removed.
        let can_remove = signer_count > 2;
        let mut remove_index: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;

        let list_h = (signer_count as f32 * ROW_H).min(list_cap);
        egui::Frame::new()
            .fill(self.colors.surface2)
            .stroke(egui::Stroke::new(1.0, c_border))
            .show(ui, |ui| {
                // A floating bar would cover the remove buttons at the end
                // of each row. A solid one takes its own room, and only when
                // the list scrolls.
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::vertical()
                    .id_salt("multisig_signers")
                    .max_height(list_h)
                    // An area offers its contents only last frame's height,
                    // so the list claims its height outright; otherwise a
                    // new row would wait a frame for room.
                    .min_scrolled_height(list_h)
                    .auto_shrink([false, true])
                    // A row added at the bottom stays in view.
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, (pubkey_hex, variant)) in
                            self.multisig_signers.iter_mut().enumerate()
                        {
                            let (row, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Sense::hover(),
                            );
                            if i + 1 < signer_count {
                                ui.painter().hline(
                                    row.x_range(),
                                    row.bottom() - 0.5,
                                    egui::Stroke::new(1.0, c_border),
                                );
                            }
                            let mut ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt(("multisig_signer_row", i))
                                    .max_rect(row.shrink2(egui::vec2(8.0, 0.0)))
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            );

                            // The label the Multisig tab and every message
                            // give this signer, filled when it must sign.
                            signer_tag(&mut ui, &self.colors, i, i < required);

                            let variant_combo = egui::ComboBox::from_id_salt(("ms_variant", i))
                                .selected_text(
                                    egui::RichText::new(format!("{}", variant))
                                        .size(11.5)
                                        .color(c_text),
                                )
                                .width(112.0)
                                .show_ui(&mut ui, |ui| {
                                    for v in ALL_VARIANTS {
                                        let text = egui::RichText::new(format!("{}", v))
                                            .size(11.5)
                                            .color(if variant == v { c_accent } else { c_text });
                                        ui.selectable_value(variant, *v, text);
                                    }
                                });
                            pin_popup_above_modal(&ui, &variant_combo.response);

                            // The key takes whatever the buttons leave.
                            let actions_w = 3.0 * ACTION_W + 2.0 * ACTION_GAP;
                            let key_w =
                                ui.available_width() - ui.spacing().item_spacing.x - actions_w;
                            ui.add(
                                egui::TextEdit::singleline(pubkey_hex)
                                    .desired_width(key_w)
                                    .hint_text("Paste public key hex")
                                    .font(egui::FontId::monospace(11.5)),
                            );

                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing.x = ACTION_GAP;
                                // The end rows keep their dead arrow, dimmed,
                                // so the buttons line up down the list.
                                let colors = &self.colors;
                                if row_action(ui, colors, RowAction::Up, i > 0).clicked() {
                                    swap = Some((i - 1, i));
                                }
                                let down = i + 1 < signer_count;
                                if row_action(ui, colors, RowAction::Down, down).clicked() {
                                    swap = Some((i, i + 1));
                                }
                                if can_remove {
                                    if row_action(ui, colors, RowAction::Remove, true).clicked() {
                                        remove_index = Some(i);
                                    }
                                } else {
                                    ui.add_space(ACTION_W);
                                }
                            });
                        }
                    });
            });

        // A moved row takes the place it lands on, required or not: being
        // required belongs to the place, not to the key.
        if let Some((a, b)) = swap {
            self.multisig_signers.swap(a, b);
        }
        if let Some(idx) = remove_index {
            self.multisig_signers.remove(idx);
        }

        ui.add_space(4.0);
        let add = egui::Button::new(
            egui::RichText::new("+ ADD SIGNER")
                .font(label_font(10.5))
                .color(c_accent),
        )
        .frame(false);
        if ui.add(add).clicked() {
            self.multisig_signers
                .push((String::new(), NEW_SIGNER_VARIANT));
        }

        ui.add_space(12.0);
        hairline(ui, c_border);
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // One signer means a single-sig account, which this modal
                // must not create; core refuses it too. A threshold the
                // signers can't meet is refused here as well, and the line
                // under the numbers says why.
                let signers = self.multisig_signers.len();
                let can_create = signers >= 2 && usize::from(self.multisig_threshold) <= signers;
                if ui
                    .add_enabled(
                        can_create,
                        accent_button(&self.colors, "Create", egui::vec2(110.0, 30.0)),
                    )
                    .clicked()
                {
                    self.multisig_modal_open = false;
                    self.create_multisig_account();
                }
                ui.add_space(6.0);
                if ui
                    .add(ghost_button(&self.colors, "Cancel", egui::vec2(90.0, 30.0)))
                    .clicked()
                {
                    self.multisig_modal_open = false;
                }
            });
        });

        list_h
    }
}

/// A hairline across the full width.
fn hairline(ui: &mut egui::Ui, color: egui::Color32) {
    let (rule, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rule.x_range(),
        rule.center().y,
        egui::Stroke::new(1.0, color),
    );
}

/// A small square button for raising or lowering a number.
fn step_button(colors: &AppColors, text: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text)
            .font(egui::FontId::proportional(13.0))
            .color(colors.text_muted),
    )
    .fill(egui::Color32::TRANSPARENT)
    .stroke(egui::Stroke::new(1.0, colors.border2))
    .corner_radius(0.0)
    .min_size(egui::vec2(24.0, 22.0))
}

/// A number between a lower and a raise button, kept within `range`.
fn stepper(ui: &mut egui::Ui, colors: &AppColors, value: &mut u8, range: RangeInclusive<u8>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if ui
            .add_enabled(*value > *range.start(), step_button(colors, "\u{2212}"))
            .clicked()
        {
            *value -= 1;
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 22.0), egui::Sense::hover());
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, colors.border2),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            value.to_string(),
            egui::FontId::proportional(13.0),
            colors.text,
        );
        if ui
            .add_enabled(*value < *range.end(), step_button(colors, "+"))
            .clicked()
        {
            *value += 1;
        }
    });
}

/// A signer's label, S1 for the first: filled with the accent when the
/// policy makes this signer required, outlined otherwise.
fn signer_tag(ui: &mut egui::Ui, colors: &AppColors, index: usize, required: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 22.0), egui::Sense::hover());
    let text_color = if required {
        ui.painter().rect_filled(rect, 0.0, colors.accent);
        colors.bg
    } else {
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, colors.border2),
            egui::StrokeKind::Inside,
        );
        colors.text_muted
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("S{}", index + 1),
        display_font(10.5),
        text_color,
    );
}

#[derive(Clone, Copy)]
enum RowAction {
    Up,
    Down,
    Remove,
}

/// One of the frameless buttons at the end of a signer row. The icons are
/// painted, so they don't depend on the font. A disabled one is dimmed to
/// the border's colour; the hovered remove button turns red.
fn row_action(
    ui: &mut egui::Ui,
    colors: &AppColors,
    action: RowAction,
    enabled: bool,
) -> egui::Response {
    let button = egui::Button::new("")
        .frame(false)
        .min_size(egui::vec2(ACTION_W, ACTION_W));
    let response = ui.add_enabled(enabled, button);

    let color = match (enabled, response.hovered(), action) {
        (false, _, _) => colors.border2,
        (true, true, RowAction::Remove) => colors.danger,
        (true, true, _) => colors.text,
        (true, false, _) => colors.text_muted,
    };
    let c = response.rect.center();
    let painter = ui.painter();
    match action {
        RowAction::Up | RowAction::Down => {
            let tip = if matches!(action, RowAction::Up) {
                -3.5
            } else {
                3.5
            };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x, c.y + tip),
                    egui::pos2(c.x - 4.0, c.y - tip),
                    egui::pos2(c.x + 4.0, c.y - tip),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        RowAction::Remove => {
            let stroke = egui::Stroke::new(1.5, color);
            let d = 3.5;
            painter.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], stroke);
            painter.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], stroke);
        }
    }
    response
}

/// How a piece of the policy line is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Plain,
    /// A signer's label, coloured like the filled labels on their rows.
    Signer,
    Warn,
    Error,
}

/// The line under the required count, as its pieces: who that count makes
/// required, named by the labels their rows carry. The figure already says
/// how many signatures are needed, so the line doesn't repeat it. It also
/// warns when the required signers leave the others nothing to do, and
/// says so when the threshold can't be met. With nobody required and
/// nothing wrong, there is no line.
fn policy_sentence(m: u8, r: u8, n: usize) -> Vec<(String, Tone)> {
    if usize::from(m) > n {
        let missing = usize::from(m) - n;
        let add = if missing == 1 {
            "Add 1 more signer".to_string()
        } else {
            format!("Add {} more signers", missing)
        };
        return vec![(
            format!(
                "The threshold is {}, but only {} signers are listed. {}, or lower the threshold.",
                m, n, add
            ),
            Tone::Error,
        )];
    }
    if r == 0 {
        return Vec::new();
    }

    let mut pieces = required_names(r);
    pieces.push((" must sign.".to_string(), Tone::Plain));
    // The required signers alone meet the threshold, so no other signature
    // can ever count.
    let others = n - usize::from(r);
    if r == m && others > 0 {
        let warning = if others == 1 {
            " The other signer is never needed."
        } else {
            " The other signers are never needed."
        };
        pieces.push((warning.to_string(), Tone::Warn));
    }
    pieces
}

/// The first `r` signers by their labels: "S1", "S1 and S2", "S1 to S3".
fn required_names(r: u8) -> Vec<(String, Tone)> {
    let label = |i: u8| (format!("S{}", i), Tone::Signer);
    match r {
        1 => vec![label(1)],
        2 => vec![label(1), (" and ".to_string(), Tone::Plain), label(2)],
        _ => vec![label(1), (" to ".to_string(), Tone::Plain), label(r)],
    }
}

/// The policy line, wrapped to `width`.
fn policy_line(pieces: &[(String, Tone)], colors: &AppColors, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    for (text, tone) in pieces {
        let color = match tone {
            Tone::Plain => colors.text_muted,
            Tone::Signer => colors.accent,
            Tone::Warn => colors.warn,
            Tone::Error => colors.danger,
        };
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color,
                ..Default::default()
            },
        );
    }
    job
}

const ALL_VARIANTS: &[SpxVariant] = &[
    SpxVariant::Sha2128F,
    SpxVariant::Sha2128S,
    SpxVariant::Sha2192F,
    SpxVariant::Sha2192S,
    SpxVariant::Sha2256F,
    SpxVariant::Sha2256S,
    SpxVariant::Shake128F,
    SpxVariant::Shake128S,
    SpxVariant::Shake192F,
    SpxVariant::Shake192S,
    SpxVariant::Shake256F,
    SpxVariant::Shake256S,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn text(m: u8, r: u8, n: usize) -> String {
        policy_sentence(m, r, n)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    /// The figure already says how many signatures are needed, so with
    /// nobody required there is nothing to add.
    #[test]
    fn nobody_required_has_no_line() {
        assert!(policy_sentence(2, 0, 3).is_empty());
    }

    /// The required signers are named by the labels on their rows, and
    /// coloured like those labels.
    #[test]
    fn required_signers_are_named_by_their_labels() {
        assert_eq!(text(2, 1, 3), "S1 must sign.");
        assert_eq!(text(3, 2, 4), "S1 and S2 must sign.");
        assert_eq!(text(4, 3, 5), "S1 to S3 must sign.");
        assert!(policy_sentence(2, 1, 3).contains(&("S1".to_string(), Tone::Signer)));
    }

    /// When the required signers alone meet the threshold, no other
    /// signature can ever count, so the line warns about it.
    #[test]
    fn required_count_at_the_threshold_warns_the_others_are_never_needed() {
        assert_eq!(
            text(1, 1, 3),
            "S1 must sign. The other signers are never needed."
        );
        assert_eq!(
            text(2, 2, 3),
            "S1 and S2 must sign. The other signer is never needed."
        );
        assert_eq!(policy_sentence(2, 2, 3).last().unwrap().1, Tone::Warn);
    }

    /// With every signer required there is no one else to warn about.
    #[test]
    fn every_signer_required_has_no_warning() {
        assert_eq!(text(3, 3, 3), "S1 to S3 must sign.");
    }

    /// The error says how to clear it, with the number of signers missing.
    #[test]
    fn threshold_above_the_signer_count_is_an_error() {
        assert_eq!(
            policy_sentence(4, 1, 3),
            vec![(
                "The threshold is 4, but only 3 signers are listed. \
                 Add 1 more signer, or lower the threshold."
                    .to_string(),
                Tone::Error
            )]
        );
        assert_eq!(
            text(5, 0, 3),
            "The threshold is 5, but only 3 signers are listed. \
             Add 2 more signers, or lower the threshold."
        );
    }
}
