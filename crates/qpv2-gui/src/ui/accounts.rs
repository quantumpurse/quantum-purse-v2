//! Accounts tab rendering — single-sig accounts only.

use eframe::egui;
use qpv2_core::types::AuthMethod;
use qpv2_core::KeyVault;

use super::utils::{paint_corner_accent, CardHover};
use crate::types::Status;
use crate::utils::format_ckb_balance;
use crate::App;

impl App {
	pub(crate) fn show_accounts_tab(&mut self, ui: &mut egui::Ui) {
		ui.horizontal(|ui| {
			ui.add_space(30.0);
			ui.vertical(|ui| {
				ui.set_width(ui.available_width() - 30.0);

				ui.heading(
					egui::RichText::new("Accounts")
						.size(26.0)
						.strong()
						.color(self.colors.text),
				);
				ui.label(
					egui::RichText::new("Derive and manage single-sig accounts for the active wallet.")
						.size(13.0)
						.color(self.colors.text_muted),
				);

				ui.add_space(22.0);

				// ── New Account card ──
				let hover = CardHover::new(ui, "acct-single", &self.colors);

				let single_card = egui::Frame::new()
					.fill(hover.fill)
					.corner_radius(18.0)
					.inner_margin(egui::Margin::symmetric(20, 24))
					.stroke(hover.stroke)
					.show(ui, |ui| {
						ui.vertical_centered(|ui| {
							hover.apply_lift(ui);
							ui.label(egui::RichText::new("\u{2726}").size(26.0));
							ui.add_space(6.0);
							ui.label(
								egui::RichText::new("Create Account")
									.size(14.0)
									.strong()
									.color(self.colors.text),
							);
							ui.add_space(4.0);
							ui.label(
								egui::RichText::new(
									"Derive a new account from your wallet seed.",
								)
								.size(11.0)
								.color(self.colors.text_muted),
							);
						});
					})
					.response;

				paint_corner_accent(
					ui.painter(),
					single_card.rect,
					18.0,
					self.colors.accent,
				);
				hover.commit(&single_card);

				if single_card.interact(egui::Sense::click()).clicked() {
					self.create_singlesig_account();
				}

				ui.add_space(20.0);

				// ── Section title ──
				let pill =
					|ui: &mut egui::Ui, fill: egui::Color32, text: String, color: egui::Color32| {
						egui::Frame::new()
							.fill(fill)
							.corner_radius(10.0)
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

				let single_sig_accounts: Vec<_> = self
					.accounts
					.iter()
					.enumerate()
					.filter(|(_, a)| a.config.signers.len() == 1)
					.collect();

				ui.horizontal(|ui| {
					ui.label(
						egui::RichText::new("Single-sig Accounts")
							.size(15.0)
							.strong()
							.color(self.colors.text),
					);
					ui.add_space(10.0);
					pill(
						ui,
						self.colors.accent_tint,
						format!("{} total", single_sig_accounts.len()),
						self.colors.accent,
					);

					if let Ok(info) = KeyVault::read_wallet_info(self.wallet_id) {
						ui.add_space(6.0);
						pill(
							ui,
							self.colors.surface2,
							format!("SPHINCS+ {}", info.spx_variant),
							self.colors.text_muted,
						);
						ui.add_space(6.0);
						pill(
							ui,
							self.colors.accent2_tint,
							match info.auth_method {
								AuthMethod::Keychain => keychain::short_name().into(),
								AuthMethod::Password => "Password".into(),
								AuthMethod::Fido2 { .. } => "FIDO2 Key".into(),
							},
							self.colors.accent2,
						);
					}

					ui.add_space(10.0);
					self.show_status(ui);
				});

				ui.add_space(10.0);

				// ── Account list (single-sig only) ──
				if single_sig_accounts.is_empty() {
					ui.label(
						egui::RichText::new("No accounts yet. Create one to get started.")
							.color(self.colors.text_muted),
					);
				} else {
					let avatar_colors = [
						(self.colors.accent, egui::Color32::from_rgb(5, 12, 10)),
						(self.colors.accent3, egui::Color32::WHITE),
						(self.colors.warn, egui::Color32::from_rgb(5, 12, 10)),
					];

					for (i, account) in single_sig_accounts {
						let lock_args = &account.lock_args;
						let address_text = match crate::utils::lock_args_to_address(
							lock_args,
							self.qp_client.is_mainnet(),
						) {
							Ok(addr) => addr,
							Err(_) => format!("0x{}", lock_args),
						};

						let balance_text = match self.spendable_balances.get(lock_args) {
							Some(Some(shannons)) => format_ckb_balance(*shannons),
							Some(None) => "Loading...".to_string(),
							None => "--".to_string(),
						};

						let (av_bg, av_fg) = avatar_colors[i % avatar_colors.len()];

						let hover = CardHover::new(ui, ("acct-row", i), &self.colors);

						let row_resp = egui::Frame::new()
							.fill(hover.fill)
							.corner_radius(9.0)
							.inner_margin(egui::Margin::symmetric(18, 14))
							.stroke(hover.stroke)
							.show(ui, |ui| {
								ui.horizontal(|ui| {
									// Avatar
									let (avatar_rect, _) = ui.allocate_exact_size(
										egui::vec2(38.0, 38.0),
										egui::Sense::hover(),
									);
									let center = avatar_rect.center();
									let radius = 19.0;

									ui.painter().circle_filled(center, radius, av_bg);
									let letter = (b'A' + (i as u8 % 26)) as char;
									ui.painter().text(
										center,
										egui::Align2::CENTER_CENTER,
										letter.to_string(),
										egui::FontId::proportional(15.0),
										av_fg,
									);

									ui.add_space(10.0);

									// Info
									ui.vertical(|ui| {
										ui.horizontal(|ui| {
											ui.label(
												egui::RichText::new(format!("Account #{}", i))
													.size(13.0),
											);
											ui.label(
												egui::RichText::new(&balance_text)
													.size(13.0)
													.strong()
													.color(self.colors.text_muted)
													.family(egui::FontFamily::Monospace),
											);
										});
										ui.label(
											egui::RichText::new(address_text.clone())
												.size(9.0)
												.color(self.colors.text_muted)
												.family(egui::FontFamily::Monospace),
										);
										let signer = &account.config.signers[0];
										let pk_hex = hex::encode(&signer.pubkey);
										let pk_short = if pk_hex.len() > 40 {
											format!(
												"{}...{}",
												&pk_hex[..20],
												&pk_hex[pk_hex.len() - 20..]
											)
										} else {
											pk_hex
										};
										ui.label(
											egui::RichText::new(format!(
												"{} {}",
												signer.variant, pk_short
											))
											.size(9.0)
											.color(self.colors.accent)
											.family(egui::FontFamily::Monospace),
										);
									});

									// Copy buttons (right-aligned)
									ui.with_layout(
										egui::Layout::right_to_left(egui::Align::Center),
										|ui| {
											if ui
												.button("\u{1f4cb}")
												.on_hover_text("Copy address")
												.clicked()
											{
												ui.ctx().copy_text(address_text.clone());
												self.status =
													Status::Info("Address copied!".to_string());
											}

											let signer = &account.config.signers[0];
											let pubkey_text = hex::encode(&signer.pubkey);
											if ui
												.button("\u{1f511}")
												.on_hover_text("Copy public key")
												.clicked()
											{
												ui.ctx().copy_text(pubkey_text);
												self.status =
													Status::Info("Public key copied!".to_string());
											}
										},
									);
								});
							});

						hover.commit(&row_resp.response);

						ui.add_space(6.0);
					}
				}

				ui.add_space(20.0);
			}); // vertical
		}); // horizontal
	}
}
