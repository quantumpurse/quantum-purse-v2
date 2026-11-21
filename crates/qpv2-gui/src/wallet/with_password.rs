//! Password wallets: create and import, with the password asked for in the
//! pinentry dialog.

use qpv2_core::types::{AuthKey, AuthMethod, SingleSigConvention, SpxVariant};
use qpv2_core::KeyVault;

use super::StartingBlock;
use crate::types::Status;
use crate::App;

impl App {
    /// Create a password-mode wallet. Opens the pinentry dialog with
    /// a confirmation field — the dialog itself enforces the match
    /// (the user can't submit until both fields agree). On submit,
    /// validates strength via `validate_password`, generates the
    /// master seed, derives the first account, and transitions to
    /// `Screen::Unlocked`. Cancellation surfaces as a quiet info
    /// banner; nothing else changes.
    pub(crate) fn create_wallet_with_password(&mut self, variant: SpxVariant) {
        let (wallet_id, wallet_name) = match self.prepare_new_wallet() {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };
        let pw = match qpv2_core::pinentry::prompt_password_with_confirmation(
            "Choose a password for your wallet. You'll be prompted for it \
             again on every signing operation.",
            "Password:",
            "Confirm:",
            "Passwords do not match.",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        let strength_str = match qpv2_core::utilities::validate_password(&pw) {
            Ok(bits) => format!(" Password strength: {} bits.", bits),
            Err(e) => {
                let msg = format!("Weak password: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        let pw_for_account = pw.clone();
        let vault = KeyVault::new(variant, wallet_id);
        if let Err(e) = vault.generate_master_seed(
            AuthKey::Password(pw),
            AuthMethod::Password,
            &wallet_name,
            false,
        ) {
            let msg = format!("Failed to create wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        // The wallet exists now, so it opens even if this fails: empty, for
        // NEW ACCOUNT to retry.
        let first_account = vault.gen_singlesig_account(AuthKey::Password(pw_for_account), None);

        self.finalize_wallet_setup(
            AuthMethod::Password,
            &format!("Wallet created successfully!{}", strength_str),
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

    pub(crate) fn import_seed_phrase_with_password(
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

        let pw = match qpv2_core::pinentry::prompt_password_with_confirmation(
            "Choose a password for your imported wallet. You'll be prompted for it \
             again on every signing operation.",
            "Password:",
            "Confirm:",
            "Passwords do not match.",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        let strength_str = match qpv2_core::utilities::validate_password(&pw) {
            Ok(bits) => format!(" Password strength: {} bits.", bits),
            Err(e) => {
                let msg = format!("Weak password: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        let pw_for_account = pw.clone();
        let vault = KeyVault::new(variant, wallet_id);

        if let Err(e) = vault.import_seed_phrase(
            seed_phrase,
            AuthKey::Password(pw),
            AuthMethod::Password,
            &wallet_name,
            single_sig_convention,
            false,
        ) {
            let msg = format!("Failed to import wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        // The wallet exists now, so it opens even if this fails: empty, for
        // NEW ACCOUNT to retry.
        let first_account = vault.gen_singlesig_account(AuthKey::Password(pw_for_account), None);

        self.finalize_wallet_setup(
            AuthMethod::Password,
            &format!("Wallet imported successfully!{}", strength_str),
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
