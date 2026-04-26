pub mod errors;

use super::types::{CipherPayload, SphincsPlusAccount, WalletInfo};
pub use errors::KeyVaultDBError;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Gets the data directory path for the key vault.
///
/// Resolves to the platform-standard application data directory, sharing a
/// root with `node-manager`'s node data (`<root>/node/...`) so all
/// QuantumPurse persistent state lives in one OS-managed location:
/// - macOS: `~/Library/Application Support/quantum-purse/`
/// - Linux: `~/.local/share/quantum-purse/`
/// - Windows: `%APPDATA%\quantum-purse\`
///
/// **Returns**:
/// - `Result<PathBuf, KeyVaultDBError>` - The data directory path on success, or an error if it cannot be determined.
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

/// Gets the path to the encrypted master seed file
fn get_master_seed_path() -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_data_dir()?.join("master_seed.json"))
}

/// Gets the path to the encrypted accounts file
fn get_accounts_path() -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_data_dir()?.join("accounts.json"))
}

/// Gets the path to the wallet info file
fn get_wallet_info_path() -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_data_dir()?.join("wallet_info.json"))
}

/// Gets the path to the persisted transaction history file for a given
/// network. The cache is namespaced per network (e.g. `tx_history_mainnet.json`,
/// `tx_history_testnet.json`) so switching networks can't leak records — or
/// a stale watermark — from one chain onto another.
///
/// Public so the GUI can read/write its own schema without going through
/// `KeyVault`. The core crate does not parse this file.
pub fn get_tx_history_path(network_tag: &str) -> Result<PathBuf, KeyVaultDBError> {
    Ok(get_data_dir()?.join(format!("tx_history_{}.json", network_tag)))
}

/// Stores the encrypted master seed in the file system.
///
/// **Parameters**:
/// - `payload: CipherPayload` - The encrypted master seed data to store.
///
/// **Returns**:
/// - `Result<(), KeyVaultDBError>` - Ok on success, or an error if storage fails.
///
/// **Warning**: This method overwrites the existing master seed.
pub fn set_encrypted_seed(payload: CipherPayload) -> Result<(), KeyVaultDBError> {
    let path = get_master_seed_path()?;
    let json = serde_json::to_string_pretty(&payload)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

/// Retrieves the encrypted master seed from the file system.
///
/// **Returns**:
/// - `Result<Option<CipherPayload>, KeyVaultDBError>` - The encrypted master seed if it exists, `None` if not found, or an error if retrieval fails.
pub fn get_encrypted_seed() -> Result<Option<CipherPayload>, KeyVaultDBError> {
    let path = get_master_seed_path()?;

    if !path.exists() {
        return Ok(None);
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let payload: CipherPayload = serde_json::from_str(&contents)?;
    Ok(Some(payload))
}

/// Helper function to load all accounts from file
fn load_accounts() -> Result<HashMap<String, SphincsPlusAccount>, KeyVaultDBError> {
    let path = get_accounts_path()?;

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let accounts: HashMap<String, SphincsPlusAccount> = serde_json::from_str(&contents)?;
    Ok(accounts)
}

/// Helper function to save all accounts to file
fn save_accounts(accounts: &HashMap<String, SphincsPlusAccount>) -> Result<(), KeyVaultDBError> {
    let path = get_accounts_path()?;
    let json = serde_json::to_string_pretty(accounts)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

/// Stores a SPHINCS+ account to the file system.
///
/// **Parameters**:
/// - `account: SphincsPlusAccount` - The SPHINCS+ account to store.
///
/// **Returns**:
/// - `Result<(), KeyVaultDBError>` - Ok on success, or an error if storage fails.
pub fn add_account(mut account: SphincsPlusAccount) -> Result<(), KeyVaultDBError> {
    let mut accounts = load_accounts()?;
    let count = accounts.len();
    account.index = count as u32;
    accounts.insert(account.lock_args.clone(), account);
    save_accounts(&accounts)?;
    Ok(())
}

/// Retrieves a child account by its lock args from the file system.
///
/// **Parameters**:
/// - `lock_args: &str` - The hex-encoded lock script's arguments corresponding to the SPHINCS+ public key of the retrieved child account.
///
/// **Returns**:
/// - `Result<Option<SphincsPlusAccount>, KeyVaultDBError>` - The child key if found, `None` if not found, or an error if retrieval fails.
pub fn get_account(lock_args: &str) -> Result<Option<SphincsPlusAccount>, KeyVaultDBError> {
    let accounts = load_accounts()?;
    Ok(accounts.get(lock_args).cloned())
}

/// Clears the master seed file.
///
/// **Returns**:
/// - `Result<(), KeyVaultDBError>` - Ok on success, or an error if the operation fails.
pub fn clear_master_seed() -> Result<(), KeyVaultDBError> {
    let path = get_master_seed_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Clears the accounts file.
///
/// **Returns**:
/// - `Result<(), KeyVaultDBError>` - Ok on success, or an error if the operation fails.
pub fn clear_accounts() -> Result<(), KeyVaultDBError> {
    let path = get_accounts_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Gets all accounts sorted by index.
///
/// **Returns**:
/// - `Result<Vec<SphincsPlusAccount>, KeyVaultDBError>` - All accounts sorted by index on success.
pub fn get_all_accounts() -> Result<Vec<SphincsPlusAccount>, KeyVaultDBError> {
    let accounts = load_accounts()?;
    let mut account_list: Vec<SphincsPlusAccount> = accounts.into_values().collect();
    account_list.sort_by_key(|a| a.index);
    Ok(account_list)
}

/// Stores wallet info in the file system.
///
/// **Parameters**:
/// - `info: WalletInfo` - The wallet info to store.
///
/// **Returns**:
/// - `Result<(), KeyVaultDBError>` - Ok on success, or an error if storage fails.
pub fn set_wallet_info(info: WalletInfo) -> Result<(), KeyVaultDBError> {
    let path = get_wallet_info_path()?;
    let json = serde_json::to_string_pretty(&info)?;
    let mut file = File::create(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

/// Retrieves wallet info from the file system.
///
/// **Returns**:
/// - `Result<Option<WalletInfo>, KeyVaultDBError>` - The wallet info if it exists, `None` if not found, or an error if retrieval fails.
pub fn get_wallet_info() -> Result<Option<WalletInfo>, KeyVaultDBError> {
    let path = get_wallet_info_path()?;

    if !path.exists() {
        return Ok(None);
    }

    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let info: WalletInfo = serde_json::from_str(&contents)?;
    Ok(Some(info))
}

/// Clears the wallet info file.
///
/// **Returns**:
/// - `Result<(), KeyVaultDBError>` - Ok on success, or an error if the operation fails.
pub fn clear_wallet_info() -> Result<(), KeyVaultDBError> {
    let path = get_wallet_info_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Clears every per-network transaction history file under the data
/// directory.
///
/// Called by `KeyVault::clear_database()` so removing the wallet also wipes
/// cached tx history for all networks — we can't know which ones the user
/// interacted with.
pub fn clear_tx_history() -> Result<(), KeyVaultDBError> {
    let dir = get_data_dir()?;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if name_str.starts_with("tx_history_") && name_str.ends_with(".json") {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
