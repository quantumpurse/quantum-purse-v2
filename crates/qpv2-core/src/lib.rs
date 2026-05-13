//! # QuantumPurse KeyVault
//!
//! This module provides a secure authentication interface for managing cryptographic keys in the
//! QuantumPurse project. It leverages AES-GCM for encryption, Scrypt for password hashing, HKDF for key derivation,
//! and the SPHINCS+ signature scheme for post-quantum transaction signing. The master seed is encrypted and stored
//! locally in files, with access authenticated via password (Scrypt) or pre-derived key (e.g. passkey PRF + HKDF).

use bip39::{Language, Mnemonic};
use ckb_fips205_utils::Hasher;
use fips205::{
    traits::{KeyGen, SerDes, Signer, Verifier},
    *,
};
use hex::encode;
use zeroize::Zeroize;

pub mod constants;
mod containers;
pub mod db;
mod macros;
pub mod types;
pub mod utilities;

use crate::constants::{
    KDF_PATH_PREFIX, MULTISIG_RESERVED_FIELD_VALUE, PUBKEY_NUM, REQUIRED_FIRST_N, THRESHOLD,
};
pub use containers::{SecureString, SecureVec};
use types::*;

////////////////////////////////////////////////////////////////////////////////
///  Key-vault functions
////////////////////////////////////////////////////////////////////////////////
pub struct KeyVault {
    /// The one parameter set chosen for QuantumPurse KeyVault setup in all 12 NIST-approved SPHINCS+ FIPS205 variants
    pub variant: SpxVariant,
}

impl KeyVault {
    /// Constructs a new `KeyVault`.
    ///
    /// **Returns**:
    /// - `KeyVault` - A new instance of the struct.
    pub fn new(variant: SpxVariant) -> Self {
        KeyVault { variant }
    }

    /// To derive SPHINCS+ key pair. One master seed can derive multiple child index-based SPHINCS+ key pairs on demand.
    ///
    /// **Parameters**:
    /// - `seed: &[u8]` - The master seed from which the child sphincs+ key is derived. MUST carry at least N*3 bytes of entropy or panics.
    /// - `index: u32` - The index of the child sphincs+ key to be derived.
    ///
    /// **Returns**:
    /// - `Result<(SecureVec, SecureVec), String>` - The SPHINCS+ key pair on success, or an error message on failure.
    ///
    /// Warning: Proper zeroization of the input seed is the responsibility of the caller.
    fn derive_spx_keys(&self, seed: &[u8], index: u32) -> Result<(SecureVec, SecureVec), String> {
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

    /// Validates an authentication key, returning an error if it is empty or uninitialized.
    fn validate_auth(auth: &AuthKey) -> Result<(), String> {
        match auth {
            AuthKey::Password(password) => {
                if password.is_empty() || password.is_uninitialized() {
                    return Err("Password cannot be empty or uninitialized".to_string());
                }
            }
            AuthKey::CryptoKey(key) => {
                if key.is_empty() || key.is_uninitialized() {
                    return Err("Derived key cannot be empty or uninitialized".to_string());
                }
            }
        }
        Ok(())
    }

    /// Clears all data in the vault.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Ok on success, or an error message on failure.
    pub fn clear_database() -> Result<(), String> {
        db::clear_master_seed().map_err(|e| e.to_string())?;
        db::clear_accounts().map_err(|e| e.to_string())?;
        db::clear_wallet_info().map_err(|e| e.to_string())?;
        db::clear_tx_history().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Retrieves the stored wallet variant.
    ///
    /// **Returns**:
    /// - `Result<SpxVariant, String>` - The stored variant on success, or an error if not found.
    pub fn get_spx_variant() -> Result<SpxVariant, String> {
        let wallet_info = db::get_wallet_info()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "Wallet not initialized. Run 'init' or 'import-mnemonic' first.".to_string()
            })?;
        Ok(wallet_info.spx_variant)
    }

    /// Retrieves all SPHINCS+ lock script arguments (processed public keys) from the database in the order they get inserted.
    ///
    /// **Returns**:
    /// - `Result<Vec<String>, String>` - An array of hex-encoded SPHINCS+ lock script arguments on success, or an error on failure.
    pub fn get_all_sphincs_lock_args() -> Result<Vec<String>, String> {
        let accounts = db::get_all_accounts().map_err(|e| e.to_string())?;
        let lock_args_array: Vec<String> = accounts
            .into_iter()
            .map(|account| account.lock_args)
            .collect();
        Ok(lock_args_array)
    }

