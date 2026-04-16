//! Passkey constants and platform helpers (macOS only).

use qpv2_core::{KeyVault, types::{AuthMethod, SpxVariant}};
use crate::types::Status;
use crate::App;

pub(crate) const RP_ID: &str = "quantumpurse.org";
pub(crate) const PRF_SALT: &[u8] = b"quantumpurse-kv-seed-encryption";

impl App {
    /// Read the stored credential ID for passkey-based wallets.
    pub(crate) fn get_credential_id(&mut self) -> Option<Vec<u8>> {
        let temp_vault = KeyVault::new(SpxVariant::Sha2128S);
        let wallet_info = match temp_vault.read_wallet_info() {
            Ok(info) => info,
            Err(e) => {
                self.status = Status::Error(format!("Failed to read wallet info: {}", e));
                return None;
            }
        };
        match wallet_info.auth_method {
            AuthMethod::PasskeyPrf { credential_id } => Some(credential_id),
            AuthMethod::Password => {
                self.status =
                    Status::Error("This wallet uses password auth, not Touch ID.".to_string());
                None
            }
        }
    }
}
