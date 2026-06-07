//! Co-signer coordination UI for multisig transactions.
//!
//! Rendered when `TransactionStatus::AwaitingCoSigners` is active.
//! Also provides a "Sign a Request" flow for the receiving co-signer.

use eframe::egui;

use crate::types::TransactionStatus;
use crate::App;

impl App {
	/// Render the co-signer coordination panel (initiator side).
	/// Shows the signing request, collected signatures, import field, and submit button.
	pub(crate) fn show_cosigner_panel(&mut self, ui: &mut egui::Ui) {
		let (request, sig_count, threshold) = match &self.tx_status {
			TransactionStatus::AwaitingCoSigners {
				request,
				signatures,
				..
			} => (
				request.clone(),
				signatures.len(),
				request.multisig_config.threshold as usize,
			),
			_ => return,
		};

		ui.add_space(16.0);
		ui.separator();
		ui.add_space(8.0);

		ui.label(
			egui::RichText::new("Multisig Signing")
				.size(16.0)
				.strong()
				.color(self.colors.text),
		);
		ui.add_space(4.0);

		ui.label(
			egui::RichText::new(format!(
				"{} of {} signatures collected.",
				sig_count, threshold
			))
			.size(13.0)
			.color(if sig_count >= threshold {
				self.colors.accent
			} else {
				self.colors.text_muted
			}),
		);

		ui.add_space(12.0);

		// ── Export signing request ──
		if ui.button("Copy Signing Request to Clipboard").clicked() {
			if let Ok(json) = serde_json::to_string_pretty(&request) {
				ui.ctx().copy_text(json);
				self.status = crate::types::Status::Info("Signing request copied!".to_string());
			}
		}

		ui.add_space(12.0);

		// ── Import co-signer response ──
		ui.label(
			egui::RichText::new("Import Co-signer Response")
				.size(13.0)
				.strong()
				.color(self.colors.text),
		);
		ui.add_space(4.0);

		if let TransactionStatus::AwaitingCoSigners {
			ref mut import_response_json, ..
		} = self.tx_status
		{
			ui.add(
				egui::TextEdit::multiline(import_response_json)
					.hint_text("Paste signing response JSON here...")
					.desired_width(ui.available_width())
					.desired_rows(3)
					.font(egui::TextStyle::Monospace),
			);
		}

		ui.add_space(4.0);

		let import_btn = egui::Button::new(
			egui::RichText::new("Import Response")
				.size(13.0)
				.strong()
				.color(self.colors.bg),
		)
		.fill(self.colors.accent2)
		.min_size(egui::vec2(160.0, 32.0));

		if ui.add(import_btn).clicked() {
			self.import_cosigner_response();
		}

		// Show inline import error/success
		self.show_status(ui);

		ui.add_space(12.0);

		// Re-read current signature count (may have changed after import).
		let (current_sig_count, current_threshold) = match &self.tx_status {
			TransactionStatus::AwaitingCoSigners {
				request,
				signatures,
				..
			} => (
				signatures.len(),
				request.multisig_config.threshold as usize,
			),
			_ => return,
		};

		// ── Submit button (enabled when threshold met) ──
		let can_submit = current_sig_count >= current_threshold;
		let submit_fill = if can_submit {
			self.colors.accent
		} else {
			self.colors.surface2
		};
		let submit_label = if can_submit {
			"Submit Transaction".to_string()
		} else {
			format!(
				"Waiting for signatures... ({}/{})",
				current_sig_count, current_threshold
			)
		};
		let submit_btn = egui::Button::new(
			egui::RichText::new(submit_label)
			.size(15.0)
			.strong()
			.color(self.colors.bg),
		)
		.fill(submit_fill)
		.min_size(egui::vec2(ui.available_width(), 44.0));

		if ui.add_enabled(can_submit, submit_btn).clicked() {
			self.submit_multisig_transaction();
		}

		ui.add_space(8.0);

		// ── Cancel ──
		if ui.button("Cancel").clicked() {
			self.tx_status = TransactionStatus::Idle;
		}
	}

