use crate::error::NodeManagerError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Public RPC endpoints provided by the Nervos Foundation.
const PUBLIC_RPC_MAINNET: &str = "https://mainnet.ckb.dev";
const PUBLIC_RPC_TESTNET: &str = "https://testnet.ckb.dev";

/// Default local RPC ports.
const LOCAL_RPC_FULL_NODE: &str = "http://127.0.0.1:8114";
const LOCAL_RPC_LIGHT_CLIENT: &str = "http://127.0.0.1:9000";

/// The type of CKB node backend to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    /// Connect to a public RPC endpoint. No local binary needed.
    PublicRpc,
    /// Run a local CKB light client process.
    LightClient,
    /// Run a local CKB full node process.
    FullNode,
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeType::PublicRpc => write!(f, "PublicRpc"),
            NodeType::LightClient => write!(f, "LightClient"),
            NodeType::FullNode => write!(f, "FullNode"),
        }
    }
}

/// The CKB network to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkType {
    Mainnet,
    Testnet,
}

impl NetworkType {
    /// Lowercase short identifier, suitable for file names and directory
    /// segments (e.g. `tx_history_mainnet.json`, `light-client/testnet/`).
    /// Distinct from `Display` which produces the capitalized form used in
    /// user-facing UI.
    pub fn tag(&self) -> &'static str {
        match self {
            NetworkType::Mainnet => "mainnet",
            NetworkType::Testnet => "testnet",
        }
    }
}

impl fmt::Display for NetworkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkType::Mainnet => write!(f, "Mainnet"),
            NetworkType::Testnet => write!(f, "Testnet"),
        }
    }
}

/// Configuration for the node manager.
///
/// All fields are configurable. The wallet persists this to disk so user
/// preferences survive restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Which backend to use. Defaults to `PublicRpc`.
    pub node_type: NodeType,

    /// Which CKB network to connect to. Defaults to `Testnet`.
    pub network: NetworkType,

    /// Path to the node binary on disk. `None` when using `PublicRpc`.
    pub binary_path: Option<PathBuf>,

    /// Directory where the node stores its chain data.
    /// Each node type gets a subdirectory (`light-client/` or `full-node/`).
    pub data_dir: PathBuf,

    /// The JSON-RPC URL to connect to.
    pub rpc_url: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_type: NodeType::PublicRpc,
            network: NetworkType::Testnet,
            binary_path: None,
            data_dir: default_data_dir(),
            rpc_url: PUBLIC_RPC_TESTNET.to_string(),
        }
    }
}

impl NodeConfig {
    /// Returns the default RPC URL for the current node type and network.
    pub fn default_rpc_url(&self) -> &'static str {
        Self::default_rpc_url_for(self.node_type, self.network)
    }

    /// Associated form of `default_rpc_url` — returns the canonical URL for
    /// any `(NodeType, NetworkType)` pair without needing a `NodeConfig`
    /// instance. Lets callers probe non-active backends (e.g. the Node
    /// Manager page showing Public RPC status while Light Client is the
    /// active backend).
    pub fn default_rpc_url_for(node_type: NodeType, network: NetworkType) -> &'static str {
        match node_type {
            NodeType::PublicRpc => match network {
                NetworkType::Mainnet => PUBLIC_RPC_MAINNET,
                NetworkType::Testnet => PUBLIC_RPC_TESTNET,
            },
            NodeType::LightClient => LOCAL_RPC_LIGHT_CLIENT,
            NodeType::FullNode => LOCAL_RPC_FULL_NODE,
        }
    }

    /// Whether this configuration requires a local node binary.
    pub fn requires_binary(&self) -> bool {
        matches!(self.node_type, NodeType::LightClient | NodeType::FullNode)
    }

    /// Returns the data subdirectory for the active node type + network.
    ///
    /// Mainnet and testnet are independent ledgers — sharing a store between
    /// them would corrupt node state, so local backends get a
    /// `<type>/<network>/` layout (e.g. `light-client/testnet/`). `PublicRpc`
    /// has no local state and uses the bare data dir.
    pub fn node_data_dir(&self) -> PathBuf {
        let net = self.network.tag();
        match self.node_type {
            NodeType::PublicRpc => self.data_dir.clone(),
            NodeType::LightClient => self.data_dir.join("light-client").join(net),
            NodeType::FullNode => self.data_dir.join("full-node").join(net),
        }
    }

    /// Loads configuration from the standard config file path.
    /// Returns `Ok(None)` if the file does not exist.
    pub fn load() -> Result<Option<Self>, NodeManagerError> {
        let path = config_file_path()?;

        if !path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config: NodeConfig =
            serde_json::from_str(&contents).map_err(NodeManagerError::SerializationError)?;
        Ok(Some(config))
    }

    /// Loads configuration from disk, or returns defaults if no config file exists.
    pub fn load_or_default() -> Result<Self, NodeManagerError> {
        Ok(Self::load()?.unwrap_or_default())
    }

    /// Persists this configuration to disk.
    pub fn save(&self) -> Result<(), NodeManagerError> {
        let path = config_file_path()?;

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let json =
            serde_json::to_string_pretty(self).map_err(NodeManagerError::SerializationError)?;
        let mut file = File::create(&path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }
}

/// Platform-standard application data directory for node data.
/// - macOS: `~/Library/Application Support/quantum-purse/node/`
/// - Linux: `~/.local/share/quantum-purse/node/`
/// - Windows: `C:\Users\<User>\AppData\Local\quantum-purse\node\`
fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("quantum-purse")
        .join("node")
}

/// Path to the node manager config file.
/// Stored alongside the node data: `<data_dir>/node_config.json`.
fn config_file_path() -> Result<PathBuf, NodeManagerError> {
    let data_dir = dirs::data_dir().ok_or_else(|| {
        NodeManagerError::ConfigError("Cannot determine platform data directory.".to_string())
    })?;
    Ok(data_dir
        .join("quantum-purse")
        .join("node")
        .join("node_config.json"))
}