    /// Check if there's a master seed stored.
    ///
    /// **Returns**:
    /// - `Result<bool, String>` - `true` if a master seed exists, or `false` if it doesn't.
    pub fn has_master_seed() -> Result<bool, String> {
        let payload = db::get_encrypted_seed().map_err(|e| e.to_string())?;
        Ok(payload.is_some())
    }

    /// Generates master seed for your wallet, encrypts it, and stores it.
    /// Returns the BIP39 mnemonic seed phrase for backup.
    /// Errors if the master seed already exists.
    ///
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to encrypt the generated master seed.
    /// - `auth_method: AuthMethod` - The authentication method to record in wallet metadata.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Ok on success, or an error on failure.
    ///
    /// **Security Considerations**:
    ///
    /// Given NIST new security post-quantum standards categorized as:
    /// 1) Key search on a block cipher with a 128-bit key (e.g. AES128)
    /// 3) Key search on a block cipher with a 192-bit key (e.g. AES192)
    /// 5) Key search on a block cipher with a 256-bit key (e.g. AES 256)
    ///
    /// First protection layer: For a symmetrical encryption practice, the first protection effort SHOULD be the responsibility of
    /// the higher layer implementation (Quantum Purse Wallet or any other system using this library) to ensure that the encrypted data
    /// is never exposed. It is also the responsibility of the end-users to always lock their device carefully.
    ///
    /// Second protection layer: Should the first protection layer fail in any situation, the encryption itself stands as the last
    /// resistance against quantum attacks. The passwords provided should be strong enough, so that breaking it requires comparable
    /// resource to break the NIST category level 1), 3) and 5).
    ///
    /// For a reference setup:
    ///  - Minimum required 20-character passwords. This puts us at ~128-bit of security in theory (less in reality because of human factors).
    ///  - Scrypt with param {log_n = 17, r = 8, p = 1, len 32} make each effort to guess a password even harder for the attacker.
    ///
    /// The theoretical security for this setup, thus starts at level 1) and is not upper limited following how long users passwords can be.
    pub fn generate_master_seed(
        &self,
        auth: AuthKey,
        auth_method: AuthMethod,
    ) -> Result<(), String> {
        Self::validate_auth(&auth)?;

        if Self::has_master_seed()? {
            return Err("Master seed already exists".to_string());
        }

        let size = self.variant.required_entropy_size_total();
        let entropy = utilities::get_random_bytes(size)
            .map_err(|e| format!("Failed generating master seed: {}", e))?;
        let encrypted_seed = utilities::encrypt(&auth, entropy.as_ref())
            .map_err(|e| format!("Encryption error: {}", e))?;

        db::set_encrypted_seed(encrypted_seed).map_err(|e| e.to_string())?;

        let wallet_info = types::WalletInfo {
            spx_variant: self.variant,
            auth_method,
        };
        db::set_wallet_info(wallet_info).map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Generates a new SPHINCS+ account - a SPHINCS+ child account derived from the master seed and stores it.
    ///
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to decrypt the master seed.
    ///
    /// **Returns**:
    /// - `Result<String, String>` - The hex-encoded SPHINCS+ lock argument (processed SPHINCS+ public key) of the account on success, or an error on failure.
    pub fn gen_new_account(&self, auth: AuthKey) -> Result<String, String> {
        Self::validate_auth(&auth)?;

        // Get and decrypt the master seed
        let payload = db::get_encrypted_seed()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Master seed not found".to_string())?;
        let seed = utilities::decrypt(&auth, payload)?;

        let index = Self::get_all_sphincs_lock_args()?.len() as u32;
        let (pub_key, _) = self
            .derive_spx_keys(&seed, index)
            .map_err(|e| format!("Key derivation error: {}", e))?;

        // Calculate lock script args and encrypt corresponding private key
        let lock_script_args = self.get_lock_script_arg(&pub_key);

        // Store to DB
        let account = SphincsPlusAccount {
            index: 0, // Init to 0; Will be set correctly in add_account
            lock_args: encode(lock_script_args),
        };

        db::add_account(account).map_err(|e| e.to_string())?;

        Ok(encode(lock_script_args))
    }

    /// Imports master seed from a mnemonic phrase, encrypts it, and stores it.
    /// Overwrites the existing master seed.
    ///
    /// **Parameters**:
    /// - `seed_phrase: SecureString` - The mnemonic phrase to import.
    ///   There're only 3 options accepted: 36, 54 or 72 words.
    /// - `auth: AuthKey` - The authentication key used to encrypt the translated master seed.
    /// - `auth_method: AuthMethod` - The authentication method to record in wallet metadata.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Ok on success, or an error on failure.
    ///
    /// **Notes**:
    /// - The provided `auth` and `seed_phrase` buffers are cleared immediately after use.
    ///
    /// **Security Considerations**:
    ///
    /// Given NIST new security post-quantum standards categorized as:
    /// 1) Key search on a block cipher with a 128-bit key (e.g. AES128)
    /// 3) Key search on a block cipher with a 192-bit key (e.g. AES192)
    /// 5) Key search on a block cipher with a 256-bit key (e.g. AES 256)
    ///
    /// First protection layer: For a symmetrical encryption practice, the first protection effort SHOULD be the responsibility of
    /// the higher layer implementation (Quantum Purse Wallet or any other system using this library) to ensure that the encrypted data
    /// is never exposed. It is also the responsibility of the end-users to always lock their device carefully.
    ///
    /// Second protection layer: Should the first protection layer fail in any situation, the encryption itself stands as the last
    /// resistance against quantum attacks. The passwords provided should be strong enough, so that breaking it requires comparable
    /// resource to break the NIST category level 1), 3) and 5).
    ///
    /// For a reference setup:
    ///  - Minimum required 20-character passwords. This puts us at ~128-bit of security in theory (less in reality because of human factors).
    ///  - Scrypt with param {log_n = 17, r = 8, p = 1, len 32} make each effort to guess a password even harder for the attacker.
    ///
    /// The theoretical security for this setup, thus starts at level 1) and is not upper limited following how long users passwords can be.
    pub fn import_seed_phrase(
        &self,
        seed_phrase: SecureString,
        auth: AuthKey,
        auth_method: AuthMethod,
    ) -> Result<(), String> {
        Self::validate_auth(&auth)?;

        if seed_phrase.is_empty() || seed_phrase.is_uninitialized() {
            return Err("Seed phrase cannot be empty or uninitialized".to_string());
        }

        let words: Vec<&str> = seed_phrase.split_whitespace().collect();
        let word_count = words.len();

        if word_count != self.variant.required_bip39_size_in_word_total() {
            return Err(format!(
                "Mismatch: The chosen SPHINCS+ parameter set {} requires {} words whereas the input mnemonic has {} words.",
                self.variant,
                self.variant.required_bip39_size_in_word_total(),
                word_count
            ));
        }

        let mut combined_entropy = SecureVec::new_with_length(0);
        let size = self.variant.required_bip39_size_in_word_component();
        for (index, chunk) in (0_u8..).zip(words.chunks(size)) {
            let chunk_str = SecureString::from_string(chunk.join(" "));
            let mnemonic = Mnemonic::parse_in(Language::English, &*chunk_str)
                .map_err(|e| format!("Invalid mnemonic: Chunk{} index {}: {}", size, index, e))?;
            combined_entropy.extend(SecureVec::from_vec(mnemonic.to_entropy()));
        }

        let payload = utilities::encrypt(&auth, &combined_entropy)?;
        db::set_encrypted_seed(payload).map_err(|e| e.to_string())?;

        let wallet_info = types::WalletInfo {
            spx_variant: self.variant,
            auth_method,
        };
        db::set_wallet_info(wallet_info).map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Exports the master seed in the form of a custom bip39 mnemonic phrase. There're only 3 options: 36, 54 or 72 words.
    ///
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to decrypt the master seed.
    ///
    /// **Returns**:
    /// - `Result<SecureString, String>` - The mnemonic phrase on success, or an error on failure.
    ///
    /// **Warning**: Exporting the mnemonic exposes it and may pose a security risk.
    pub fn export_seed_phrase(&self, auth: AuthKey) -> Result<SecureString, String> {
        Self::validate_auth(&auth)?;

        let payload = db::get_encrypted_seed()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Master seed not found".to_string())?;

        let entropy = utilities::decrypt(&auth, payload)?;
        let size = self.variant.required_entropy_size_component();
        let chunks = entropy.chunks(size);

        let mut combined_mnemonic = SecureString::new();
        for chunk in chunks {
            let mnemonic = Mnemonic::from_entropy_in(Language::English, chunk)
                .map_err(|e| format!("Export seed error: {}", e))?;
            for word in mnemonic.words() {
                combined_mnemonic.extend(word); //TODO: Pre-allocate SecureString capacity to prevent push_str reallocation from leaking unzeroized copies of the mnemonic in freed heap memory.
            }
        }

        Ok(combined_mnemonic)
    }

    /// Sign and produce a valid signature for the CKB Blockchain Quantum Resistant Lock Script.
    ///
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to decrypt the private key.
    /// - `lock_args: String` - The hex-encoded lock script's arguments corresponding to the SPHINCS+ public key of the account that signs.
    /// - `message: Vec<u8>` - The CKB transaction message all. For details check https://github.com/xxuejie/rfcs/blob/cighash-all/rfcs/0000-ckb-tx-message-all/0000-ckb-tx-message-all.md
    ///
    /// **Returns**:
    /// - `Result<Vec<u8>, String>` - The signature on success, or an error on failure.
    pub fn ckb_sign(
        &self,
        auth: AuthKey,
        lock_args: String,
        message: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        Self::validate_auth(&auth)?;

        let account = db::get_account(&lock_args)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Account not found".to_string())?;

        // Get and decrypt the master seed
        let payload = db::get_encrypted_seed()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Master seed not found".to_string())?;
        let seed = utilities::decrypt(&auth, payload)?;

        let (_, pri_key) = self.derive_spx_keys(&seed, account.index)?;

        match self.variant {
            SpxVariant::Sha2128S => {
                ckb_spx_sign!(slh_dsa_sha2_128s, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2128F => {
                ckb_spx_sign!(slh_dsa_sha2_128f, pri_key, &message, self.variant)
            }
            SpxVariant::Shake128S => {
                ckb_spx_sign!(slh_dsa_shake_128s, pri_key, &message, self.variant)
            }
            SpxVariant::Shake128F => {
                ckb_spx_sign!(slh_dsa_shake_128f, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2192S => {
                ckb_spx_sign!(slh_dsa_sha2_192s, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2192F => {
                ckb_spx_sign!(slh_dsa_sha2_192f, pri_key, &message, self.variant)
            }
            SpxVariant::Shake192S => {
                ckb_spx_sign!(slh_dsa_shake_192s, pri_key, &message, self.variant)
            }
            SpxVariant::Shake192F => {
                ckb_spx_sign!(slh_dsa_shake_192f, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2256S => {
                ckb_spx_sign!(slh_dsa_sha2_256s, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2256F => {
                ckb_spx_sign!(slh_dsa_sha2_256f, pri_key, &message, self.variant)
            }
            SpxVariant::Shake256S => {
                ckb_spx_sign!(slh_dsa_shake_256s, pri_key, &message, self.variant)
            }
            SpxVariant::Shake256F => {
                ckb_spx_sign!(slh_dsa_shake_256f, pri_key, &message, self.variant)
            }
        }
    }

    /// Raw SPHINCS+ sign
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to decrypt the private key.
    /// - `lock_args: String` - The hex-encoded lock script's arguments corresponding to the SPHINCS+ public key of the account that signs.
    /// - `message: Vec<u8>` - The CKB transaction message all. For details check https://github.com/xxuejie/rfcs/blob/cighash-all/rfcs/0000-ckb-tx-message-all/0000-ckb-tx-message-all.md
    ///
    /// **Returns**:
    /// - `Result<(Vec<u8>, Vec<u8>), String>` - A tuple of (signature, public_key) on success, or an error on failure.
    pub fn raw_sign(
        &self,
        auth: AuthKey,
        lock_args: String,
        message: Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        Self::validate_auth(&auth)?;

        let account = db::get_account(&lock_args)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Account not found".to_string())?;

        // Get and decrypt the master seed
        let payload = db::get_encrypted_seed()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Master seed not found".to_string())?;
        let seed = utilities::decrypt(&auth, payload)?;

        let (_, pri_key) = self.derive_spx_keys(&seed, account.index)?;

        match self.variant {
            SpxVariant::Sha2128S => {
                raw_spx_sign!(slh_dsa_sha2_128s, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2128F => {
                raw_spx_sign!(slh_dsa_sha2_128f, pri_key, &message, self.variant)
            }
            SpxVariant::Shake128S => {
                raw_spx_sign!(slh_dsa_shake_128s, pri_key, &message, self.variant)
            }
            SpxVariant::Shake128F => {
                raw_spx_sign!(slh_dsa_shake_128f, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2192S => {
                raw_spx_sign!(slh_dsa_sha2_192s, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2192F => {
                raw_spx_sign!(slh_dsa_sha2_192f, pri_key, &message, self.variant)
            }
            SpxVariant::Shake192S => {
                raw_spx_sign!(slh_dsa_shake_192s, pri_key, &message, self.variant)
            }
            SpxVariant::Shake192F => {
                raw_spx_sign!(slh_dsa_shake_192f, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2256S => {
                raw_spx_sign!(slh_dsa_sha2_256s, pri_key, &message, self.variant)
            }
            SpxVariant::Sha2256F => {
                raw_spx_sign!(slh_dsa_sha2_256f, pri_key, &message, self.variant)
            }
            SpxVariant::Shake256S => {
                raw_spx_sign!(slh_dsa_shake_256s, pri_key, &message, self.variant)
            }
            SpxVariant::Shake256F => {
                raw_spx_sign!(slh_dsa_shake_256f, pri_key, &message, self.variant)
            }
        }
    }

    /// Raw SPHINCS+ verify
    /// **Parameters**:
    /// - `public_key: &[u8]` - SPHINCS+ public key associated with the signature.
    /// - `message: &[u8]` - The message from which the signature was created.
    /// - `signature: &[u8]` - The SPHINCS+ signature to verify.
    ///
    /// **Returns**:
    /// - `Result<bool, String>` - A tuple of (signature, public_key) on success, or an error on failure.
    pub fn raw_verify(
        &self,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<bool, String> {
        match self.variant {
            SpxVariant::Sha2128S => {
                raw_spx_verify!(slh_dsa_sha2_128s, public_key, message, signature)
            }
            SpxVariant::Sha2128F => {
                raw_spx_verify!(slh_dsa_sha2_128f, public_key, message, signature)
            }
            SpxVariant::Shake128S => {
                raw_spx_verify!(slh_dsa_shake_128s, public_key, message, signature)
            }
            SpxVariant::Shake128F => {
                raw_spx_verify!(slh_dsa_shake_128f, public_key, message, signature)
            }
            SpxVariant::Sha2192S => {
                raw_spx_verify!(slh_dsa_sha2_192s, public_key, message, signature)
            }
            SpxVariant::Sha2192F => {
                raw_spx_verify!(slh_dsa_sha2_192f, public_key, message, signature)
            }
            SpxVariant::Shake192S => {
                raw_spx_verify!(slh_dsa_shake_192s, public_key, message, signature)
            }
            SpxVariant::Shake192F => {
                raw_spx_verify!(slh_dsa_shake_192f, public_key, message, signature)
            }
            SpxVariant::Sha2256S => {
                raw_spx_verify!(slh_dsa_sha2_256s, public_key, message, signature)
            }
            SpxVariant::Sha2256F => {
                raw_spx_verify!(slh_dsa_sha2_256f, public_key, message, signature)
            }
            SpxVariant::Shake256S => {
                raw_spx_verify!(slh_dsa_shake_256s, public_key, message, signature)
            }
            SpxVariant::Shake256F => {
                raw_spx_verify!(slh_dsa_shake_256f, public_key, message, signature)
            }
        }
    }

    /// Supporting wallet recovery - quickly derives a list of lock script arguments (processed public keys).
    ///
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to decrypt the master seed.
    /// - `start_index: u32` - The starting index for derivation.
    /// - `count: u32` - The number of sequential lock scripts arguments to derive.
    ///
    /// **Returns**:
    /// - `Result<Vec<String>, String>` - A list of lock script arguments on success, or an error on failure.
    pub fn try_gen_account_batch(
        &self,
        auth: AuthKey,
        start_index: u32,
        count: u32,
    ) -> Result<Vec<String>, String> {
        Self::validate_auth(&auth)?;

        // Get and decrypt the master seed
        let payload = db::get_encrypted_seed()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Master seed not found".to_string())?;
        let seed = utilities::decrypt(&auth, payload)?;
        let mut lock_args_array: Vec<String> = Vec::new();
        for index in start_index..(start_index + count) {
            let (pub_key, _) = self
                .derive_spx_keys(&seed, index)
                .map_err(|e| format!("Key derivation error: {}", e))?;

            // Calculate lock script args
            let lock_script_args = self.get_lock_script_arg(&pub_key);
            lock_args_array.push(encode(lock_script_args));
        }
        Ok(lock_args_array)
    }

    /// Supporting wallet recovery - Recovers the wallet by deriving and storing private keys for the first N accounts.
    ///
    /// **Parameters**:
    /// - `auth: AuthKey` - The authentication key used to decrypt the master seed.
    /// - `count: u32` - The number of accounts to recover (from index 0 to count-1).
    ///
    /// **Returns**:
    /// - `Result<Vec<String>, String>` - A list of newly generated sphincs+ lock script arguments (processed public keys) on success, or an error on failure.
    pub fn recover_accounts(&self, auth: AuthKey, count: u32) -> Result<Vec<String>, String> {
        Self::validate_auth(&auth)?;

        // Get and decrypt the master seed
        let payload = db::get_encrypted_seed()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Master seed not found".to_string())?;
        let mut lock_args_array: Vec<String> = Vec::new();
        let seed = utilities::decrypt(&auth, payload)?;
        for index in 0..count {
            let (pub_key, _) = self
                .derive_spx_keys(&seed, index)
                .map_err(|e| format!("Key derivation error: {}", e))?;

            // Calculate lock script args and encrypt corresponding private key
            let lock_script_args = self.get_lock_script_arg(&pub_key);
            // Store to DB
            let account = SphincsPlusAccount {
                index: 0, // Init to 0; Will be set correctly in add_account
                lock_args: encode(lock_script_args),
            };
            lock_args_array.push(encode(lock_script_args));

            db::add_account(account).map_err(|e| e.to_string())?;
        }
        Ok(lock_args_array)
    }

    /// Checks whether a wallet exists (i.e. a master seed is stored).
    ///
    /// **Returns**:
    /// - `bool` - `true` if a wallet exists.
    pub fn wallet_exists() -> bool {
        Self::has_master_seed().unwrap_or(false)
    }

    /// Reads and returns the stored wallet info.
    ///
    /// **Returns**:
    /// - `Result<WalletInfo, String>` - The wallet info on success, or an error if not found.
    pub fn read_wallet_info() -> Result<WalletInfo, String> {
        db::get_wallet_info()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "Wallet not initialized. Run 'init' or 'import-mnemonic' first.".to_string()
            })
    }

    /// Checks if the wallet's authentication method matches the expected one.
    ///
    /// **Parameters**:
    /// - `expected: &AuthMethod` - The authentication method the caller expects to use.
    ///
    /// **Returns**:
    /// - `Result<(), String>` - Ok if methods match, or an error message explaining the mismatch.
    pub fn check_auth_compatibility(expected: &AuthMethod) -> Result<(), String> {
        let wallet_info = Self::read_wallet_info()?;

        match (&wallet_info.auth_method, expected) {
            (AuthMethod::Password, AuthMethod::Password) => Ok(()),
            (AuthMethod::Keychain, AuthMethod::Keychain) => Ok(()),
            (AuthMethod::Keychain, AuthMethod::Password) => {
                Err("This wallet uses platform credential store authentication and cannot be accessed with a password.".to_string())
            }
            (AuthMethod::Password, AuthMethod::Keychain) => {
                Err("This wallet uses password authentication and cannot be accessed with a credential store.".to_string())
            }
        }
    }

    /// Building CKB SPHINCS+ all-in-one lockscript arguments
    ///
    /// **Parameters**:
    /// - `public_key: &SecureVec` - The SPHINCS+ public key to be used in the lock script.
    ///
    /// **Returns**:
    /// - `[u8; 32]` - The lock script arguments as a byte array.
    fn get_lock_script_arg(&self, public_key: &SecureVec) -> [u8; 32] {
        let all_in_one_config: [u8; 4] = [
            MULTISIG_RESERVED_FIELD_VALUE,
            REQUIRED_FIRST_N,
            THRESHOLD,
            PUBKEY_NUM,
        ];
        let sign_flag: u8 = self.variant << 1;
        let mut script_args_hasher = Hasher::script_args_hasher();
        script_args_hasher.update(&all_in_one_config);
        script_args_hasher.update(&[sign_flag]);
        script_args_hasher.update(public_key);
        script_args_hasher.hash()
    }
}
