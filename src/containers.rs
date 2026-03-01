use aes_gcm::aead::{Buffer, Error as AeadError};
use std::ops::DerefMut;
use std::ops::{Deref /*DerefMut*/};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::Zeroize;
#[cfg(test)]
pub static ZEROIZED: AtomicBool = AtomicBool::new(false);

/// A secure string type for custom BIP39 menmonic seed words
/// Used in containing BIP39 component/elemental mnemonic word string
/// facilitating custom BIP39 for quantumPurse Keyvault
#[derive(Debug, PartialEq)]
pub struct SecureString(String);

impl SecureString {
    pub fn new() -> Self {
        SecureString(String::new())
    }

    pub fn from_utf8(bytes: Vec<u8>) -> Result<Self, String> {
        match String::from_utf8(bytes) {
            Ok(s) => Ok(SecureString(s)),
            Err(e) => {
                let mut leaked_handle = e.into_bytes();
                leaked_handle.zeroize();
                Err("Invalid UTF-8 input".to_string())
            }
        }
    }

    pub fn from_string(s: String) -> Self {
        SecureString(s)
    }

    /// Notice: Only used in combining mnemonics to mnemonics.
    /// If used in other cases will introduce unexpected outcomes
    pub fn extend(&mut self, s: &str) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(s);
    }

    pub fn is_uninitialized(&self) -> bool {
        self.0.as_bytes().iter().all(|&byte| byte == 0)
    }
}

impl Drop for SecureString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Deref for SecureString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// impl DerefMut for SecureString {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.0
//     }
// }

/// A secure vector that zeroizes its contents when dropped.
/// Used in containing sensitive bytes like passwords or master seed.
#[derive(Debug, PartialEq)]
pub struct SecureVec(Vec<u8>);

impl SecureVec {
    pub fn new_with_length(len: usize) -> Self {
        SecureVec(vec![0u8; len])
    }

    pub fn from_vec(vec: Vec<u8>) -> Self {
        SecureVec(vec)
    }

    pub fn extend(&mut self, other: SecureVec) {
        self.0.extend_from_slice(&other);
    }

    pub fn is_uninitialized(&self) -> bool {
        self.0.iter().all(|&byte| byte == 0)
    }
}

impl Zeroize for SecureVec {
    fn zeroize(&mut self) {
        self.0.zeroize();
        #[cfg(test)]
        ZEROIZED.store(true, Ordering::SeqCst);
    }
}

// impl ZeroizeOnDrop for SecureVec {}
impl Drop for SecureVec {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl Deref for SecureVec {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SecureVec {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRef<[u8]> for SecureVec {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for SecureVec {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl Buffer for SecureVec {
    fn extend_from_slice(&mut self, other: &[u8]) -> Result<(), AeadError> {
        self.0.extend_from_slice(other);
        Ok(())
    }

    fn truncate(&mut self, len: usize) {
        if len < self.0.len() {
            use zeroize::Zeroize;
            self.0[len..].zeroize();
        }
        self.0.truncate(len);
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}
