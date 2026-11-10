//! The receipt a user keeps after binding: the signed event and, when the
//! server acked it, where it landed in the archive. With it, the user can
//! later prove the server published (or dropped) their binding.

use std::fs;
use std::path::{Path, PathBuf};

use crate::client::AppendAck;
use crate::event::BindingEvent;

/// Writes `binding-<unix ms>.json` into `dir` and returns its path. Without
/// an ack, the receipt holds the signed event alone.
pub fn write_receipt(
    dir: &Path,
    event: &BindingEvent,
    ack: Option<&AppendAck>,
) -> Result<PathBuf, String> {
    let mut receipt = serde_json::json!({ "event": event });
    if let Some(ack) = ack {
        receipt["leaf_index"] = ack.leaf_index.into();
        receipt["mmr_root"] = ack.mmr_root.clone().into();
        receipt["attestation"] = ack.attestation.clone().into();
        receipt["git_commit_hash"] = ack.git_commit_hash.clone().into();
    }
    let text = serde_json::to_string_pretty(&receipt)
        .map_err(|e| format!("Failed to encode the receipt: {e}"))?;
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    let path = dir.join(format!(
        "binding-{}.json",
        chrono::Utc::now().timestamp_millis()
    ));
    fs::write(&path, text).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(path)
}
