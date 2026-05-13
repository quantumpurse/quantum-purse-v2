use crate::{ACCOUNT, KEY_LEN, SERVICE};
use dbus_secret_service::{EncryptionType, SecretService};
use qpv2_core::SecureVec;
use std::collections::HashMap;

const LABEL: &str = "QuantumPurse";

fn attributes() -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    attrs.insert("service".to_string(), SERVICE.to_string());
    attrs.insert("account".to_string(), ACCOUNT.to_string());
    attrs
}

fn map_err(e: dbus_secret_service::Error) -> String {
    format!("Secret Service error: {}", e)
}

pub fn store_key(key: &[u8]) -> Result<(), String> {
    if key.len() != KEY_LEN {
        return Err(format!("Expected {KEY_LEN}-byte key, got {}.", key.len()));
    }

    let ss = SecretService::new(EncryptionType::Dh).map_err(map_err)?;
    let collection = ss.get_default_collection().map_err(map_err)?;

    collection
        .create_item(LABEL, attributes(), key, true, "application/octet-stream")
        .map_err(map_err)?;

    Ok(())
}

pub fn retrieve_key() -> Result<SecureVec, String> {
    let ss = SecretService::new(EncryptionType::Dh).map_err(map_err)?;
    let result = ss.search_items(attributes()).map_err(map_err)?;

    let item = result
        .unlocked
        .into_iter()
        .next()
        .ok_or_else(|| "Credential not found.".to_string())?;

    let secret = item.get_secret().map_err(map_err)?;
    let secure = SecureVec::from_vec(secret);
    if secure.len() != KEY_LEN {
        return Err(format!(
            "Credential returned {}-byte key, expected {KEY_LEN}.",
            secure.len()
        ));
    }

    Ok(secure)
}

pub fn delete_key() -> Result<(), String> {
    let ss = SecretService::new(EncryptionType::Dh).map_err(map_err)?;
    let result = ss.search_items(attributes()).map_err(map_err)?;

    for item in result.unlocked {
        item.delete().map_err(map_err)?;
    }

    Ok(())
}
