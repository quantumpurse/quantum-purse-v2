pub mod errors;
pub mod wallets;

use super::types::{CipherPayload, SphincsPlusAccount, WalletInfo};
use errors::KeyVaultDBError;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;

pub fn get_data_dir() -> Result<PathBuf, KeyVaultDBError> {
    let base = dirs::data_dir().ok_or_else(|| {
        KeyVaultDBError::DatabaseError("Cannot determine platform data directory".to_string())
    })?;

    let data_dir = base.join("quantum-purse");

    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)?;
    }

    Ok(data_dir)
}

/// Returns the wallet subdirectory path without creating it.
pub fn get_wallet_dir(wallet_id: u32) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_data_dir()?.join("wallets").join(wallet_id.to_string()))
}

/// Returns the wallet subdirectory path, creating it if needed.
pub fn create_wallet_dir(wallet_id: u32) -> Result<PathBuf, KeyVaultDBError> {
    let dir = get_wallet_dir(wallet_id)?;
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn get_master_seed_path(wallet_id: u32) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_wallet_dir(wallet_id)?.join("seed.json"))
}

fn get_singlesig_path(wallet_id: u32) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_wallet_dir(wallet_id)?.join("accounts.json"))
}

fn get_wallet_info_path(wallet_id: u32) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_wallet_dir(wallet_id)?.join("meta.json"))
}

pub fn get_tx_history_path(wallet_id: u32, network_tag: &str) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_wallet_dir(wallet_id)?.join(format!("tx_history_{}.json", network_tag)))
}

pub fn set_encrypted_seed(wallet_id: u32, payload: CipherPayload) -> Result<(), KeyVaultDBError> {
    let path = get_master_seed_path(wallet_id)?;
    let json = serde_json::to_string_pretty(&payload)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn get_encrypted_seed(wallet_id: u32) -> Result<Option<CipherPayload>, KeyVaultDBError> {
    let path = get_master_seed_path(wallet_id)?;

    if !path.exists() {
        return Ok(None);
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let payload: CipherPayload = serde_json::from_str(&contents)?;
    Ok(Some(payload))
}

fn load_singlesig(wallet_id: u32) -> Result<HashMap<String, SphincsPlusAccount>, KeyVaultDBError> {
    let path = get_singlesig_path(wallet_id)?;

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let accounts: HashMap<String, SphincsPlusAccount> = serde_json::from_str(&contents)?;
    Ok(accounts)
}

pub fn add_singlesig_account(wallet_id: u32, mut account: SphincsPlusAccount) -> Result<(), KeyVaultDBError> {
    let mut accounts = load_singlesig(wallet_id)?;
    if accounts.contains_key(&account.lock_args) {
        return Ok(());
    }
    let count = accounts.len();
    account.index = count as u32;
    accounts.insert(account.lock_args.clone(), account);
    let path = get_singlesig_path(wallet_id)?;
    let json = serde_json::to_string_pretty(&accounts)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn get_singlesig_account(
    wallet_id: u32,
    lock_args: &str,
) -> Result<Option<SphincsPlusAccount>, KeyVaultDBError> {
    let accounts = load_singlesig(wallet_id)?;
    Ok(accounts.get(lock_args).cloned())
}

pub fn clear_master_seed(wallet_id: u32) -> Result<(), KeyVaultDBError> {
    let path = get_master_seed_path(wallet_id)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn clear_singlesig_accounts(wallet_id: u32) -> Result<(), KeyVaultDBError> {
    let path = get_singlesig_path(wallet_id)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn get_singlesig_accounts(wallet_id: u32) -> Result<Vec<SphincsPlusAccount>, KeyVaultDBError> {
    let accounts = load_singlesig(wallet_id)?;
    let mut account_list: Vec<SphincsPlusAccount> = accounts.into_values().collect();
    account_list.sort_by_key(|a| a.index);
    Ok(account_list)
}

// ── Multisig accounts (multisig_accounts.json) ──

fn get_multisig_path(wallet_id: u32) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_wallet_dir(wallet_id)?.join("multisig_accounts.json"))
}

fn load_multisig(wallet_id: u32) -> Result<HashMap<String, SphincsPlusAccount>, KeyVaultDBError> {
    let path = get_multisig_path(wallet_id)?;

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let accounts: HashMap<String, SphincsPlusAccount> = serde_json::from_str(&contents)?;
    Ok(accounts)
}

pub fn add_multisig_account(wallet_id: u32, mut account: SphincsPlusAccount) -> Result<(), KeyVaultDBError> {
    let mut accounts = load_multisig(wallet_id)?;
    if accounts.contains_key(&account.lock_args) {
        return Ok(());
    }
    let count = accounts.len();
    account.index = count as u32;
    accounts.insert(account.lock_args.clone(), account);
    let path = get_multisig_path(wallet_id)?;
    let json = serde_json::to_string_pretty(&accounts)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn get_multisig_account(
    wallet_id: u32,
    lock_args: &str,
) -> Result<Option<SphincsPlusAccount>, KeyVaultDBError> {
    let accounts = load_multisig(wallet_id)?;
    Ok(accounts.get(lock_args).cloned())
}

pub fn get_multisig_accounts(wallet_id: u32) -> Result<Vec<SphincsPlusAccount>, KeyVaultDBError> {
    let accounts = load_multisig(wallet_id)?;
    let mut account_list: Vec<SphincsPlusAccount> = accounts.into_values().collect();
    account_list.sort_by_key(|a| a.index);
    Ok(account_list)
}

pub fn clear_multisig_accounts(wallet_id: u32) -> Result<(), KeyVaultDBError> {
    let path = get_multisig_path(wallet_id)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn set_wallet_info(wallet_id: u32, info: WalletInfo) -> Result<(), KeyVaultDBError> {
    let path = get_wallet_info_path(wallet_id)?;
    let json = serde_json::to_string_pretty(&info)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn get_wallet_info(wallet_id: u32) -> Result<Option<WalletInfo>, KeyVaultDBError> {
    let path = get_wallet_info_path(wallet_id)?;

    if !path.exists() {
        return Ok(None);
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let info: WalletInfo = serde_json::from_str(&contents)?;
    Ok(Some(info))
}

pub fn clear_wallet_info(wallet_id: u32) -> Result<(), KeyVaultDBError> {
    let path = get_wallet_info_path(wallet_id)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn clear_tx_history(wallet_id: u32) -> Result<(), KeyVaultDBError> {
    let dir = get_wallet_dir(wallet_id)?;
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str.starts_with("tx_history_") && name_str.ends_with(".json") {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
