//! The receipt a user keeps after binding: the signed event and, when the
//! server acked it, where it landed in the archive. With it, the user can
//! later prove the server published (or dropped) their binding.

use crate::client::AppendAck;
use crate::event::BindingEvent;

/// A receipt for the user to save wherever they choose; this crate never
/// writes files.
#[derive(Debug, Clone)]
pub struct Receipt {
    /// Suggested name: `binding-<unix ms>.json`.
    pub file_name: String,
    pub json: String,
}

impl Receipt {
    /// Without an ack, the receipt holds the signed event alone.
    pub fn new(event: &BindingEvent, ack: Option<&AppendAck>) -> Result<Self, String> {
        let mut receipt = serde_json::json!({ "event": event });
        if let Some(ack) = ack {
            receipt["leaf_index"] = ack.leaf_index.into();
            receipt["mmr_root"] = ack.mmr_root.clone().into();
            receipt["attestation"] = ack.attestation.clone().into();
            receipt["git_commit_hash"] = ack.git_commit_hash.clone().into();
        }
        let json = serde_json::to_string_pretty(&receipt)
            .map_err(|e| format!("Failed to encode the receipt: {e}"))?;
        Ok(Self {
            file_name: format!("binding-{}.json", chrono::Utc::now().timestamp_millis()),
            json,
        })
    }
}
