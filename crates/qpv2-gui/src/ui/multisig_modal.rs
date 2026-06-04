//! Modal for creating a multisig account.

use eframe::egui;
use qpv2_core::types::SpxVariant;

use crate::App;

impl App {
	pub(crate) fn show_multisig_modal(&mut self, ctx: &egui::Context) {
		if !self.multisig_modal_open {
			return;
		}

		// Semi-transparent backdrop.
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

		let modal_width = 480.0;
		let modal_pos = egui::pos2(
			(screen_rect.width() - modal_width) / 2.0,
			screen_rect.height() * 0.12,
		);

		egui::Area::new(egui::Id::new("multisig_modal_area"))
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

						ui.label(
							egui::RichText::new("Create Multi-sig Account")
								.size(20.0)
								.strong()
								.color(self.colors.text),
						);
						ui.add_space(16.0);

						// Threshold (M)
						ui.horizontal(|ui| {
							ui.label(
								egui::RichText::new("Threshold (M)")
									.size(12.0)
									.color(self.colors.text_muted),
							);
							ui.add(
								egui::DragValue::new(&mut self.multisig_threshold)
									.range(1..=255u8),
							);
						});
						ui.add_space(8.0);

						// Required first N (R)
						ui.horizontal(|ui| {
							ui.label(
								egui::RichText::new("Required first N (R)")
									.size(12.0)
									.color(self.colors.text_muted),
							);
							ui.add(
								egui::DragValue::new(&mut self.multisig_required_first_n)
									.range(0..=self.multisig_threshold),
							);
						});
						ui.add_space(16.0);

						// Co-signers list
						ui.label(
							egui::RichText::new("Co-signers")
								.size(13.0)
								.strong()
								.color(self.colors.text),
						);
						ui.add_space(6.0);

						let mut remove_index: Option<usize> = None;
						for (i, (pubkey_hex, variant)) in
							self.multisig_co_signers.iter_mut().enumerate()
						{
							egui::Frame::new()
								.fill(self.colors.surface2)
								.corner_radius(10.0)
								.inner_margin(egui::Margin::symmetric(12, 10))
								.show(ui, |ui| {
									ui.horizontal(|ui| {
										ui.label(
											egui::RichText::new("Variant")
												.size(11.0)
												.color(self.colors.text_muted),
										);
										egui::ComboBox::from_id_salt(("ms_variant", i))
											.selected_text(format!("{}", variant))
											.width(130.0)
											.show_ui(ui, |ui| {
												for v in ALL_VARIANTS {
													ui.selectable_value(
														variant,
														*v,
														format!("{}", v),
													);
												}
											});
										ui.with_layout(
											egui::Layout::right_to_left(egui::Align::Center),
											|ui| {
												if ui.small_button("\u{2715}").clicked() {
													remove_index = Some(i);
												}
											},
										);
									});
									ui.add_space(4.0);
									ui.label(
										egui::RichText::new("Public key (hex)")
											.size(11.0)
											.color(self.colors.text_muted),
									);
									ui.add(
										egui::TextEdit::multiline(pubkey_hex)
											.desired_width(modal_width - 24.0)
											.desired_rows(2)
											.font(egui::TextStyle::Monospace),
									);
								});
							ui.add_space(6.0);
						}

						if let Some(idx) = remove_index {
							self.multisig_co_signers.remove(idx);
						}

						if ui
							.button(
								egui::RichText::new("+ Add Co-signer")
									.size(12.0)
									.color(self.colors.accent2),
							)
							.clicked()
						{
							self.multisig_co_signers
								.push((String::new(), SpxVariant::Sha2128S));
						}

						ui.add_space(20.0);

						// Buttons
						ui.horizontal(|ui| {
							ui.with_layout(
								egui::Layout::right_to_left(egui::Align::Center),
								|ui| {
									if ui.button("Create").clicked() {
										self.multisig_modal_open = false;
										self.create_multisig_account();
									}
									if ui.button("Cancel").clicked() {
										self.multisig_modal_open = false;
									}
								},
							);
						});
					});
			});

		if backdrop_clicked {
			self.multisig_modal_open = false;
		}
	}
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
