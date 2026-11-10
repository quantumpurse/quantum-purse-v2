//! The binding API key the DAO portal hands out:
//! `dao_<64 lowercase hex account key>_<32 alphanumeric secret>`.

const PREFIX: &str = "dao_";
const KEY_LEN: usize = 64;
const SECRET_LEN: usize = 32;

/// The account key inside `api_key`, the only part the wallet uses: every
/// event it signs must name this key. Surrounding whitespace is an error;
/// trim before calling.
pub fn account_pubkey(api_key: &str) -> Result<String, String> {
    let malformed = || {
        "Malformed API key: expected dao_<64-hex pubkey>_<32-char secret>. \
         Re-copy the full key from the DAO website."
            .to_string()
    };
    let rest = api_key.strip_prefix(PREFIX).ok_or_else(malformed)?;
    let (key, secret) = rest.split_once('_').ok_or_else(malformed)?;
    let key_ok = key.len() == KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let secret_ok = secret.len() == SECRET_LEN && secret.bytes().all(|b| b.is_ascii_alphanumeric());
    if key_ok && secret_ok {
        Ok(key.to_string())
    } else {
        Err(malformed())
    }
}

#[cfg(test)]
mod tests {
    use super::account_pubkey;

    fn key() -> String {
        format!("dao_{}_{}", "ab".repeat(32), "Xy9".repeat(10) + "Zz")
    }

    #[test]
    fn a_well_formed_key_yields_its_account_key() {
        assert_eq!(account_pubkey(&key()).unwrap(), "ab".repeat(32));
    }

    #[test]
    fn a_malformed_key_is_refused() {
        let pubkey = "ab".repeat(32);
        let secret = "Xy9".repeat(10) + "Zz";
        for bad in [
            String::new(),
            secret.clone(),
            format!("dao_{pubkey}"),
            format!("dao_{pubkey}_{}", &secret[..31]),
            format!("dao_{}_{secret}", &pubkey[..62]),
            format!("dao_{}_{secret}", pubkey.to_uppercase()),
            format!("{}x", key()),
            format!(" {}", key()),
        ] {
            assert!(account_pubkey(&bad).is_err(), "{bad:?}");
        }
    }
}
