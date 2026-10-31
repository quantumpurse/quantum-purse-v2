//! Multisig co-signing logic: collecting co-signers' responses on the
//! initiating side, and answering someone else's signing request on the
//! co-signing side. The screens that drive it are in `ui/cosigner.rs`.

use qpv2_core::types::{
    AuthKey, AuthMethod, MsigSignature, SigningRequest, SigningResponse, SpxVariant,
};
use qpv2_core::KeyVault;

use crate::types::{Status, TransactionStatus};
use crate::wallet::{get_fido2_dek, spawn_fido2_job};
use crate::App;

/// Who in this wallet signs a co-signer's request, and what.
struct CosignSigner {
    variant: SpxVariant,
    /// This wallet's keys among the request's signers: each one's position,
    /// and the single-sig account holding it.
    keys: Vec<(usize, String)>,
    /// The message to sign, decoded from the request.
    message: Vec<u8>,
}

impl App {
    /// Parse and validate the import buffer as a `SigningResponse`, then add
    /// its signatures to the collected set.
    pub(crate) fn import_cosigner_response(&mut self) {
        tracing::info!("Import cosigner response triggered.");

        let (request, signatures, import_response_json) = match &mut self.tx_status {
            TransactionStatus::AwaitingCoSigners {
                request,
                signatures,
                import_response_json,
                ..
            } => (request, signatures, import_response_json),
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
            .map(|s| s.signer_index.to_string())
            .collect();
        signatures.extend(response.signed);
        import_response_json.clear();
        self.status = crate::types::Status::Info(format!(
            "Signature from signer {} imported.",
            signer_positions.join(", ")
        ));
    }

    /// Checks a co-signer's request against THIS wallet's accounts: which
    /// accounts sign, at which positions among the signers, and the message.
    fn find_local_signer(&self, request: &SigningRequest) -> Result<CosignSigner, String> {
        let variant = KeyVault::get_spx_variant(self.wallet_id)
            .map_err(|e| format!("Failed to read variant: {}", e))?;
        let keys = KeyVault::find_local_signers(self.wallet_id, request.multisig_config())?;

        let message = hex::decode(request.signing_message())
            .map_err(|e| format!("Invalid signing message hex: {}", e))?;

        Ok(CosignSigner {
            variant,
            keys,
            message,
        })
    }

    /// Co-signs on a password or Touch ID wallet: checks the request, asks
    /// for the key as it signs, once per key this wallet holds, then shows
    /// the response.
    pub(crate) fn co_sign(&mut self, request: SigningRequest) {
        let signer = match self.find_local_signer(&request) {
            Ok(s) => s,
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

        let signed = sign(self.wallet_id, &request, signer, |purpose| {
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
        let signer = match self.find_local_signer(&request) {
            Ok(s) => s,
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
            sign(wallet_id, &request, signer, |purpose| {
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

/// Signs a co-signer's request with each key this wallet holds in it,
/// unlocking the wallet with `unlock` for each, and returns the response to
/// send back. Touches no UI state, so it can run on a worker.
fn sign(
    wallet_id: u32,
    request: &SigningRequest,
    signer: CosignSigner,
    mut unlock: impl FnMut(&str) -> Result<AuthKey, String>,
) -> Result<String, String> {
    let vault = KeyVault::new(signer.variant, wallet_id);
    let total = signer.keys.len();
    let mut signed = Vec::with_capacity(total);
    for (k, (signer_index, lock_args)) in signer.keys.into_iter().enumerate() {
        // The unlock dialog names the key, so signing twice isn't a surprise.
        let purpose = if total == 1 {
            "co-sign a multisig transaction".to_string()
        } else {
            format!(
                "co-sign a multisig transaction (key {} of {})",
                k + 1,
                total
            )
        };
        let auth = unlock(&purpose)?;
        let (signature, _pubkey) = vault
            .raw_sign(auth, lock_args, signer.message.clone())
            .map_err(|e| format!("Signing failed: {}", e))?;
        signed.push(MsigSignature {
            signer_index,
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
