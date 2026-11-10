//! What the wallet remembers about DAO servers: which one to use, and the
//! key each one showed first (trust on first use).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_SERVER_URL: &str = "https://api.daov2.site";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub server_url: String,
    /// Server URL → the key it showed on first contact.
    #[serde(default)]
    pub server_keys: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER_URL.to_string(),
            server_keys: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Reads `path`; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("Failed to read DAO v2 settings {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!(
                "Failed to read DAO v2 settings {}: {e}",
                path.display()
            )),
        }
    }

    /// Writes `path` through a temporary file, so a crash never leaves a
    /// half-written pin behind.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to encode DAO v2 settings: {e}"))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(|e| format!("Failed to write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, path).map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }

    /// Trust on first use: the first key `url` shows is remembered, and a
    /// later different key is refused, since a server whose key changed
    /// could be someone else. Returns whether the key was newly pinned.
    pub fn pin_server_key(&mut self, url: &str, served: &str) -> Result<bool, String> {
        match self.server_keys.get(url) {
            Some(pinned) if pinned == served => Ok(false),
            Some(pinned) => Err(format!(
                "SECURITY: the DAO server at {url} now shows key {served}, but this wallet \
                 pinned {pinned} on first contact. Refusing to continue. If the server \
                 operator really changed the key, remove {url} from the DAO v2 settings file."
            )),
            None => {
                self.server_keys.insert(url.to_string(), served.to_string());
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_key_is_pinned_and_a_different_one_refused() {
        let mut settings = Settings::default();
        assert_eq!(settings.pin_server_key("https://a", "k1"), Ok(true));
        assert_eq!(settings.pin_server_key("https://a", "k1"), Ok(false));
        assert!(settings
            .pin_server_key("https://a", "k2")
            .unwrap_err()
            .starts_with("SECURITY"));
        assert_eq!(settings.pin_server_key("https://b", "k2"), Ok(true));
    }
}
