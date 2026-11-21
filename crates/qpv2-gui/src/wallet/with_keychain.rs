//! Keychain wallets (Touch ID on macOS, the TPM on Windows and Linux): create
//! and import, with a random key kept in the platform credential store.

use qpv2_core::types::{AuthKey, AuthMethod, SingleSigConvention, SpxVariant};
use qpv2_core::KeyVault;

use super::StartingBlock;
use crate::types::Status;
use crate::App;

impl App {
    /// Create a Keychain wallet. Generates a random 32-byte key,
    /// stores it in the platform credential store, then creates the
    /// wallet and first account.
    pub(crate) fn create_wallet_with_keychain(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };
        let key = match qpv2_core::utilities::get_random_bytes(32) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("Failed to generate key: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        if let Err(e) = keychain::store_key(wallet_id, &key) {
            let msg = format!("Failed to store key in Keychain: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        let key_for_account = key.clone();

        let vault = KeyVault::new(variant, wallet_id);
        if let Err(e) = vault.generate_master_seed(
            AuthKey::CryptoKey(key),
            AuthMethod::Keychain,
            &wallet_name,
            false,
        ) {
            let _ = keychain::delete_key(wallet_id);
            let msg = format!("Failed to create wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        // The wallet exists now, so it opens even if this fails: empty, for
        // NEW ACCOUNT to retry.
        let first_account = vault.gen_singlesig_account(AuthKey::CryptoKey(key_for_account), None);

        self.finalize_wallet_setup(
            AuthMethod::Keychain,
            &format!("Wallet created with {}!", keychain::short_name()),
            wallet_id,
            wallet_name,
            StartingBlock::Tip,
        );
        if let Err(e) = first_account {
            let msg = format!(
                "Failed to create first account: {}. Press NEW ACCOUNT to retry.",
                e.to_string().trim_end_matches('.')
            );
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
        }
    }

    pub(crate) fn import_seed_phrase_with_keychain(
        &mut self,
        variant: SpxVariant,
        single_sig_convention: SingleSigConvention,
    ) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };
        let seed_phrase = match qpv2_core::pinentry::prompt_seed_phrase(variant) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        let key = match qpv2_core::utilities::get_random_bytes(32) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("Failed to generate key: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        if let Err(e) = keychain::store_key(wallet_id, &key) {
            let msg = format!("Failed to store key in Keychain: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        let key_for_account = key.clone();
        let vault = KeyVault::new(variant, wallet_id);

        if let Err(e) = vault.import_seed_phrase(
            seed_phrase,
            AuthKey::CryptoKey(key),
            AuthMethod::Keychain,
            &wallet_name,
            single_sig_convention,
            false,
        ) {
            let _ = keychain::delete_key(wallet_id);
            let msg = format!("Failed to import wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        // The wallet exists now, so it opens even if this fails: empty, for
        // NEW ACCOUNT to retry.
        let first_account = vault.gen_singlesig_account(AuthKey::CryptoKey(key_for_account), None);

        self.finalize_wallet_setup(
            AuthMethod::Keychain,
            &format!("Wallet imported with {}!", keychain::short_name()),
            wallet_id,
            wallet_name,
            StartingBlock::Detect,
        );
        if let Err(e) = first_account {
            let msg = format!(
                "Failed to create first account: {}. Press NEW ACCOUNT to retry.",
                e.to_string().trim_end_matches('.')
            );
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
        }
    }
}
