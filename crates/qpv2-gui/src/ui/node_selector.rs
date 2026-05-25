//! Node selector popup rendering.

use ckb_node::NodeType;
use eframe::egui;

use crate::App;

impl App {
	pub(crate) fn show_node_selector_popup(&mut self, ctx: &egui::Context) {
		if !self.node_selector_open {
			return;
		}

		let Some(selector_rect) = self.node_selector_rect else {
			return;
		};

		let dropdown_pos = egui::pos2(selector_rect.left(), selector_rect.bottom() + 4.0);

		let area_response = egui::Area::new(egui::Id::new("node_selector_dropdown"))
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

						let row_defs = [
							(
								NodeType::FullNode,
								"\u{1F5A5}",
								"Full Node",
								"FULL",
								self.colors.accent_tint,
								self.colors.accent,
							),
							(
								NodeType::LightClient,
								"\u{1F4A1}",
								"Light Client",
								"LIGHT",
								self.colors.accent2_tint,
								self.colors.accent2,
							),
							(
								NodeType::PublicRpc,
								"\u{1F310}",
								"Remote RPC",
								"RPC",
								self.colors.warn_tint,
								self.colors.warn,
							),
						];

						let current_type = self.qp_client.config().node_type;

						for (ntype, icon, name, badge_text, badge_fill, accent_color) in row_defs
						{
							let selected = current_type == ntype;
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
										ui.label(
											egui::RichText::new(icon)
												.size(14.0)
												.color(accent_color),
										);
										ui.add_space(6.0);
										let name_color = if selected {
											self.colors.accent
										} else {
											self.colors.text
										};
										ui.label(
											egui::RichText::new(name)
												.size(12.5)
												.color(name_color),
										);
										ui.with_layout(
											egui::Layout::right_to_left(egui::Align::Center),
											|ui| {
												egui::Frame::new()
													.fill(badge_fill)
													.corner_radius(4.0)
													.inner_margin(egui::Margin::symmetric(6, 1))
													.show(ui, |ui| {
														ui.label(
															egui::RichText::new(badge_text)
																.size(8.5)
																.family(
																	egui::FontFamily::Monospace,
																)
																.color(accent_color),
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
								self.node_selector_open = false;
								self.switch_to_backend(ntype);
							}
						}
					});
			});

		if ctx.input(|i| i.pointer.any_click()) {
			let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
			if let Some(pos) = pointer_pos {
				let dropdown_rect = area_response.response.rect;
				if !dropdown_rect.contains(pos) && !selector_rect.contains(pos) {
					self.node_selector_open = false;
				}
			}
		}
	}
}
