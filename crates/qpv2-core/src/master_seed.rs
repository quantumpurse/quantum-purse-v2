//! The master seed, opened for key derivation.

use fips205::{
    traits::{KeyGen, SerDes},
    *,
};
use scrypt::{scrypt, Params};
use zeroize::Zeroize;

use crate::constants::{KDF_PATH_PREFIX, PASSPHRASE_SALT_PREFIX, PASSPHRASE_SCRYPT};
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
        Self::apply_passphrase(&mut seed, passphrase)?;

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

    /// Mixes an optional passphrase into the master seed in place, turning it
    /// into the seed that keys are derived from.
    ///
    /// The recipe, which can never change once a passphrase wallet holds funds:
    ///
    /// ```text
    /// new seed = scrypt(password = seed,
    ///                   salt     = PASSPHRASE_SALT_PREFIX || passphrase,
    ///                   cost     = PASSPHRASE_SCRYPT (log_n, r, p),
    ///                   output length = seed length)
    /// ```
    ///
    /// - No passphrase (`None`) leaves the seed unchanged, so a wallet without
    ///   one keeps its keys.
    /// - An empty passphrase (`Some("")`) is refused, so "no passphrase" has
    ///   one spelling, `None`, and a caller has to choose it on purpose.
    /// - The whole seed is replaced, not only its two secret parts. The third
    ///   part becomes the public half of every key, so leaving it alone would
    ///   link a passphrase wallet to the plain one on-chain.
    /// - Scrypt makes each passphrase guess slow for someone who holds the seed.
    /// - Only printable ASCII is accepted. Other characters can be typed as
    ///   different bytes on different systems, which would open a different,
    ///   empty wallet.
    ///
    /// **Parameters**:
    /// - `seed: &mut [u8]` - The master seed, overwritten with the new seed of the same length.
    /// - `passphrase: Option<&str>` - The passphrase, or `None` for a wallet without one.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Nothing on success, or an error message on failure, in which case the seed is untouched.
    ///
    /// Warning: Proper zeroization of the inputs is the responsibility of the caller.
    fn apply_passphrase(seed: &mut [u8], passphrase: Option<&str>) -> Result<(), String> {
        if seed.is_empty() {
            return Err("The master seed is empty.".to_string());
        }
        let Some(passphrase) = passphrase else {
            return Ok(());
        };
        if passphrase.is_empty() {
            return Err("The passphrase is empty.".to_string());
        }
        // Every character must be printable ASCII, space to '~'. The error does
        // not name the one that isn't, because it is part of a secret.
        if !passphrase.chars().all(|c| matches!(c, ' '..='~')) {
            return Err(
                "The passphrase may only use English letters, digits, spaces and common symbols."
                    .to_string(),
            );
        }

        let mut salt = SecureVec::from_vec(PASSPHRASE_SALT_PREFIX.to_vec());
        salt.extend(SecureVec::from_vec(passphrase.as_bytes().to_vec()));

        // Scrypt fills the whole buffer it is given, so the output length is the
        // seed's and not `PASSPHRASE_SCRYPT.len`.
        let scrypt_param = Params::new(
            PASSPHRASE_SCRYPT.log_n,
            PASSPHRASE_SCRYPT.r,
            PASSPHRASE_SCRYPT.p,
            PASSPHRASE_SCRYPT.len,
        )
        .map_err(|e| format!("Scrypt params error: {:?}", e))?;
        let mut new_seed = SecureVec::new_with_length(seed.len());
        scrypt(seed, &salt, &scrypt_param, &mut new_seed)
            .map_err(|e| format!("Scrypt error: {:?}", e))?;
        seed.copy_from_slice(&new_seed);
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_passphrase_keeps_the_seed() {
        let mut seed: Vec<u8> = (0..96).collect();
        MasterSeed::apply_passphrase(&mut seed, None).unwrap();
        assert_eq!(seed, (0..96).collect::<Vec<u8>>());
    }

    #[test]
    fn test_empty_passphrase_is_rejected() {
        let mut seed: Vec<u8> = (0..96).collect();
        assert_eq!(
            MasterSeed::apply_passphrase(&mut seed, Some("")),
            Err("The passphrase is empty.".to_string())
        );
    }

    #[test]
    fn test_passphrase_rejects_an_empty_seed() {
        for passphrase in [None, Some("correct horse battery staple")] {
            assert_eq!(
                MasterSeed::apply_passphrase(&mut [], passphrase),
                Err("The master seed is empty.".to_string())
            );
        }
    }

    /// Pins the recipe: a change to the salt prefix, the scrypt cost, or what
    /// goes in as password and salt moves every passphrase wallet's addresses.
    /// The 96-byte seed is the longest one, past the 64 bytes scrypt's own
    /// parameter check allows.
    #[test]
    fn test_passphrase_recipe_is_pinned() {
        let mut seed: Vec<u8> = (0..96).collect();
        MasterSeed::apply_passphrase(&mut seed, Some("correct horse battery staple")).unwrap();
        assert_eq!(
            hex::encode(&seed),
            "7283cb0dcbfb83e7026a463fb000422ebcd68fab21b35a01471af0a4006a707f\
             5d652708dd5592b4331aedfe0291428bb838b12ad31cb633b4a1cdd821e42b76\
             2e29e22db3a257b23688f3ce247d5f5f4c40b8c92b5de4de994877a9828ab314"
        );
    }

    #[test]
    fn test_passphrase_rejects_characters_outside_printable_ascii() {
        let mut seed: Vec<u8> = (0..96).collect();
        for typed in ["caf\u{e9}", "tab\there", "line\nbreak"] {
            assert!(
                MasterSeed::apply_passphrase(&mut seed, Some(typed)).is_err(),
                "{:?} was accepted",
                typed
            );
        }
    }
}
