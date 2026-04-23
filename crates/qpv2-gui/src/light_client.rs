//! Light-client process management — binary discovery, config.toml
//! materialization, and spawn.
//!
//! Called from the node selector's Apply handler when the user picks
//! `NodeType::LightClient`. The caller stores the returned `NodeProcess` on
//! `App::light_client_process` so `Drop` kills the child on app exit.

use node_manager::{NetworkType, NodeConfig, NodeProcess};
use std::path::{Path, PathBuf};

/// Upstream light-client configs embedded at compile time. On first run (or
/// if the user deletes the file) we write one of these into
/// `<data_dir>/light-client/<network>/config.toml`, rewriting the relative
/// `[store]` / `[network]` paths to absolute paths inside the per-network
/// directory.
const TESTNET_TEMPLATE: &str =
    include_str!("../../../vendor/ckb-light-client/config/testnet.toml");
const MAINNET_TEMPLATE: &str =
    include_str!("../../../vendor/ckb-light-client/config/mainnet.toml");

/// Resolves the path to the `ckb-light-client` executable.
///
/// Order of precedence:
/// 1. `config.binary_path` — user override via Settings.
/// 2. Bundled sibling to the current executable (macOS
///    `qpv2.app/Contents/MacOS/ckb-light-client`).
/// 3. Dev fallback — walk up from the current exe to find
///    `vendor/ckb-light-client/target/{release,debug}/ckb-light-client`.
pub fn locate_binary(config: &NodeConfig) -> Result<PathBuf, String> {
    // 1. Explicit user override.
    if let Some(path) = &config.binary_path {
        if path.exists() {
            return Ok(path.clone());
        }
        return Err(format!(
            "Configured binary_path does not exist: {}",
            path.display()
        ));
    }

    // 2. Bundled alongside the current executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("ckb-light-client");
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // 3. Dev fallback — search for the vendor build output by walking up
    //    from the current exe (typically `<workspace>/target/{debug,release}/qpv2-gui`).
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

    Err(
        "ckb-light-client binary not found. \
         Build the submodule first: \
         `(cd vendor/ckb-light-client && cargo build --release)`, \
         or set the binary path under Settings."
            .to_string(),
    )
}

/// Ensures `<data_dir>/light-client/<network>/config.toml` exists, writing
/// the embedded template (with rewritten absolute store/network paths) if
/// it's missing. Idempotent — leaves an existing file untouched so users can
/// hand-edit.
pub fn ensure_config_file(config: &NodeConfig) -> Result<(), String> {
    let data_dir = config.node_data_dir();
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create {}: {}", data_dir.display(), e))?;

    let config_path = data_dir.join("config.toml");
    if config_path.exists() {
        return Ok(());
    }

    let template = match config.network {
        NetworkType::Mainnet => MAINNET_TEMPLATE,
        NetworkType::Testnet => TESTNET_TEMPLATE,
    };
    let rewritten = rewrite_template_paths(template, &data_dir);

    std::fs::write(&config_path, rewritten)
        .map_err(|e| format!("Failed to write {}: {}", config_path.display(), e))?;
    Ok(())
}

/// Starts the light-client process. Assumes `config.node_type == LightClient`.
/// Materializes `config.toml` if missing, locates the binary, then delegates
/// to `NodeProcess::start` which also waits for the RPC port to be
/// reachable.
pub fn spawn(config: &NodeConfig) -> Result<NodeProcess, String> {
    ensure_config_file(config)?;

    // `NodeProcess::start` consults `config.binary_path` itself; give it a
    // config with `binary_path` filled in from our discovery result if it
    // wasn't already.
    let mut resolved = config.clone();
    if resolved.binary_path.is_none() {
        resolved.binary_path = Some(locate_binary(config)?);
    }

    NodeProcess::start(&resolved).map_err(|e| e.to_string())
}

/// Rewrites the two relative paths in the upstream template
/// (`[store] path = "data/store"` and `[network] path = "data/network"`) to
/// absolute paths inside `data_dir`. Keeps the rest of the TOML verbatim so
/// the extensive bootnode list and comments aren't disturbed — avoids
/// pulling in a full TOML writer just for two lines.
fn rewrite_template_paths(template: &str, data_dir: &Path) -> String {
    let store_abs = data_dir.join("data").join("store");
    let network_abs = data_dir.join("data").join("network");

    let mut out = String::with_capacity(template.len() + 256);
    let mut section: Option<&str> = None;
    for line in template.lines() {
        let trimmed = line.trim_start();
        if let Some(header) = trimmed
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
        {
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
                    out.push_str(&format!(
                        "path = {:?}\n",
                        network_abs.display().to_string()
                    ));
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
