//! Everything the wallet checks in the server's event before it signs.
//! A failed check refuses to sign; nothing here asks the server anything.

use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::event::{derive_user_id, BindingEvent};
use crate::proof::verify_proof;

/// Rule 15: the longest an event may stay open for signing.
pub const CO_SIGN_WINDOW_SECS: i64 = 20;
/// Rule 15: how far the server's `created_at` may be from this device's clock.
pub const MAX_CLOCK_SKEW_MS: i64 = 1_000;
/// Rule 15: how far the event's block height may be from this wallet's tip.
pub const TOL_BLOCKS: u64 = 2;

/// What the wallet knows on its own, without the server.
pub struct Expected<'a> {
    /// The key in the API key the user pasted.
    pub account_pubkey: &'a str,
    /// The addresses the user chose.
    pub addresses: &'a [String],
    /// The server's key, pinned on first use.
    pub server_pubkey: &'a str,
}

/// Runs every check, in order, and returns the first failure. `received_at`
/// is when the event reached this device; `local_tip` is the chain tip this
/// wallet's own node shows, `None` if it has none yet.
pub fn verify_event(
    event: &BindingEvent,
    expected: &Expected,
    received_at: DateTime<Utc>,
    local_tip: Option<u64>,
) -> Result<(), String> {
    let computed = event.compute_hash();
    if computed != event.event_hash {
        return Err(format!(
            "Event hash mismatch: the server says {} but the event's fields hash to {computed}.",
            event.event_hash
        ));
    }
    let proof = event
        .server_proof
        .as_deref()
        .ok_or_else(|| "The event carries no server proof.".to_string())?;
    verify_proof(proof, &event.event_hash, expected.server_pubkey)?;

    if event.account_pubkey != expected.account_pubkey {
        return Err("Server substituted a different account key — refusing to sign.".to_string());
    }
    // Rule 6: the account named must be the one the key owns.
    let owner = derive_user_id(&event.account_pubkey);
    if event.user_id != owner {
        return Err(format!(
            "The event names account {} but the key belongs to account {owner} — refusing to sign.",
            event.user_id
        ));
    }

    check_addresses(&event.ckb_addresses, expected.addresses)?;
    if !event.is_binding {
        return Err("Server returned an unbinding event — refusing to sign.".to_string());
    }
    check_freshness(event, received_at)?;
    check_block_height(event.ckb_block_height, local_tip)
}

/// The event must list exactly the addresses the user chose, each once, in
/// any order.
fn check_addresses(listed: &[String], chosen: &[String]) -> Result<(), String> {
    let chosen_set: HashSet<&String> = chosen.iter().collect();
    let mut seen = HashSet::new();
    for address in listed {
        if !chosen_set.contains(address) {
            return Err(format!(
                "Server returned an address the wallet did not send: {address}"
            ));
        }
        if !seen.insert(address) {
            return Err("Address duplication detected from server — refusing to sign.".to_string());
        }
    }
    if let Some(dropped) = chosen.iter().find(|a| !seen.contains(a)) {
        return Err(format!(
            "Server dropped address {dropped} from the binding. Refresh your bound addresses and try again."
        ));
    }
    Ok(())
}

/// Rule 15's time bounds, judged at the moment the event arrived so that
/// the wallet's own work after that is not counted against the server.
fn check_freshness(event: &BindingEvent, received_at: DateTime<Utc>) -> Result<(), String> {
    let window = event.expired_at - event.created_at;
    if window.num_milliseconds() <= 0 {
        return Err("The binding event's expired_at is not after its created_at.".to_string());
    }
    if window.num_milliseconds() > CO_SIGN_WINDOW_SECS * 1_000 {
        return Err(format!(
            "The binding event's signing window is {} s, longer than the {CO_SIGN_WINDOW_SECS} s allowed.",
            window.num_seconds()
        ));
    }
    let now = received_at.naive_utc();
    let skew_ms = (event.created_at - now).num_milliseconds();
    if skew_ms.abs() > MAX_CLOCK_SKEW_MS {
        let direction = if skew_ms > 0 { "ahead of" } else { "behind" };
        return Err(format!(
            "The DAO server's time is {:.1} s {direction} this device's clock. \
             Check that this device's clock is set automatically, then retry.",
            skew_ms.abs() as f64 / 1_000.0
        ));
    }
    if event.expired_at <= now {
        return Err("Binding challenge has expired. Please retry.".to_string());
    }
    Ok(())
}

