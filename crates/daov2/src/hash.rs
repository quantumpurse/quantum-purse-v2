//! The field encoding every DAO v2 hash uses (rule 3).

use sha2::{Digest, Sha256};

/// SHA-256 over fields in DAO v2's encoding. A variable-length field is its
/// u32 little-endian byte length followed by its bytes, so no two different
/// field lists produce the same bytes; fixed-width fields are written raw.
pub(crate) struct HashBuilder(Sha256);

impl HashBuilder {
    pub(crate) fn new() -> Self {
        Self(Sha256::new())
    }

    pub(crate) fn str(mut self, value: &str) -> Self {
        self.0.update((value.len() as u32).to_le_bytes());
        self.0.update(value.as_bytes());
        self
    }

    /// A list's element count, written before its elements.
    pub(crate) fn count(mut self, n: usize) -> Self {
        self.0.update((n as u32).to_le_bytes());
        self
    }

    pub(crate) fn byte(mut self, value: u8) -> Self {
        self.0.update([value]);
        self
    }

    pub(crate) fn i64(mut self, value: i64) -> Self {
        self.0.update(value.to_le_bytes());
        self
    }

    /// The lowercase hex digest.
    pub(crate) fn digest(self) -> String {
        hex::encode(self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::HashBuilder;

    /// Computed by the DAO backend's `HashBuilder`: the length prefix keeps
    /// "AB" ‖ "C" apart from "A" ‖ "BC".
    #[test]
    fn strings_are_length_prefixed_like_the_backend() {
        assert_eq!(
            HashBuilder::new().str("AB").str("C").digest(),
            "b8ead03f07faa107f4cdd0566864e3deb2d5c2a2de00e4f2cb22b1aebdd86f21"
        );
    }
}
