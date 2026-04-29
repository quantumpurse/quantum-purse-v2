pub mod client;
pub mod config;
pub mod error;
pub mod process;
pub mod wallet_helpers;

pub use ckb_sdk::rpc::ckb_indexer::{CellType, Tx};
pub use client::{CkbClient, QpClient};
pub use config::{NetworkType, NodeConfig, NodeType};

use error::NodeManagerError;
use process::{FullNodeProcess, LightClientProcess, NodeProcess};
pub use wallet_helpers::queries::{DepositedCell, PreparedCell};
pub use wallet_helpers::tx_builder::{
    fill_witness, QpDaoDepositBuilder, QpDaoPrepareBuilder, QpDaoWithdrawBuilder, QpTransferBuilder,
};

/// Single-owner slot for the local CKB node child process.
///
/// `App` holds this directly; the type is intentionally **not** `Clone`,
/// **not** wrapped in `Arc`/`Mutex`. Two consequences flow from that:
///
/// 1. `Drop` is deterministic. When `App` drops, the `Option<Box<dyn
///    NodeProcess>>` drops, which runs the inner `*Process::drop`
///    (SIGTERM → grace → SIGKILL). No background-thread `Arc` clone
///    can outlive the App and orphan the child.
/// 2. Backend switches are full replacements. A new
///    `LocalNodeProcess::new(new_cfg)` retires the old one via that same
///    `Drop` chain — no in-place mutation of the running process.
pub struct LocalNodeProcess {
    /// Held only because `spawn()` needs it to construct the right
    /// `NodeProcess` impl. External readers go through `QpClient`.
    config: NodeConfig,
    /// `None` for `PublicRpc`, and for `LightClient`/`FullNode` until
    /// `spawn()` is called or after `stop()`.
    process: Option<Box<dyn NodeProcess>>,
}

impl LocalNodeProcess {
    /// Builds an empty slot from the given config. Call `spawn()`
    /// afterward to launch the local node when the config calls for
    /// one.
    pub fn new(config: NodeConfig) -> Self {
        Self {
            config,
            process: None,
        }
    }

    /// Spawns the local node process for the active backend and stores
    /// the handle internally. Idempotent: a no-op when a process is
    /// already running, and a no-op for `PublicRpc`.
    pub fn spawn(&mut self) -> Result<(), NodeManagerError> {
        if self.process.is_some() {
            return Ok(());
        }
        let process: Box<dyn NodeProcess> = match self.config.node_type {
            NodeType::LightClient => Box::new(LightClientProcess::start(&self.config)?),
            NodeType::FullNode => Box::new(FullNodeProcess::start(&self.config)?),
            NodeType::PublicRpc => return Ok(()),
        };
        self.process = Some(process);
        Ok(())
    }

    /// Stops and drops the running local node process, if any. No-op
    /// when the slot is empty.
    pub fn stop(&mut self) {
        if let Some(mut proc) = self.process.take() {
            let _ = proc.stop();
        }
    }

    /// `true` when the slot is occupied — `spawn()` succeeded and
    /// `stop()` hasn't run. Not a strict liveness check; for true
    /// online-ness, probe the RPC via `QpClient`.
    pub fn has_local_process(&self) -> bool {
        self.process.is_some()
    }
}
