//! The address-binding event: its wire shape, its hash, and the challenge
//! the wallet signs for each address.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::hash::HashBuilder;

/// The only event kind this wallet signs.
pub const EVENT_TYPE: &str = "address_bindings";

/// Domain tag of the account id derivation (rule 6).
const USER_ID_DOMAIN: &str = "ckb-dao-v2/user_id/v1";

/// A binding event as the DAO server sends it and takes it back. The field
/// set and names are the backend's `AddressBindingEvent`; the proofs and
/// `bind_signatures` sit outside the hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingEvent {
    pub event_type: String,
    pub event_hash: String,
    pub user_id: String,
    pub account_pubkey: String,
    pub user_proof: Option<String>,
    pub server_proof: Option<String>,
    pub ckb_block_height: i64,
    pub seen_leaf_count: i64,
    pub seen_mmr_root: String,
    pub ckb_addresses: Vec<String>,
    pub bind_signatures: Vec<String>,
    pub is_binding: bool,
    pub created_at: NaiveDateTime,
    pub expired_at: NaiveDateTime,
}

impl BindingEvent {
    /// The event hash computed from the fields, in the backend's order.
    /// The kind is this wallet's constant, not the server's `event_type`,
    /// so a server cannot pass off another kind of event under this hash.
    /// Times hash in chrono's `NaiveDateTime` Display form, which is the
    /// backend's form too.
    pub fn compute_hash(&self) -> String {
        let mut builder = HashBuilder::new()
            .str(EVENT_TYPE)
            .str(&self.user_id)
            .str(&self.account_pubkey)
            .i64(self.ckb_block_height)
            .count(self.ckb_addresses.len());
        for address in &self.ckb_addresses {
            builder = builder.str(address);
        }
        builder
            .byte(u8::from(self.is_binding))
            .str(&self.created_at.to_string())
            .str(&self.expired_at.to_string())
            .i64(self.seen_leaf_count)
            .str(&self.seen_mmr_root)
            .digest()
    }
}

/// The message an address's key signs: the hex digest of the event hash and
/// the address, built the backend's way. The signature covers the 64 ASCII
/// characters of this hex string, which is what the DAO server verifies.
pub fn challenge(event_hash: &str, address: &str) -> String {
    HashBuilder::new().str(event_hash).str(address).digest()
}

/// The account id a key owns (rule 6): the first 16 bytes of the hash of
/// the domain tag and the lowercased key, in lowercase 8-4-4-4-12 form.
pub fn derive_user_id(account_pubkey: &str) -> String {
    let digest = HashBuilder::new()
        .str(USER_ID_DOMAIN)
        .str(&account_pubkey.to_lowercase())
        .digest();
    format!(
        "{}-{}-{}-{}-{}",
        &digest[0..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f").unwrap()
    }

    /// The binding fixture the DAO backend, portal and v1 wallet all pin.
    pub(crate) fn fixture() -> BindingEvent {
        let mut event = BindingEvent {
            event_type: EVENT_TYPE.to_string(),
            event_hash: String::new(),
            user_id: "123e4567-e89b-12d3-a456-426614174000".to_string(),
            account_pubkey: "aa".repeat(32),
            user_proof: None,
            server_proof: None,
            ckb_block_height: 12_345_678,
            seen_leaf_count: 3,
            seen_mmr_root: "cd".repeat(32),
            ckb_addresses: vec!["ckt1qexample1".to_string(), "ckt1qexample2".to_string()],
            bind_signatures: Vec::new(),
            is_binding: true,
            created_at: at("2026-01-02 03:04:05"),
            expired_at: at("2026-01-02 03:04:25"),
        };
        event.event_hash = event.compute_hash();
        event
    }

    #[test]
    fn the_event_hash_matches_the_backend() {
        assert_eq!(
            fixture().compute_hash(),
            "ffda98ad335b9b7f3a8a640f3d805dcc490a8125a6b346acfcbc25f21975bac2"
        );
    }

    /// Real server timestamps carry microseconds; the backend's hash of this
    /// event with them is pinned here, read from the server's wire form.
    #[test]
    fn a_server_event_with_microseconds_hashes_like_the_backend() {
        let mut json = serde_json::to_value(fixture()).unwrap();
        json["created_at"] = "2026-01-02T03:04:05.123456".into();
        json["expired_at"] = "2026-01-02T03:04:25.123456".into();
        let event: BindingEvent = serde_json::from_value(json).unwrap();
        assert_eq!(
            event.compute_hash(),
            "525e39f470650c0829d681e377dc98ba908b969104f63696d7106aa6914b86f9"
        );
    }

    #[test]
    fn the_challenge_matches_the_backend() {
        assert_eq!(
            challenge(&fixture().event_hash, "ckt1qexample1"),
            "dbc204e51f5172ae877c6bc549fc743d531bd7de6b90565b80a08b0a4d841394"
        );
    }

    #[test]
    fn the_account_id_matches_the_backend() {
        assert_eq!(
            derive_user_id(&"aa".repeat(32)),
            "6ad556ea-3228-4baa-f0d0-0b00a70be16c"
        );
        assert_eq!(
            derive_user_id(&"AA".repeat(32)),
            derive_user_id(&"aa".repeat(32))
        );
    }
}
