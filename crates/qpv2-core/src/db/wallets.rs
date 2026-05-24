use super::errors::KeyVaultDBError;
use super::{get_data_dir, get_wallet_dir, get_wallet_info};
use crate::types::WalletEntry;
use std::fs;

fn wallets_root_dir() -> Result<std::path::PathBuf, KeyVaultDBError> {
	Ok(get_data_dir()?.join("wallets"))
}

pub fn list_wallets() -> Result<Vec<WalletEntry>, KeyVaultDBError> {
	let root = wallets_root_dir()?;
	if !root.exists() {
		return Ok(Vec::new());
	}

	let mut entries = Vec::new();
	for dir_entry in fs::read_dir(&root)? {
		let dir_entry = dir_entry?;
		if !dir_entry.file_type()?.is_dir() {
			continue;
		}
		let id: u32 = match dir_entry.file_name().to_str().and_then(|s| s.parse().ok()) {
			Some(n) => n,
			None => continue,
		};
		let name = match get_wallet_info(id) {
			Ok(Some(info)) => info.name,
			_ => continue,
		};
		entries.push(WalletEntry { id, name });
	}
	entries.sort_by_key(|e| e.id);

	Ok(entries)
}

pub fn next_wallet_id() -> Result<u32, KeyVaultDBError> {
	let root = wallets_root_dir()?;
	if !root.exists() {
		return Ok(0);
	}
	let mut max_id: Option<u32> = None;
	for dir_entry in fs::read_dir(&root)? {
		let dir_entry = dir_entry?;
		if !dir_entry.file_type()?.is_dir() {
			continue;
		}
		if let Some(id) = dir_entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) {
			max_id = Some(max_id.map_or(id, |m: u32| m.max(id)));
		}
	}
	Ok(max_id.map_or(0, |m| m + 1))
}

pub fn rename_wallet(wallet_id: u32, new_name: &str) -> Result<(), KeyVaultDBError> {
	let mut info = get_wallet_info(wallet_id)?
		.ok_or_else(|| {
			KeyVaultDBError::DatabaseError(format!("Wallet '{}' not found.", wallet_id))
		})?;
	info.name = new_name.to_string();
	let path = get_wallet_dir(wallet_id)?.join("wallet_info.json");
	let json = serde_json::to_string_pretty(&info)?;
	std::fs::write(path, json.as_bytes())?;
	Ok(())
}