	/// Parse and validate the import buffer as a `SigningResponse`, then add
	/// the signature to the collected set.
	fn import_cosigner_response(&mut self) {
		tracing::info!("Import cosigner response triggered.");

		let (request_msg, n_signers, signatures, import_response_json) = match &mut self.tx_status {
			TransactionStatus::AwaitingCoSigners {
				request,
				signatures,
				import_response_json,
				..
			} => (request.signing_message.clone(), request.multisig_config.signers.len(), signatures, import_response_json),
			_ => {
				tracing::error!("import_cosigner_response: not in AwaitingCoSigners state.");
				return;
			}
		};

		let buf = import_response_json.trim().to_string();
		tracing::info!("Import buffer length: {} chars.", buf.len());
		if buf.is_empty() {
			self.status = crate::types::Status::Error("Paste a response first.".to_string());
			return;
		}

		let response: qpv2_core::types::SigningResponse = match serde_json::from_str(&buf) {
			Ok(r) => r,
			Err(e) => {
				self.status =
					crate::types::Status::Error(format!("Invalid response JSON: {}", e));
				return;
			}
		};

		if response.signing_message != request_msg {
			self.status =
				crate::types::Status::Error("Response signing message does not match.".to_string());
			return;
		}

		if response.signer_index >= n_signers {
			self.status = crate::types::Status::Error(format!(
				"Signer index {} out of range (N={}).",
				response.signer_index, n_signers
			));
			return;
		}

		if signatures.iter().any(|(idx, _)| *idx == response.signer_index) {
			self.status = crate::types::Status::Error(format!(
				"Signer {} already provided a signature.",
				response.signer_index
			));
			return;
		}

		let sig_bytes = match hex::decode(&response.signature) {
			Ok(b) => b,
			Err(e) => {
				self.status =
					crate::types::Status::Error(format!("Invalid signature hex: {}", e));
				return;
			}
		};

		signatures.push((response.signer_index, sig_bytes));
		import_response_json.clear();
		self.status = crate::types::Status::Info(format!(
			"Signature from signer {} imported.",
			response.signer_index
		));
	}

	/// Co-signer signing flow: paste a signing request, verify details,
	/// sign with a local key, and copy the response.
	pub(crate) fn show_sign_request_ui(&mut self, ui: &mut egui::Ui) {
		ui.add_space(16.0);
		ui.separator();
		ui.add_space(8.0);

		ui.label(
			egui::RichText::new("Sign a Request")
				.size(16.0)
				.strong()
				.color(self.colors.text),
		);
		ui.add_space(4.0);
		ui.label(
			egui::RichText::new("Paste a signing request from another party to co-sign.")
				.size(12.0)
				.color(self.colors.text_muted),
		);
		ui.add_space(8.0);

		// ── Show completed response if available ──
		if self.cosign_response_json.is_some() {
			let response_copy = self.cosign_response_json.clone().unwrap();

			ui.label(
				egui::RichText::new("Signed! Copy the response and send it back.")
					.size(13.0)
					.color(self.colors.accent),
			);
			ui.add_space(4.0);

			let mut display = response_copy.clone();
			ui.add(
				egui::TextEdit::multiline(&mut display)
					.desired_width(ui.available_width())
					.desired_rows(4)
					.font(egui::TextStyle::Monospace),
			);
			ui.add_space(4.0);

			if ui.button("Copy Response").clicked() {
				ui.ctx().copy_text(response_copy);
				self.status = crate::types::Status::Info("Response copied!".to_string());
			}
			if ui.button("Done").clicked() {
				self.cosign_response_json = None;
				self.cosign_request_json.clear();
			}
			return;
		}

		// ── Paste area for the signing request ──
		ui.add(
			egui::TextEdit::multiline(&mut self.cosign_request_json)
				.hint_text("Paste signing request JSON here...")
				.desired_width(ui.available_width())
				.desired_rows(4)
				.font(egui::TextStyle::Monospace),
		);
		ui.add_space(4.0);

		// ── Preview + Sign ──
		if !self.cosign_request_json.trim().is_empty() {
			match serde_json::from_str::<qpv2_core::types::SigningRequest>(
				self.cosign_request_json.trim(),
			) {
				Ok(request) => {
					ui.group(|ui| {
						ui.label(
							egui::RichText::new("Transaction Details")
								.size(13.0)
								.strong()
								.color(self.colors.text),
						);
						ui.label(format!("Type: {}", request.metadata.tx_type));
						ui.label(format!("From: {}", request.metadata.from_address));
						if let Some(ref to) = request.metadata.to_address {
							ui.label(format!("To: {}", to));
						}
						if let Some(ref amount) = request.metadata.amount_ckb {
							ui.label(format!("Amount: {} CKB", amount));
						}
						ui.label(format!(
							"Threshold: {}-of-{}",
							request.multisig_config.threshold,
							request.multisig_config.signers.len()
						));
					});

					ui.add_space(8.0);

					let sign_btn = egui::Button::new(
						egui::RichText::new("Approve & Sign")
							.size(15.0)
							.strong()
							.color(self.colors.bg),
					)
					.fill(self.colors.accent)
					.min_size(egui::vec2(ui.available_width(), 40.0));

					if ui.add(sign_btn).clicked() {
						self.cosign_sign_request(request);
					}
				}
				Err(e) => {
					ui.label(
						egui::RichText::new(format!("Invalid JSON: {}", e))
							.size(11.0)
							.color(self.colors.danger),
					);
				}
			}
		}
	}

