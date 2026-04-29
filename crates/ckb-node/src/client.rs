//! Cloneable handle to the active CKB backend.
//!
//! Bundles the protocol speaker (`Arc<dyn Client>`) with the
//! `NodeConfig` snapshot it was built from. This is the unit that
//! background threads carry: one cheap clone gives them the rpc client
//! plus the backend-shape knowledge (`network`, `node_type`,
//! `is_mainnet`) they need to call `wallet_helpers::*` correctly without
//! having to consult any single-owner state.
//!
//! Lifecycle: replaced wholesale on backend switch — `App` builds a new
//! `NodeClient` from the new config, stores it, and lets the old one
//! drop when its last in-flight thread finishes its work. The trait
//! object behind the `Arc` keeps speaking to the old backend until it
//! does; this is intentional, and the only correct way to retire an
//! HTTP client that may be mid-request.

use std::sync::Arc;

use crate::config::{NetworkType, NodeConfig, NodeType};
use crate::rpc::{self, Client};

#[derive(Clone)]
pub struct NodeClient {
    client: Arc<dyn Client>,
    config: NodeConfig,
}

impl NodeClient {
    /// Builds a fresh handle bound to `config`. Constructs the
    /// concrete `Client` impl appropriate for `config.node_type` via
    /// `rpc::build`.
    pub fn new(config: NodeConfig) -> Self {
        let client = rpc::build(&config);
        Self { client, config }
    }

    /// Returns a cloned `Arc` handle to the rpc client. Use when moving
    /// the client into a background thread independent of `self`.
    pub fn client(&self) -> Arc<dyn Client> {
        self.client.clone()
    }

    /// Returns a borrowed view of the rpc client. Use in synchronous
    /// code that does not need to outlive `self`.
    pub fn client_ref(&self) -> &dyn Client {
        self.client.as_ref()
    }

    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    pub fn network(&self) -> NetworkType {
        self.config.network
    }

    pub fn node_type(&self) -> NodeType {
        self.config.node_type
    }

    pub fn is_mainnet(&self) -> bool {
        self.config.network == NetworkType::Mainnet
    }
}