/// Rule 15: the event's block height must be near the tip this wallet sees
/// on its own.
fn check_block_height(height: i64, local_tip: Option<u64>) -> Result<(), String> {
    let tip = local_tip.ok_or_else(|| {
        "This wallet has not synced the CKB chain yet, so it cannot check the event's block \
         height. Wait for the node to sync, then retry."
            .to_string()
    })?;
    let height = u64::try_from(height)
        .map_err(|_| format!("The event's block height {height} is negative."))?;
    if height.abs_diff(tip) > TOL_BLOCKS {
        return Err(format!(
            "The event's block height {height} is more than {TOL_BLOCKS} blocks from this \
             wallet's tip {tip}."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::tests::{at, fixture};
    use crate::proof::tests::{server_pubkey, sign, SERVER_SEED};
    use chrono::TimeZone;

    const HEIGHT: u64 = 12_345_678;

    /// The fixture event, re-keyed and re-signed as an honest server would.
    fn honest() -> BindingEvent {
        let mut event = fixture();
        event.account_pubkey = "ab".repeat(32);
        event.user_id = derive_user_id(&event.account_pubkey);
        resign(event)
    }

    fn resign(mut event: BindingEvent) -> BindingEvent {
        event.event_hash = event.compute_hash();
        event.server_proof = Some(sign(&SERVER_SEED, &event.event_hash));
        event
    }

    /// One second after the fixture's `created_at`.
    fn arrival() -> DateTime<Utc> {
        Utc.from_utc_datetime(&at("2026-01-02 03:04:06"))
    }

    fn check(event: &BindingEvent, tip: Option<u64>) -> Result<(), String> {
        let addresses = vec!["ckt1qexample1".to_string(), "ckt1qexample2".to_string()];
        let key = server_pubkey();
        let expected = Expected {
            account_pubkey: &"ab".repeat(32),
            addresses: &addresses,
            server_pubkey: &key,
        };
        verify_event(event, &expected, arrival(), tip)
    }

    #[test]
    fn an_honest_event_passes() {
        assert!(check(&honest(), Some(HEIGHT)).is_ok());
    }

    #[test]
    fn a_changed_field_breaks_the_hash() {
        let mut event = honest();
        event.ckb_block_height += 1;
        assert!(check(&event, Some(HEIGHT))
            .unwrap_err()
            .contains("hash mismatch"));
    }

    #[test]
    fn an_event_signed_by_another_key_is_refused() {
        let mut event = honest();
        event.server_proof = Some(sign(&[0x22; 32], &event.event_hash));
        assert!(check(&event, Some(HEIGHT))
            .unwrap_err()
            .contains("unknown key"));
    }

    #[test]
    fn another_account_key_is_refused() {
        let mut event = honest();
        event.account_pubkey = "cd".repeat(32);
        event.user_id = derive_user_id(&event.account_pubkey);
        let err = check(&resign(event), Some(HEIGHT)).unwrap_err();
        assert!(err.contains("different account key"), "{err}");
    }

    /// Rule 6: a server could name another account while keeping the key.
    #[test]
    fn an_account_id_the_key_does_not_own_is_refused() {
        let mut event = honest();
        event.user_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
        let err = check(&resign(event), Some(HEIGHT)).unwrap_err();
        assert!(err.contains("belongs to account"), "{err}");
    }

    #[test]
    fn an_address_set_other_than_the_chosen_one_is_refused() {
        for (addresses, want) in [
            (vec!["ckt1qexample1", "ckt1qother"], "did not send"),
            (vec!["ckt1qexample1"], "dropped"),
            (
                vec!["ckt1qexample1", "ckt1qexample2", "ckt1qexample1"],
                "duplication",
            ),
        ] {
            let mut event = honest();
            event.ckb_addresses = addresses.iter().map(|a| a.to_string()).collect();
            let err = check(&resign(event), Some(HEIGHT)).unwrap_err();
            assert!(err.contains(want), "{want}: {err}");
        }
    }

    #[test]
    fn the_chosen_addresses_pass_in_any_order() {
        let mut event = honest();
        event.ckb_addresses.reverse();
        assert!(check(&resign(event), Some(HEIGHT)).is_ok());
    }

    #[test]
    fn an_unbinding_event_is_refused() {
        let mut event = honest();
        event.is_binding = false;
        assert!(check(&resign(event), Some(HEIGHT))
            .unwrap_err()
            .contains("unbinding"));
    }

    #[test]
    fn a_window_longer_than_twenty_seconds_is_refused() {
        let mut event = honest();
        event.expired_at = event.created_at + chrono::Duration::seconds(21);
        assert!(check(&resign(event), Some(HEIGHT))
            .unwrap_err()
            .contains("signing window"));
    }

    #[test]
    fn an_expiry_not_after_creation_is_refused() {
        let mut event = honest();
        event.expired_at = event.created_at;
        assert!(check(&resign(event), Some(HEIGHT))
            .unwrap_err()
            .contains("not after"));
    }

    #[test]
    fn a_server_clock_off_by_more_than_a_second_is_refused() {
        for offset in [-3, 2] {
            let mut event = honest();
            event.created_at = at("2026-01-02 03:04:06") + chrono::Duration::seconds(offset);
            event.expired_at = event.created_at + chrono::Duration::seconds(20);
            let err = check(&resign(event), Some(HEIGHT)).unwrap_err();
            assert!(err.contains("clock"), "{offset}: {err}");
        }
    }

    #[test]
    fn an_expired_event_is_refused() {
        let mut event = honest();
        event.created_at = at("2026-01-02 03:04:05.500");
        event.expired_at = at("2026-01-02 03:04:06");
        assert!(check(&resign(event), Some(HEIGHT))
            .unwrap_err()
            .contains("expired"));
    }

    #[test]
    fn the_block_height_must_be_within_two_blocks_of_the_tip() {
        for tip in [HEIGHT - 2, HEIGHT, HEIGHT + 2] {
            assert!(check(&honest(), Some(tip)).is_ok(), "{tip}");
        }
        for tip in [HEIGHT - 3, HEIGHT + 3] {
            assert!(check(&honest(), Some(tip))
                .unwrap_err()
                .contains("block height"));
        }
    }

    #[test]
    fn no_local_tip_means_no_signature() {
        assert!(check(&honest(), None).unwrap_err().contains("not synced"));
    }
}
