//! The master seed, opened for key derivation.

use fips205::{
    traits::{KeyGen, SerDes},
    *,
};
use zeroize::Zeroize;

use crate::constants::KDF_PATH_PREFIX;
use crate::spx_keygen;
use crate::types::{AuthKey, SpxVariant};
use crate::{db, utilities, KeyVault, SecureVec};

/// A wallet's master seed, decrypted, with the wallet's passphrase mixed in
/// and verified. Only [`MasterSeed::open`] makes one, and keys are derived
/// from nothing else, so no key can come from a seed that skipped a step.
/// The seed bytes never leave this type.
pub(crate) struct MasterSeed {
    seed: SecureVec,
    /// The wallet's SPHINCS+ parameter set, which every key is derived with.
    variant: SpxVariant,
}

impl MasterSeed {
    /// Opens the wallet's master seed for key derivation. A passphrase that
    /// disagrees with the wallet's `is_passphrase` is refused before the seed
    /// is decrypted, and a wrong one is refused after it is mixed in.
    ///
    /// **Parameters**:
    /// - `vault: &KeyVault` - The wallet whose master seed is opened.
    /// - `auth: &AuthKey` - The authentication key used to decrypt the master seed.
    /// - `passphrase: Option<&str>` - The wallet's passphrase, or `None` for a wallet without one.
    ///
    /// **Returns**:
    /// - `Result<MasterSeed, String>` - The opened master seed on success, or an error message on failure.
    pub(crate) fn load(
        vault: &KeyVault,
        auth: &AuthKey,
        passphrase: Option<&str>,
    ) -> Result<Self, String> {
        Self::check_passphrase_compatibility(vault, passphrase)?;

        let payload = db::get_encrypted_seed(vault.wallet_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                tracing::error!("Master seed not found");
                "Master seed not found".to_string()
            })?;
        let mut seed = utilities::decrypt(auth, payload)?;
        utilities::apply_passphrase(&mut seed, passphrase)?;

        let seed = MasterSeed {
            seed,
            variant: vault.variant,
        };
        seed.verify_passphrase(vault, passphrase)?;
        Ok(seed)
    }

    /// To derive SPHINCS+ key pair. One master seed can derive multiple child index-based SPHINCS+ key pairs on demand.
    /// The master seed MUST carry at least N*3 bytes of entropy or this panics.
    ///
    /// **Parameters**:
    /// - `index: u32` - The index of the child sphincs+ key to be derived.
    ///
    /// **Returns**:
    /// - `Result<(SecureVec, SecureVec), String>` - The SPHINCS+ key pair on success, or an error message on failure.
    pub(crate) fn derive_spx_keys(&self, index: u32) -> Result<(SecureVec, SecureVec), String> {
        let seed = &self.seed;
        match self.variant {
            SpxVariant::Sha2128S => {
                spx_keygen!(slh_dsa_sha2_128s::KG, slh_dsa_sha2_128s::N, seed, index)
            }
            SpxVariant::Sha2128F => {
                spx_keygen!(slh_dsa_sha2_128f::KG, slh_dsa_sha2_128f::N, seed, index)
            }
            SpxVariant::Sha2192S => {
                spx_keygen!(slh_dsa_sha2_192s::KG, slh_dsa_sha2_192s::N, seed, index)
            }
            SpxVariant::Sha2192F => {
                spx_keygen!(slh_dsa_sha2_192f::KG, slh_dsa_sha2_192f::N, seed, index)
            }
            SpxVariant::Sha2256S => {
                spx_keygen!(slh_dsa_sha2_256s::KG, slh_dsa_sha2_256s::N, seed, index)
            }
            SpxVariant::Sha2256F => {
                spx_keygen!(slh_dsa_sha2_256f::KG, slh_dsa_sha2_256f::N, seed, index)
            }
            SpxVariant::Shake128S => {
                spx_keygen!(slh_dsa_shake_128s::KG, slh_dsa_shake_128s::N, seed, index)
            }
            SpxVariant::Shake128F => {
                spx_keygen!(slh_dsa_shake_128f::KG, slh_dsa_shake_128f::N, seed, index)
            }
            SpxVariant::Shake192S => {
                spx_keygen!(slh_dsa_shake_192s::KG, slh_dsa_shake_192s::N, seed, index)
            }
            SpxVariant::Shake192F => {
                spx_keygen!(slh_dsa_shake_192f::KG, slh_dsa_shake_192f::N, seed, index)
            }
            SpxVariant::Shake256S => {
                spx_keygen!(slh_dsa_shake_256s::KG, slh_dsa_shake_256s::N, seed, index)
            }
            SpxVariant::Shake256F => {
                spx_keygen!(slh_dsa_shake_256f::KG, slh_dsa_shake_256f::N, seed, index)
            }
        }
    }

    /// Refuses a passphrase that disagrees with how the wallet was created: a
    /// passphrase wallet given none, or a plain wallet given one. Either would
    /// derive the keys of a different, empty wallet without any error.
    ///
    /// **Parameters**:
    /// - `vault: &KeyVault` - The wallet whose recorded `is_passphrase` is checked.
    /// - `passphrase: Option<&str>` - The passphrase about to be applied, or `None`.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Nothing when the two agree, or an error message when they do not.
    fn check_passphrase_compatibility(
        vault: &KeyVault,
        passphrase: Option<&str>,
    ) -> Result<(), String> {
        let wallet_info = KeyVault::get_meta(vault.wallet_id)?;
        match (wallet_info.is_passphrase, passphrase.is_some()) {
            (true, false) => Err("This wallet uses a passphrase, but none was given.".to_string()),
            (false, true) => {
                Err("This wallet does not use a passphrase, but one was given.".to_string())
            }
            _ => Ok(()),
        }
    }

    /// Refuses a wrong passphrase. Nothing is encrypted with the passphrase,
    /// so a wrong one still gives a seed, just one that derives other keys.
    /// The first stored account's key is derived again from this seed and
    /// compared with the stored one. A wallet with no account yet has
    /// nothing to compare with, so it passes. Without a passphrase there is
    /// nothing to verify, because AES-GCM already refuses a wrong password
    /// or key.
    ///
    /// **Parameters**:
    /// - `vault: &KeyVault` - The wallet whose stored accounts are compared.
    /// - `passphrase: Option<&str>` - The passphrase that was applied, or `None`.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Nothing when the passphrase is right or can't be checked yet, or an error message when it is wrong.
    fn verify_passphrase(&self, vault: &KeyVault, passphrase: Option<&str>) -> Result<(), String> {
        if passphrase.is_none() {
            return Ok(());
        }
        let accounts = KeyVault::get_singlesig_accounts(vault.wallet_id)?;
        let Some(first) = accounts.first() else {
            return Ok(());
        };
        let (pub_key, _) = self.derive_spx_keys(first.index)?;
        if pub_key.as_ref() != first.config.signers()[0].pubkey() {
            let msg = "Wrong passphrase. It produced an account this wallet doesn't have.";
            tracing::error!("{} (wallet_id={})", msg, vault.wallet_id);
            return Err(msg.to_string());
        }
        Ok(())
    }
}
