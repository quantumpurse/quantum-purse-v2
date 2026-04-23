//! Light-client process management — binary discovery, `config.toml`
//! materialization, and spawn.
//!
//! Exposed through `NodeManager::spawn()`; callers shouldn't need to
//! reach in here directly. The module stays `pub` for documentation
//! and to let tests poke at the helpers if ever needed.

use crate::config::{NetworkType, NodeConfig};
use crate::error::NodeManagerError;
use crate::process::NodeProcess;
use std::path::{Path, PathBuf};

/// Upstream light-client configs embedded at compile time. On first run
/// (or when the user deletes the file) one of these is written to
/// `<data_dir>/light-client/<network>/config.toml` with the relative
/// `[store]` / `[network]` paths rewritten to absolute paths inside the
/// per-network directory.
const TESTNET_TEMPLATE: &str =
    include_str!("../../../vendor/ckb-light-client/config/testnet.toml");
const MAINNET_TEMPLATE: &str =
    include_str!("../../../vendor/ckb-light-client/config/mainnet.toml");

/// Resolves the path to the `ckb-light-client` executable.
///
/// Order of precedence:
/// 1. `config.binary_path` — explicit user override via Settings.
/// 2. Bundled sibling to the currently-running executable (macOS
///    `qpv2.app/Contents/MacOS/ckb-light-client`).
/// 3. Dev fallback — walk up from the current exe to find
///    `vendor/ckb-light-client/target/{release,debug}/ckb-light-client`.
pub fn locate_binary(config: &NodeConfig) -> Result<PathBuf, NodeManagerError> {
    if let Some(path) = &config.binary_path {
        if path.exists() {
            return Ok(path.clone());
        }
        return Err(NodeManagerError::BinaryNotFound {
            path: path.display().to_string(),
            reason: "File does not exist.".to_string(),
        });
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("ckb-light-client");
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            for profile in ["release", "debug"] {
                let candidate = ancestor
                    .join("vendor")
                    .join("ckb-light-client")
                    .join("target")
                    .join(profile)
                    .join("ckb-light-client");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }

    Err(NodeManagerError::BinaryNotFound {
        path: "<auto-discovery>".to_string(),
        reason: "ckb-light-client binary not found. Build the submodule \
                 first: `(cd vendor/ckb-light-client && cargo build --release)`, \
                 or set the binary path under Settings."
            .to_string(),
    })
}

/// Ensures `<data_dir>/light-client/<network>/config.toml` exists, writing
/// the embedded template (with rewritten absolute store/network paths) if
/// it's missing. Idempotent — leaves an existing file untouched so users
/// can hand-edit.
pub fn ensure_config_file(config: &NodeConfig) -> Result<(), NodeManagerError> {
    let data_dir = config.node_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    let config_path = data_dir.join("config.toml");
    if config_path.exists() {
        return Ok(());
    }

    let template = match config.network {
        NetworkType::Mainnet => MAINNET_TEMPLATE,
        NetworkType::Testnet => TESTNET_TEMPLATE,
    };
    let rewritten = rewrite_template_paths(template, &data_dir);

    std::fs::write(&config_path, rewritten)?;
    Ok(())
}

/// Starts the light-client process. Materializes `config.toml` if missing,
/// locates the binary, then delegates to `NodeProcess::start` which also
/// waits for the RPC port to be reachable.
///
/// GUI code should call `NodeManager::spawn()` rather than this directly;
/// that method keeps the node-type dispatch in one place.
pub fn spawn(config: &NodeConfig) -> Result<NodeProcess, NodeManagerError> {
    ensure_config_file(config)?;

    // `NodeProcess::start` consults `config.binary_path` itself; give it a
    // config with `binary_path` filled in from our discovery result if it
    // wasn't already.
    let mut resolved = config.clone();
    if resolved.binary_path.is_none() {
        resolved.binary_path = Some(locate_binary(config)?);
    }

    NodeProcess::start(&resolved)
}

/// Rewrites the two relative paths in the upstream template
/// (`[store] path = "data/store"` and `[network] path = "data/network"`)
/// to absolute paths inside `data_dir`. Keeps the rest of the TOML
/// verbatim so the extensive bootnode list and comments aren't disturbed
/// — avoids pulling in a full TOML writer just for two lines.
fn rewrite_template_paths(template: &str, data_dir: &Path) -> String {
    let store_abs = data_dir.join("data").join("store");
    let network_abs = data_dir.join("data").join("network");

    let mut out = String::with_capacity(template.len() + 256);
    let mut section: Option<&str> = None;
    for line in template.lines() {
        let trimmed = line.trim_start();
        if let Some(header) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = Some(header.trim());
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if trimmed.starts_with("path") && trimmed.contains('=') {
            match section {
                Some("store") => {
                    out.push_str(&format!("path = {:?}\n", store_abs.display().to_string()));
                    continue;
                }
                Some("network") => {
                    out.push_str(&format!("path = {:?}\n", network_abs.display().to_string()));
                    continue;
                }
                _ => {}
            }
        }

        out.push_str(line);
        out.push('\n');
    }
    out
}
