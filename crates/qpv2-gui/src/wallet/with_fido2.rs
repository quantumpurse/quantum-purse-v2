//! FIDO2 security-key wallets: create, import, new account and seed export,
//! plus the helpers that ask the key for the vault key on a worker thread.

use qpv2_core::types::{AuthKey, AuthMethod, SingleSigConvention, SpxVariant};
use qpv2_core::KeyVault;

use super::{get_exported_seed_phrase, StartingBlock};
use crate::types::{
    Fido2AccountCreate, Fido2WalletFlow, Fido2WalletSetup, Fido2WalletUpdate, Status,
};
use crate::App;

/// Name of the worker that runs a FIDO2 wallet create or import, so the log
/// shows which thread made each call to the key.
const FIDO2_SETUP_THREAD: &str = "fido2-setup-thread";

/// Name of the worker that asks the security key for a new account.
const FIDO2_ACCOUNT_THREAD: &str = "fido2-account-thread";

/// Name of the worker that asks the security key and then signs, co-signs or
/// reads the seed phrase.
const FIDO2_JOB_THREAD: &str = "fido2-job-thread";

/// Why a security-key job was refused: one request at a time, as a second one
/// would reach the key while it waits for the touch the first one asked for.
const FIDO2_BUSY: &str = "The security key is busy with another request — finish it and try again.";

