//! The DAO server's wallet API. Every call except the server key carries
//! the binding API key as a Bearer token.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::event::BindingEvent;

/// Bound on every call, so a server that stops answering fails the step
/// instead of hanging it.
const TIMEOUT: Duration = Duration::from_secs(15);

/// The DAO account the API key belongs to, as the server describes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub user_id: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// The server's answer to a submitted bind: where the event landed in the
/// archive, signed (security assumption 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendAck {
    #[serde(default)]
    pub bound_addresses: Vec<String>,
    #[serde(default)]
    pub git_commit_hash: Option<String>,
    pub leaf_index: i64,
    pub mmr_root: String,
    pub attestation: String,
}

#[derive(Deserialize)]
struct ServerKeyReply {
    public_key: String,
}

#[derive(Deserialize)]
struct BoundAddressesReply {
    #[serde(default)]
    bound_addresses: Vec<String>,
    account_info: AccountInfo,
}

#[derive(Deserialize)]
struct SessionReply {
    payload: BindingEvent,
}

#[derive(Deserialize)]
struct ErrorReply {
    message: String,
}

pub struct DaoClient {
    base_url: String,
    http: reqwest::blocking::Client,
}

impl DaoClient {
    pub fn new(base_url: &str) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| format!("Failed to build the HTTP client: {e}"))?;
        Ok(Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The key the server signs events and acks with.
    pub fn server_public_key(&self) -> Result<String, String> {
        let request = self.http.get(self.url("/config/server-public-key"));
        let reply: ServerKeyReply = self.send(request, "read the server key")?;
        Ok(reply.public_key.trim().to_lowercase())
    }

    /// The addresses this account has bound, and who the account is.
    pub fn bound_addresses(&self, api_key: &str) -> Result<(Vec<String>, AccountInfo), String> {
        let request = self
            .http
            .get(self.url("/governance/address-binding/wallet/bound-addresses"))
            .bearer_auth(api_key);
        let reply: BoundAddressesReply = self.send(request, "read the bound addresses")?;
        Ok((reply.bound_addresses, reply.account_info))
    }

    /// Asks the server for a binding event over `addresses`, to sign.
    pub fn open_session(
        &self,
        api_key: &str,
        addresses: &[String],
    ) -> Result<BindingEvent, String> {
        let request = self
            .http
            .post(self.url("/governance/address-binding/session"))
            .bearer_auth(api_key)
            .json(&serde_json::json!({ "addresses_to_bind": addresses }));
        let reply: SessionReply = self.send(request, "open a binding session")?;
        Ok(reply.payload)
    }

    /// Submits the event with its signatures filled in.
    pub fn submit(&self, api_key: &str, event: &BindingEvent) -> Result<AppendAck, String> {
        let request = self
            .http
            .post(self.url("/governance/address-binding/verify"))
            .bearer_auth(api_key)
            .json(event);
        self.send(request, "submit the binding")
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Sends `request` and reads a JSON reply. A refusal reads the server's
    /// `{ "message": … }` body when it has one.
    fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::blocking::RequestBuilder,
        action: &str,
    ) -> Result<T, String> {
        let response = request.send().map_err(|e| {
            format!(
                "Could not reach the DAO server at {} to {action}: {e}",
                self.base_url
            )
        })?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|e| format!("The DAO server's reply to {action} could not be read: {e}"))?;
        if !status.is_success() {
            let message = serde_json::from_str::<ErrorReply>(&body)
                .map(|e| e.message)
                .unwrap_or(body);
            return Err(format!(
                "The DAO server refused to {action} ({status}): {message}"
            ));
        }
        serde_json::from_str(&body)
            .map_err(|e| format!("The DAO server's reply to {action} is malformed: {e}"))
    }
}
