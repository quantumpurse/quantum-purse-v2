//! Persistent transaction-history store.
//!
//! Serialized to `<data_dir>/tx_history_<network>.json` via
//! `qpv2_core::db::get_tx_history_path()`. Survives app restarts so the
//! dashboard can render instantly on unlock and the periodic sync only
//! pulls new blocks.

use crate::types::TxRecord;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};

/// On-disk schema. The incremental-sync watermark is derivable from
/// `records` (max `block_number` among committed rows), so it isn't
/// stored separately. Old files that include a `watermark` field still
/// load cleanly — serde ignores unknown fields by default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TxHistoryStore {
    pub records: Vec<TxRecord>,
}

impl TxHistoryStore {
    /// Loads the store for `network_tag` if present. Returns `Ok(None)`
    /// when the file is absent (fresh wallet or never synced on this
    /// network), `Err` when the file exists but can't be read or parsed.
    pub fn load(network_tag: &str) -> Result<Option<Self>, String> {
        let path =
            qpv2_core::db::get_tx_history_path(network_tag).map_err(|e| format!("path: {}", e))?;
        if !path.exists() {
            return Ok(None);
        }
        let mut file = File::open(&path).map_err(|e| format!("open: {}", e))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|e| format!("read: {}", e))?;
        let store: TxHistoryStore =
            serde_json::from_str(&buf).map_err(|e| format!("parse: {}", e))?;
        Ok(Some(store))
    }

    /// Writes the store atomically via tmp-file + rename so a crash mid-write
    /// cannot corrupt the canonical file. `network_tag` scopes the file to
    /// the active chain.
    pub fn save(&self, network_tag: &str) -> Result<(), String> {
        let final_path =
            qpv2_core::db::get_tx_history_path(network_tag).map_err(|e| format!("path: {}", e))?;
        let tmp_path = final_path.with_extension("json.tmp");

        let json = serde_json::to_string_pretty(self).map_err(|e| format!("serialize: {}", e))?;
        {
            let mut file = File::create(&tmp_path).map_err(|e| format!("create tmp: {}", e))?;
            file.write_all(json.as_bytes())
                .map_err(|e| format!("write tmp: {}", e))?;
            file.sync_all().map_err(|e| format!("fsync tmp: {}", e))?;
        }
        fs::rename(&tmp_path, &final_path).map_err(|e| format!("rename: {}", e))?;
        Ok(())
    }
}
