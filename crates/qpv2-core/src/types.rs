use crate::containers::{SecureString, SecureVec};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Shl, Shr};

/// Scrypt param structure.
pub struct ScryptParam {
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
    pub len: usize,
}

/// Represents an encrypted payload containing salt, IV, and ciphertext, all hex-encoded.
///
/// **Fields**:
/// - `salt: String` - Hex-encoded salt used for key derivation with Scrypt.
/// - `iv: String` - Hex-encoded initialization vector (nonce) for AES-GCM encryption.
/// - `cipher_text: String` - Hex-encoded encrypted data produced by AES-GCM.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CipherPayload {
    pub salt: String,
    pub iv: String,
    pub cipher_text: String,
}

/// Represents a SPHINCS+ account with the lock script argument (processed public key).
///
/// **Fields**:
/// - `index: u32` - db addition order
/// - `lock_args: String` - The lock script's argument calculated from the SPHINCS+ public key.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SphincsPlusAccount {
    pub index: u32,
    pub lock_args: String,
}

/// Authentication method used to protect the vault.
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(tag = "type")]
pub enum AuthMethod {
    /// Password-based authentication using scrypt key derivation.
    #[default]
    Password,
    /// Platform credential store (macOS Keychain, Windows TPM, Linux TPM).
    Keychain,
    /// FIDO2 hardware key with hmac-secret extension.
    Fido2 { credential_id: String },
}

/// Authentication key used to encrypt/decrypt the vault.
/// Unifies password-based and crypto key paths so that all core functions
/// accept a single parameter regardless of how the key was obtained.
pub enum AuthKey {
    /// Password to be hashed with Scrypt before use as AES-256 key.
    Password(SecureString),
    /// Pre-derived 32-byte AES-256 key (e.g. from passkey PRF + HKDF).
    CryptoKey(SecureVec),
}

/// Represents wallet metadata information.
///
/// **Fields**:
/// - `spx_variant: SpxVariant` - The SPHINCS+ variant used for this wallet.
/// - `auth_method: AuthMethod` - The authentication method protecting this wallet.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalletInfo {
    pub spx_variant: SpxVariant,
    #[serde(default)]
    pub auth_method: AuthMethod,
}

/// ID of all 12 SPHINCS+ variants following https://github.com/cryptape/quantum-resistant-lock-script/pull/14
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpxVariant {
    Sha2128F = 48,
    Sha2128S,
    Sha2192F,
    Sha2192S,
    Sha2256F,
    Sha2256S,
    Shake128F,
    Shake128S,
    Shake192F,
    Shake192S,
    Shake256F,
    Shake256S,
}

impl SpxVariant {
    // Each seed in the SPHINCS+ input seed trio {sk_seed, sk_prf, pk_seed} needs this amount of entropy in byte
    pub fn required_entropy_size_component(&self) -> usize {
        match self {
            Self::Sha2128F | Self::Sha2128S | Self::Shake128F | Self::Shake128S => 16,
            Self::Sha2192F | Self::Sha2192S | Self::Shake192F | Self::Shake192S => 24,
            _ => 32,
        }
    }

    // The whole SPHINCS+ seed backup seed/ the trio {sk_seed, sk_prf, pk_seed} needs this much of entropy in byte
    pub fn required_entropy_size_total(&self) -> usize {
        self.required_entropy_size_component() * 3
    }

    // Mapping each SPHINCS+ variant to the corresponding bip39 type (differentiated by word count)
    // Each word count option below contain the corresponding entropy defined in `required_entropy_size_component`
    pub fn required_bip39_size_in_word_component(&self) -> usize {
        match self {
            Self::Sha2128F | Self::Sha2128S | Self::Shake128F | Self::Shake128S => 12,
            Self::Sha2192F | Self::Sha2192S | Self::Shake192F | Self::Shake192S => 18,
            _ => 24,
        }
    }

    // The whole SPHINCS+ seed backup seed/ the trio {sk_seed, sk_prf, pk_seed} will need this much of words in BIP39 standard
    pub fn required_bip39_size_in_word_total(&self) -> usize {
        self.required_bip39_size_in_word_component() * 3
    }
}

impl fmt::Display for SpxVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SpxVariant::Sha2128F => "Sha2128F",
            SpxVariant::Sha2128S => "Sha2128S",
            SpxVariant::Sha2192F => "Sha2192F",
            SpxVariant::Sha2192S => "Sha2192S",
            SpxVariant::Sha2256F => "Sha2256F",
            SpxVariant::Sha2256S => "Sha2256S",
            SpxVariant::Shake128F => "Shake128F",
            SpxVariant::Shake128S => "Shake128S",
            SpxVariant::Shake192F => "Shake192F",
            SpxVariant::Shake192S => "Shake192S",
            SpxVariant::Shake256F => "Shake256F",
            SpxVariant::Shake256S => "Shake256S",
        };
        write!(f, "{}", s)
    }
}

impl Shr<u8> for SpxVariant {
    type Output = u8;
    fn shr(self, rhs: u8) -> u8 {
        (self as u8) >> rhs
    }
}

impl Shl<u8> for SpxVariant {
    type Output = u8;
    fn shl(self, rhs: u8) -> u8 {
        (self as u8) << rhs
    }
}