impl App {
    /// Create a FIDO2-authenticated wallet. Asks the key how it verifies
    /// its user (a pinentry PIN dialog, or nothing for a key that verifies
    /// on-device), registers a credential, then derives the encryption key
    /// via hmac-secret.
    ///
    /// Runs on a worker thread, like `start_create_trezor_watch_only_wallet`:
    /// the PIN dialog can stay open for minutes and the key waits for a
    /// touch or an on-device confirm, so the screen it was started from has
    /// to keep painting to say what it waits for. `poll_fido2_wallet_setup`
    /// picks up the result for `finish_create_wallet_with_fido2`. No-op
    /// while a security-key setup is already in flight.
    pub(crate) fn start_create_wallet_with_fido2(&mut self, variant: SpxVariant) {
        if self.fido2_connect_rx.is_some() {
            return;
        }
        // Settle the name now: the key stores it at registration, so the
        // wallet keeps this one instead of generating another at the end.
        let wallet_name = match self.prepare_new_wallet() {
            Ok((_, name)) => name,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.fido2_connect_rx = Some(rx);

        let worker = std::thread::Builder::new().name(FIDO2_SETUP_THREAD.to_string());
        let spawned = worker.spawn(move || {
            let result = (|| -> Fido2WalletUpdate {
                let verification = fido2_verification("register a new credential")?;
                let credential = keychain::fido2::register(&verification, &wallet_name)
                    .map_err(|e| format!("FIDO2 registration failed: {}", e))?;
                let hmac_output =
                    keychain::fido2::authenticate(&credential.credential_id, &verification)
                        .map_err(|e| format!("FIDO2 authentication failed: {}", e))?;
                let key = qpv2_core::utilities::derive_vault_enc_key(&hmac_output)
                    .map_err(|e| format!("Key derivation failed: {}", e))?;
                Ok(Fido2WalletFlow::Create(Fido2WalletSetup {
                    wallet_name,
                    variant,
                    credential_id: hex::encode(&credential.credential_id),
                    key,
                }))
            })();
            let _ = tx.send(result);
        });
        // The failed spawn drops the sender, so the poller reports the end.
        if let Err(e) = spawned {
            tracing::error!("Failed to start the FIDO2 setup thread: {}", e);
        }
    }

    /// Write the wallet `start_create_wallet_with_fido2` prepared. Called by
    /// `poll_fido2_wallet_setup` on the UI thread; the wallet id is
    /// allocated only now, so a cancelled or failed key step leaves no empty
    /// wallet behind. The core checks the name again when it writes.
    pub(crate) fn finish_create_wallet_with_fido2(&mut self, setup: Fido2WalletSetup) {
        let Fido2WalletSetup {
            wallet_name,
            variant,
            credential_id,
            key,
        } = setup;
        let wallet_id = match qpv2_core::db::wallets::next_wallet_id() {
            Ok(id) => id,
            Err(e) => {
                let msg = e.to_string();
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        let key_for_account = key.clone();
        let auth_method = AuthMethod::Fido2 {
            credential_id: credential_id.clone(),
        };

        let vault = KeyVault::new(variant, wallet_id);
        if let Err(e) =
            vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method, &wallet_name)
        {
            let msg = format!("Failed to create wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        // The wallet exists now, so it opens even if this fails: empty, for
        // NEW ACCOUNT to retry.
        let first_account = vault.gen_singlesig_account(AuthKey::CryptoKey(key_for_account));

        self.finalize_wallet_setup(
            AuthMethod::Fido2 { credential_id },
            "Wallet created with FIDO2 security key!",
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

    /// Import a seed phrase into a FIDO2-authenticated wallet: asks for the
    /// phrase, then takes the same key steps as
    /// `start_create_wallet_with_fido2`, on a worker thread for the same
    /// reason. `poll_fido2_wallet_setup` picks up the result for
    /// `finish_import_seed_phrase_with_fido2`. No-op while a security-key
    /// setup is already in flight.
    pub(crate) fn start_import_seed_phrase_with_fido2(
        &mut self,
        variant: SpxVariant,
        single_sig_convention: SingleSigConvention,
    ) {
        if self.fido2_connect_rx.is_some() {
            return;
        }
        // Settle the name now: the key stores it at registration, so the
        // wallet keeps this one instead of generating another at the end.
        let wallet_name = match self.prepare_new_wallet() {
            Ok((_, name)) => name,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.fido2_connect_rx = Some(rx);

        let worker = std::thread::Builder::new().name(FIDO2_SETUP_THREAD.to_string());
        let spawned = worker.spawn(move || {
            let result = (|| -> Fido2WalletUpdate {
                // Before the key steps, so a cancelled phrase leaves no
                // credential behind on the key.
                let seed_phrase = qpv2_core::pinentry::prompt_seed_phrase(variant)?;
                let verification = fido2_verification("register a new credential")?;
                let credential = keychain::fido2::register(&verification, &wallet_name)
                    .map_err(|e| format!("FIDO2 registration failed: {}", e))?;
                let hmac_output =
                    keychain::fido2::authenticate(&credential.credential_id, &verification)
                        .map_err(|e| format!("FIDO2 authentication failed: {}", e))?;
                let key = qpv2_core::utilities::derive_vault_enc_key(&hmac_output)
                    .map_err(|e| format!("Key derivation failed: {}", e))?;
                Ok(Fido2WalletFlow::Import(
                    Fido2WalletSetup {
                        wallet_name,
                        variant,
                        credential_id: hex::encode(&credential.credential_id),
                        key,
                    },
                    seed_phrase,
                    single_sig_convention,
                ))
            })();
            let _ = tx.send(result);
        });
        // The failed spawn drops the sender, so the poller reports the end.
        if let Err(e) = spawned {
            tracing::error!("Failed to start the FIDO2 setup thread: {}", e);
        }
    }

    /// Write the wallet `start_import_seed_phrase_with_fido2` prepared.
    /// Called by `poll_fido2_wallet_setup` on the UI thread; the wallet id
    /// is allocated only now, so a cancelled or failed key step leaves no
    /// empty wallet behind. The core checks the name again when it writes.
    pub(crate) fn finish_import_seed_phrase_with_fido2(
        &mut self,
        setup: Fido2WalletSetup,
        seed_phrase: qpv2_core::SecureString,
        single_sig_convention: SingleSigConvention,
    ) {
        let Fido2WalletSetup {
            wallet_name,
            variant,
            credential_id,
            key,
        } = setup;
        let wallet_id = match qpv2_core::db::wallets::next_wallet_id() {
            Ok(id) => id,
            Err(e) => {
                let msg = e.to_string();
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        let key_for_account = key.clone();
        let auth_method = AuthMethod::Fido2 {
            credential_id: credential_id.clone(),
        };
        let vault = KeyVault::new(variant, wallet_id);

        if let Err(e) = vault.import_seed_phrase(
            seed_phrase,
            AuthKey::CryptoKey(key),
            auth_method,
            &wallet_name,
            single_sig_convention,
        ) {
            let msg = format!("Failed to import wallet: {}", e);
            tracing::error!("{}", msg);
            self.status = Status::Error(msg);
            return;
        }

        // The wallet exists now, so it opens even if this fails: empty, for
        // NEW ACCOUNT to retry.
        let first_account = vault.gen_singlesig_account(AuthKey::CryptoKey(key_for_account));

        // The other import buttons clear this at the click; this one keeps
        // it for a retry until the wallet exists.
        self.import_from_v1 = false;
        self.finalize_wallet_setup(
            AuthMethod::Fido2 { credential_id },
            "Wallet imported with FIDO2 security key!",
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

    /// Start creating the next account of a FIDO2 wallet. The PIN dialog and
    /// the key's request run on a worker, as in `start_create_wallet_with_fido2`,
    /// so the window keeps painting and the accounts panel can tell the user
    /// to touch the key. `poll_fido2_new_account` hands the vault key to
    /// `finish_create_fido2_wallet_account`. No-op while one is
    /// already in flight.
    pub(super) fn start_create_fido2_wallet_account(&mut self, credential_id: String) {
        if self.fido2_new_account_rx.is_some() {
            return;
        }
        let cred_bytes = match hex::decode(&credential_id) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("Invalid credential ID: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.fido2_new_account_rx = Some(rx);

        let worker = std::thread::Builder::new().name(FIDO2_ACCOUNT_THREAD.to_string());
        let spawned = worker.spawn(move || {
            let result = (|| -> Fido2AccountCreate {
                let verification = fido2_verification("create a new account")?;
                let hmac_output = keychain::fido2::authenticate(&cred_bytes, &verification)?;
                qpv2_core::utilities::derive_vault_enc_key(&hmac_output)
                    .map(AuthKey::CryptoKey)
                    .map_err(|e| format!("Key derivation failed: {}", e))
            })();
            let _ = tx.send(result);
        });
        // The failed spawn drops the sender, so the poller reports the end.
        if let Err(e) = spawned {
            tracing::error!("Failed to start the FIDO2 account thread: {}", e);
        }
    }

    /// Derive and store the account `start_create_fido2_wallet_account`
    /// got the vault key for. Called by `poll_fido2_new_account` on the UI thread.
    pub(crate) fn finish_create_fido2_wallet_account(&mut self, auth: AuthKey) {
        let variant = match KeyVault::get_spx_variant(self.wallet_id) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("Failed to read wallet variant: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
                return;
            }
        };
        let vault = KeyVault::new(variant, self.wallet_id);
        match vault.gen_singlesig_account(auth) {
            Ok(account) => {
                tracing::info!(
                    "Account created (wallet_id={}, lock_args={}...)",
                    self.wallet_id,
                    &account.lock_args[..8.min(account.lock_args.len())]
                );
                self.balances.insert(account.lock_args.clone(), Some(0));
                self.spendable_balances
                    .insert(account.lock_args.clone(), Some(0));
                // A wallet's first account may hold history: an import whose
                // first account failed opens empty, and this makes it.
                let start = if self.accounts.is_empty() {
                    StartingBlock::Detect
                } else {
                    StartingBlock::Tip
                };
                self.register_lock_scripts_with_light_client(
                    std::slice::from_ref(&account.lock_args),
                    start,
                );
                self.accounts.push(account);
                self.refresh_wallet_cache();
                self.status = Status::Info("New account created!".to_string());
            }
            Err(e) => {
                let msg = format!("Failed to create account: {}", e);
                tracing::error!("{}", msg);
                self.status = Status::Error(msg);
            }
        }
    }

    /// Shows the seed phrase on a FIDO2 wallet. The security key and the
    /// decryption run in the background, so the window stays live while the
    /// key waits for a touch; `poll_fido2_export` opens the dialog.
    pub(crate) fn export_seed_phrase_with_fido2(&mut self) {
        if self.seed_export_dialog_done_rx.is_some() {
            return;
        }
        let credential_id = match self.get_fido2_credential() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
                return;
            }
        };
        let wallet_id = self.wallet_id;
        let job = move || {
            let auth = get_fido2_dek(&credential_id, "export the seed phrase")?;
            get_exported_seed_phrase(wallet_id, auth)
        };
        match spawn_fido2_job(job) {
            Ok(rx) => self.fido2_export_rx = Some(rx),
            Err(e) => {
                tracing::error!("{}", e);
                self.status = Status::Error(e);
            }
        }
    }

    /// True while one of our requests waits on a security key: a wallet create
    /// or import, a new account, or a signature, co-signature or seed export.
    /// The security-key counterpart of `trezor_operation_in_flight`.
    pub(crate) fn fido2_operation_in_flight(&self) -> bool {
        self.fido2_connect_rx.is_some()
            || self.fido2_new_account_rx.is_some()
            || self.fido2_sign_rx.is_some()
            || self.fido2_cosign_rx.is_some()
            || self.fido2_export_rx.is_some()
            || self.governance.bind_uses_fido2
    }

    /// The open wallet's security-key credential, for a new job on the key.
    /// Refused while another request waits on the key (`FIDO2_BUSY`).
    pub(crate) fn get_fido2_credential(&self) -> Result<String, String> {
        if self.fido2_operation_in_flight() {
            return Err(FIDO2_BUSY.to_string());
        }
        match &self.auth_method {
            Some(AuthMethod::Fido2 { credential_id }) => Ok(credential_id.clone()),
            _ => Err("This wallet doesn't use a FIDO2 security key.".to_string()),
        }
    }
}

/// Asks the connected security key how it verifies its user, and prompts for
/// the key's PIN only when the key wants one from this host. A key that
/// verifies on itself (a Trezor, a fingerprint key) gets no dialog.
fn fido2_verification(purpose: &str) -> Result<keychain::fido2::Verification, String> {
    // Every FIDO2 action passes here before its first USB call. If the app
    // dies inside the USB library, the log's last line names the action and
    // the thread it ran on.
    tracing::info!("FIDO2: looking for a security key to {}", purpose);
    if !keychain::fido2::needs_host_pin()? {
        return Ok(keychain::fido2::Verification::OnDevice);
    }
    let pin = qpv2_core::pinentry::prompt_password(
        &format!("Enter your FIDO2 security key PIN to {}.", purpose),
        "PIN:",
    )?;
    Ok(keychain::fido2::Verification::OnHost(pin))
}

/// Asks the security key for the wallet's vault key: the PIN dialog when the
/// key wants one, then a touch. Blocks until the key answers, so it runs on a
/// worker (`spawn_fido2_job`).
pub(crate) fn get_fido2_dek(credential_id: &str, purpose: &str) -> Result<AuthKey, String> {
    let cred_bytes =
        hex::decode(credential_id).map_err(|e| format!("Invalid credential ID: {}", e))?;
    let verification = fido2_verification(purpose)?;
    let hmac_output = keychain::fido2::authenticate(&cred_bytes, &verification)?;
    qpv2_core::utilities::derive_vault_enc_key(&hmac_output)
        .map(AuthKey::CryptoKey)
        .map_err(|e| format!("Key derivation failed: {}", e))
}

/// Runs a security-key job on a worker named for the log, and hands back
/// where its result will arrive.
pub(crate) fn spawn_fido2_job<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<std::sync::mpsc::Receiver<T>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(FIDO2_JOB_THREAD.to_string())
        .spawn(move || {
            let _ = tx.send(job());
        })
        .map_err(|e| format!("Failed to start the FIDO2 thread: {}", e))?;
    Ok(rx)
}
