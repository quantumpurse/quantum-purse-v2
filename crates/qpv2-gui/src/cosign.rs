//! Multisig co-signing logic: collecting co-signers' responses on the
//! initiating side, and answering someone else's signing request on the
//! co-signing side. The screens that drive it are in `ui/cosigner.rs`.

use qpv2_core::types::{
    AuthKey, AuthMethod, LocalSigner, MsigSignature, SigningRequest, SigningResponse,
};
use qpv2_core::KeyVault;

use crate::types::{Status, TransactionStatus};
use crate::wallet::{get_fido2_dek, spawn_fido2_job};
use crate::App;

impl App {
    /// Parse and validate the import buffer as a `SigningResponse`, then add
    /// its signatures to the collected set.
    pub(crate) fn import_cosigner_response(&mut self) {
        tracing::info!("Import cosigner response triggered.");

        let (request, signatures, signing_response_json) = match &mut self.tx_status {
            TransactionStatus::AwaitingCoSigners {
                request,
                signatures,
                signing_response_json,
                ..
            } => (request, signatures, signing_response_json),
            _ => {
                tracing::error!("import_cosigner_response: not in AwaitingCoSigners state.");
                return;
            }
        };

        let buf = signing_response_json.trim().to_string();
        tracing::info!("Import buffer length: {} chars.", buf.len());
        if buf.is_empty() {
            self.status = crate::types::Status::Error("Paste a response first.".to_string());
            return;
        }

        let response: qpv2_core::types::SigningResponse = match serde_json::from_str(&buf) {
            Ok(r) => r,
            Err(e) => {
                self.status = crate::types::Status::Error(format!("Invalid response JSON: {}", e));
                return;
            }
        };

        if let Err(e) = response.validate(request, signatures) {
            self.status = crate::types::Status::Error(e);
            return;
        }

        let signer_positions: Vec<String> = response
            .signed
            .iter()
            .map(|s| format!("S{}", s.signer_index + 1))
            .collect();
        signatures.extend(response.signed);
        signing_response_json.clear();
        self.status = crate::types::Status::Info(format!(
            "Signature from {} imported.",
            signer_positions.join(", ")
        ));
    }

    /// Co-signs on a password or Touch ID wallet: checks the request, asks
    /// for the key as it signs, once per key this wallet holds, then shows
    /// the response.
    pub(crate) fn co_sign(&mut self, request: SigningRequest) {
        let local_signers =
            match KeyVault::find_local_signers(self.wallet_id, request.multisig_config()) {
                Ok(found) => found,
                Err(e) => {
                    self.status = Status::Error(e);
                    return;
                }
            };

        // Authenticate. Trezor is refused up front — the firmware only signs
        // a transaction locked by its own single-sig lock, so there is no
        // device flow for `resolve_auth_key`'s generic Trezor message to
        // point at.
        if matches!(self.auth_method, Some(AuthMethod::Trezor { .. })) {
            self.status = Status::Error(
                "Co-signing a multisig transaction on a Trezor is not supported yet.".to_string(),
            );
            return;
        }

        let signed = sign(self.wallet_id, &request, local_signers, |purpose| {
            self.resolve_auth_key(purpose)
                .map_err(|e| format!("Auth failed: {}", e))
        });
        match signed {
            Ok(response) => self.show_cosign_response(response),
            Err(e) => self.status = Status::Error(e),
        }
    }

    /// Co-signs on a FIDO2 wallet: checks the request, then the security key
    /// and the signing run in the background, so the window stays live while
    /// the key waits for a touch, one per key this wallet holds;
    /// `poll_fido2_cosign` shows the response.
    pub(crate) fn co_sign_with_fido2(&mut self, request: SigningRequest) {
        let local_signers =
            match KeyVault::find_local_signers(self.wallet_id, request.multisig_config()) {
                Ok(found) => found,
                Err(e) => {
                    self.status = Status::Error(e);
                    return;
                }
            };
        let credential_id = match self.get_fido2_credential() {
            Ok(c) => c,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        let wallet_id = self.wallet_id;
        let job = move || {
            sign(wallet_id, &request, local_signers, |purpose| {
                get_fido2_dek(&credential_id, purpose).map_err(|e| format!("Auth failed: {}", e))
            })
        };
        match spawn_fido2_job(job) {
            Ok(rx) => self.fido2_cosign_rx = Some(rx),
            Err(e) => self.status = Status::Error(e),
        }
    }

    /// Puts a signed co-signer response on screen, to copy back.
    pub(crate) fn show_cosign_response(&mut self, response: String) {
        self.cosign_response_json = Some(response);
        self.status = Status::Info("Signed! Copy the response below.".to_string());
    }
}

/// Signs a co-signer's request as each of `local_signers`, the signers whose
/// keys this wallet holds, unlocking the wallet with `unlock` for each, and
/// returns the response to send back. Touches no UI state, so it can run on
/// a worker.
fn sign(
    wallet_id: u32,
    request: &SigningRequest,
    local_signers: Vec<LocalSigner>,
    mut unlock: impl FnMut(&str) -> Result<AuthKey, String>,
) -> Result<String, String> {
    let variant = KeyVault::get_spx_variant(wallet_id)
        .map_err(|e| format!("Failed to read variant: {}", e))?;
    let vault = KeyVault::new(variant, wallet_id);
    let message = request.signing_message().as_bytes().to_vec();
    let total = local_signers.len();
    let mut signed = Vec::with_capacity(total);
    for (i, local_signer) in local_signers.into_iter().enumerate() {
        // The unlock dialog names the key, so signing twice isn't a surprise.
        let purpose = if total == 1 {
            "co-sign a multisig transaction".to_string()
        } else {
            format!(
                "co-sign a multisig transaction (key {} of {})",
                i + 1,
                total
            )
        };
        let auth = unlock(&purpose)?;
        let (signature, _pubkey) = vault
            .raw_sign(auth, None, local_signer.lock_args, message.clone())
            .map_err(|e| format!("Signing failed: {}", e))?;
        signed.push(MsigSignature {
            signer_index: usize::from(local_signer.signer.index()),
            signature,
        });
    }
    let response = SigningResponse {
        version: request.version(),
        signed,
        signing_message: request.signing_message().to_string(),
    };
    serde_json::to_string_pretty(&response)
        .map_err(|e| format!("Failed to serialize response: {}", e))
}