	/// Authenticate, find the matching local key, sign, and produce the response JSON.
	fn cosign_sign_request(&mut self, request: qpv2_core::types::SigningRequest) {
		use qpv2_core::KeyVault;

		let variant = match KeyVault::get_spx_variant(self.wallet_id) {
			Ok(v) => v,
			Err(e) => {
				self.status = crate::types::Status::Error(format!("Failed to read variant: {}", e));
				return;
			}
		};

		let singlesig_accounts = match KeyVault::get_singlesig_accounts(self.wallet_id) {
			Ok(a) => a,
			Err(e) => {
				self.status = crate::types::Status::Error(format!("Failed to load accounts: {}", e));
				return;
			}
		};

		let mut matched_signer_index = None;
		let mut matched_lock_args = None;

		for account in &singlesig_accounts {
			let account_pubkey = &account.config.signers[0].pubkey;
			for (i, signer) in request.multisig_config.signers.iter().enumerate() {
				if signer.pubkey == *account_pubkey && signer.variant == variant {
					matched_signer_index = Some(i);
					matched_lock_args = Some(account.lock_args.clone());
					break;
				}
			}
			if matched_signer_index.is_some() {
				break;
			}
		}

		let signer_index = match matched_signer_index {
			Some(i) => i,
			None => {
				self.status = crate::types::Status::Error(
					"No local account matches a signer in this request.".to_string(),
				);
				return;
			}
		};
		let singlesig_lock_args = matched_lock_args.unwrap();

		let message_bytes = match hex::decode(&request.signing_message) {
			Ok(b) => b,
			Err(e) => {
				self.status =
					crate::types::Status::Error(format!("Invalid signing message hex: {}", e));
				return;
			}
		};

		// Authenticate
		let auth = match &self.auth_method {
			Some(qpv2_core::types::AuthMethod::Password) => {
				match qpv2_core::pinentry::prompt_password(
						"Enter your wallet password to co-sign a multisig transaction.",
						"Password:",
					) {
					Ok(pw) => qpv2_core::types::AuthKey::Password(pw),
					Err(e) => {
						self.status = crate::types::Status::Error(format!("Auth failed: {}", e));
						return;
					}
				}
			}
			Some(qpv2_core::types::AuthMethod::Keychain) => {
				match keychain::retrieve_key(self.wallet_id) {
					Ok(key) => qpv2_core::types::AuthKey::CryptoKey(key),
					Err(e) => {
						self.status = crate::types::Status::Error(format!("Auth failed: {}", e));
						return;
					}
				}
			}
			Some(qpv2_core::types::AuthMethod::Fido2 { ref credential_id }) => {
				let cred_bytes = match hex::decode(credential_id) {
					Ok(b) => b,
					Err(e) => {
						self.status =
							crate::types::Status::Error(format!("Invalid credential: {}", e));
						return;
					}
				};
				match qpv2_core::pinentry::prompt_password(
						"Enter your wallet password to co-sign a multisig transaction.",
						"Password:",
					) {
					Ok(pin) => {
						match keychain::fido2::authenticate(&cred_bytes, &pin) {
							Ok(hmac) => {
								match qpv2_core::utilities::derive_vault_enc_key(&hmac) {
									Ok(key) => qpv2_core::types::AuthKey::CryptoKey(key),
									Err(e) => {
										self.status = crate::types::Status::Error(format!(
											"Key derivation failed: {}",
											e
										));
										return;
									}
								}
							}
							Err(e) => {
								self.status =
									crate::types::Status::Error(format!("FIDO2 auth failed: {}", e));
								return;
							}
						}
					}
					Err(e) => {
						self.status = crate::types::Status::Error(format!("PIN prompt failed: {}", e));
						return;
					}
				}
			}
			None => {
				self.status =
					crate::types::Status::Error("No authentication method set.".to_string());
				return;
			}
		};

		let vault = KeyVault::new(variant, self.wallet_id);
		let (signature, _pubkey) = match vault.raw_sign(auth, singlesig_lock_args, message_bytes) {
			Ok(s) => s,
			Err(e) => {
				self.status = crate::types::Status::Error(format!("Signing failed: {}", e));
				return;
			}
		};

		let response = qpv2_core::types::SigningResponse {
			version: request.version,
			signer_index,
			signature: hex::encode(&signature),
			signing_message: request.signing_message.clone(),
		};

		match serde_json::to_string_pretty(&response) {
			Ok(json) => {
				self.cosign_response_json = Some(json);
				self.status =
					crate::types::Status::Info("Signed! Copy the response below.".to_string());
			}
			Err(e) => {
				self.status =
					crate::types::Status::Error(format!("Failed to serialize response: {}", e));
			}
		}
	}
}
