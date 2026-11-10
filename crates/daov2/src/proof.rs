//! The DAO server's Schnorr proofs: `server_proof` over an event hash, and
//! the append ack's attestation over the event's place in the archive.

use secp256k1::{schnorr::Signature, Secp256k1, XOnlyPublicKey};
use sha2::{Digest, Sha256};

use crate::client::AppendAck;
use crate::event::BindingEvent;

/// Checks that `proof` (128 hex signature ‖ 64 hex x-only key) is
/// `server_pubkey`'s BIP-340 signature over the bytes of the hex string
/// `message`, the way the DAO server signs.
pub fn verify_proof(proof: &str, message: &str, server_pubkey: &str) -> Result<(), String> {
    if proof.len() != 192 {
        return Err(format!(
            "A server proof is 192 hex characters; this one has {}.",
            proof.len()
        ));
    }
    let (sig_hex, key_hex) = proof.split_at(128);
    if !key_hex.eq_ignore_ascii_case(server_pubkey) {
        return Err(format!(
            "The proof was signed by an unknown key {key_hex}, not the DAO server's {server_pubkey}."
        ));
    }
    let signature = hex::decode(sig_hex)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or_else(|| "The server proof's signature is not valid hex.".to_string())?;
    let key = hex::decode(key_hex)
        .ok()
        .and_then(|bytes| XOnlyPublicKey::from_slice(&bytes).ok())
        .ok_or_else(|| "The server proof's key is not a valid x-only key.".to_string())?;
    Secp256k1::verification_only()
        .verify_schnorr(&signature, message.as_bytes(), &key)
        .map_err(|_| "The server's signature does not verify against its key.".to_string())
}

/// Rule 4: SHA-256 of `leaf_index` (u64 little-endian) ‖ the event hash ‖
/// the archive root, as lowercase hex.
pub fn checkpoint_digest(
    leaf_index: u64,
    event_hash: &str,
    mmr_root: &str,
) -> Result<String, String> {
    let hash32 = |hex_str: &str, what: &str| {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(hex_str, &mut bytes)
            .map_err(|_| format!("The {what} is not 64 hex characters: {hex_str}"))?;
        Ok::<_, String>(bytes)
    };
    let mut hasher = Sha256::new();
    hasher.update(leaf_index.to_le_bytes());
    hasher.update(hash32(event_hash, "event hash")?);
    hasher.update(hash32(mmr_root, "archive root")?);
    Ok(hex::encode(hasher.finalize()))
}

/// Security assumption 1: the server's ack must commit to this very event at
/// a place in the archive at or after the length the event saw (rule 17),
/// signed with the server's key.
pub fn verify_ack(
    event: &BindingEvent,
    ack: &AppendAck,
    server_pubkey: &str,
) -> Result<(), String> {
    if ack.leaf_index < event.seen_leaf_count.max(0) {
        return Err(format!(
            "The server placed the event at leaf {}, before the {} leaves it said the archive held.",
            ack.leaf_index, event.seen_leaf_count
        ));
    }
    let digest = checkpoint_digest(ack.leaf_index as u64, &event.event_hash, &ack.mmr_root)?;
    verify_proof(&ack.attestation, &digest, server_pubkey)
        .map_err(|e| format!("The server's ack does not verify: {e}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use secp256k1::Keypair;

    pub(crate) const SERVER_SEED: [u8; 32] = [0x11; 32];

    /// A proof in the server's format: signature ‖ x-only key, as hex.
    pub(crate) fn sign(seed: &[u8; 32], message: &str) -> String {
        let secp = Secp256k1::new();
        let keypair = Keypair::from_seckey_slice(&secp, seed).unwrap();
        let signature = secp.sign_schnorr_no_aux_rand(message.as_bytes(), &keypair);
        format!(
            "{}{}",
            hex::encode(signature.to_byte_array()),
            hex::encode(keypair.x_only_public_key().0.serialize())
        )
    }

    pub(crate) fn server_pubkey() -> String {
        sign(&SERVER_SEED, "")[128..].to_string()
    }

    #[test]
    fn a_proof_by_the_server_verifies() {
        let proof = sign(&SERVER_SEED, "message");
        assert!(verify_proof(&proof, "message", &server_pubkey()).is_ok());
        assert!(verify_proof(&proof, "other", &server_pubkey()).is_err());
    }

    #[test]
    fn a_proof_by_another_key_is_refused() {
        let proof = sign(&[0x22; 32], "message");
        let err = verify_proof(&proof, "message", &server_pubkey()).unwrap_err();
        assert!(err.contains("unknown key"), "{err}");
    }

    /// Computed by the DAO backend's `checkpoint_digest`.
    #[test]
    fn the_checkpoint_digest_matches_the_backend() {
        assert_eq!(
            checkpoint_digest(7, &"01".repeat(32), &"02".repeat(32)).unwrap(),
            "9aa4f8760c93a8b2047fee696a2b3657fb4f9f6214bb671d2e92bb145d4d1c96"
        );
    }

    fn ack(event: &BindingEvent, leaf_index: i64) -> AppendAck {
        let root = "ee".repeat(32);
        let digest = checkpoint_digest(leaf_index as u64, &event.event_hash, &root).unwrap();
        AppendAck {
            bound_addresses: event.ckb_addresses.clone(),
            git_commit_hash: None,
            leaf_index,
            mmr_root: root,
            attestation: sign(&SERVER_SEED, &digest),
        }
    }

    #[test]
    fn an_ack_for_the_event_verifies() {
        let event = crate::event::tests::fixture();
        assert!(verify_ack(&event, &ack(&event, 5), &server_pubkey()).is_ok());
    }

    #[test]
    fn an_ack_before_the_archive_the_event_saw_is_refused() {
        let event = crate::event::tests::fixture();
        let err = verify_ack(&event, &ack(&event, 2), &server_pubkey()).unwrap_err();
        assert!(err.contains("before the 3 leaves"), "{err}");
    }

    #[test]
    fn an_ack_for_another_event_is_refused() {
        let event = crate::event::tests::fixture();
        let mut other = event.clone();
        other.event_hash = "ab".repeat(32);
        assert!(verify_ack(&event, &ack(&other, 5), &server_pubkey()).is_err());
    }
}
