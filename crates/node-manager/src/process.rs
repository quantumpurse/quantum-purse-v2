use crate::config::{NodeConfig, NodeType};
use crate::error::NodeManagerError;
use std::fs::File;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long to wait for the node RPC to become reachable after starting.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait between RPC health-check polls during startup.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long to wait for the node to exit gracefully before sending SIGKILL.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Manages the lifecycle of a local CKB node process.
///
/// Only applicable for `NodeType::LightClient` and `NodeType::FullNode`.
/// For `NodeType::PublicRpc`, no process management is needed.
pub struct NodeProcess {
    child: Child,
    node_type: NodeType,
}

impl NodeProcess {
    /// Spawns the node binary as a child process and waits for its RPC endpoint to become reachable.
    ///
    /// Returns an error if `config.node_type` is `PublicRpc`, if `binary_path` is `None`,
    /// or if the binary does not exist at the configured path.
    pub fn start(config: &NodeConfig) -> Result<Self, NodeManagerError> {
        if !config.requires_binary() {
            return Err(NodeManagerError::UnsupportedOperation {
                node_type: config.node_type.to_string(),
                reason: "PublicRpc does not require a local process.".to_string(),
            });
        }

        let binary_path =
            config
                .binary_path
                .as_ref()
                .ok_or_else(|| NodeManagerError::BinaryNotFound {
                    path: "<not configured>".to_string(),
                    reason: "No binary path configured.".to_string(),
                })?;

        if !binary_path.exists() {
            return Err(NodeManagerError::BinaryNotFound {
                path: binary_path.display().to_string(),
                reason: "File does not exist.".to_string(),
            });
        }

        // Ensure the node data directory exists.
        let node_data_dir = config.node_data_dir();
        if !node_data_dir.exists() {
            std::fs::create_dir_all(&node_data_dir)?;
        }

        // Redirect the child's stdout/stderr to a log file so the signed
        // `.app` bundle case (where inherited stderr vanishes) still leaves
        // a paper trail. Truncated on each start — if the user needs older
        // logs they rotate them themselves.
        let log_path = node_data_dir.join("node.log");
        let log_file = File::create(&log_path).map_err(|e| {
            NodeManagerError::ProcessError(format!(
                "Failed to create node log at '{}': {}",
                log_path.display(),
                e
            ))
        })?;
        let log_file_err = log_file.try_clone().map_err(|e| {
            NodeManagerError::ProcessError(format!("Failed to clone log handle: {}", e))
        })?;

        let child = Command::new(binary_path)
            .arg("run")
            .arg("--config-file")
            .arg(node_data_dir.join("config.toml"))
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err))
            .spawn()
            .map_err(|e| {
                NodeManagerError::ProcessError(format!(
                    "Failed to spawn node binary '{}': {}",
                    binary_path.display(),
                    e
                ))
            })?;

        let mut process = Self {
            child,
            node_type: config.node_type,
        };

        // Wait for the RPC endpoint to become reachable.
        process.wait_for_rpc(&config.rpc_url)?;

        Ok(process)
    }

    /// Stops the running node process.
    ///
    /// On Unix, sends SIGTERM for graceful shutdown, then SIGKILL if the process
    /// does not exit within the grace period. On other platforms, kills immediately.
    pub fn stop(&mut self) -> Result<(), NodeManagerError> {
        // Send SIGTERM on Unix for graceful shutdown.
        send_terminate_signal(&self.child);

        // Wait for graceful exit within the grace period.
        let deadline = Instant::now() + SHUTDOWN_GRACE_PERIOD;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(NodeManagerError::ProcessError(format!(
                        "Error waiting for node to exit: {e}"
                    )));
                }
            }
        }

        // Force kill if still running after grace period.
        self.child.kill().map_err(|e| {
            NodeManagerError::ProcessError(format!("Failed to kill node process: {e}"))
        })?;
        self.child.wait().map_err(|e| {
            NodeManagerError::ProcessError(format!(
                "Error waiting for node process after kill: {e}"
            ))
        })?;

        Ok(())
    }

    /// Checks whether the node process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Returns the `NodeType` this process was started with.
    pub fn node_type(&self) -> NodeType {
        self.node_type
    }

    /// Switches from the currently running node to a different node type.
    ///
    /// Stops the current process, updates the config, and starts the new node.
    /// If `new_type` is `PublicRpc`, the current process is stopped and no new
    /// process is started — returns `Ok(None)`.
    pub fn switch_node_type(
        mut self,
        new_type: NodeType,
        config: &mut NodeConfig,
    ) -> Result<Option<Self>, NodeManagerError> {
        self.stop()?;

        config.node_type = new_type;
        config.rpc_url = config.default_rpc_url().to_string();
        config.save()?;

        if new_type == NodeType::PublicRpc {
            // Prevent Drop from trying to stop again.
            std::mem::forget(self);
            return Ok(None);
        }

        // Prevent Drop from trying to stop the already-stopped child.
        std::mem::forget(self);

        Ok(Some(Self::start(config)?))
    }

    /// Polls the RPC endpoint until it responds or the timeout is reached.
    fn wait_for_rpc(&mut self, rpc_url: &str) -> Result<(), NodeManagerError> {
        let addr = parse_host_port(rpc_url)?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;

        while Instant::now() < deadline {
            // Check if the process died during startup.
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(NodeManagerError::ProcessError(format!(
                    "Node process exited during startup with status: {status}"
                )));
            }

            // Try a TCP connect to check if the RPC port is open.
            if TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok() {
                return Ok(());
            }

            thread::sleep(POLL_INTERVAL);
        }

        // Timed out — kill the process and report failure.
        let _ = self.child.kill();
        let _ = self.child.wait();
        Err(NodeManagerError::ProcessError(format!(
            "Node RPC at {rpc_url} did not become reachable within {} seconds.",
            STARTUP_TIMEOUT.as_secs()
        )))
    }
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        // Best-effort cleanup: stop the node if the handle is dropped.
        let _ = self.stop();
    }
}

/// Sends a termination signal to the child process.
/// On Unix, sends SIGTERM. On other platforms, falls back to kill().
fn send_terminate_signal(child: &Child) {
    #[cfg(unix)]
    {
        let pid = child.id();
        // SAFETY: We own the child process and SIGTERM is a standard signal.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        // On non-Unix, there's no SIGTERM equivalent.
        // The caller will follow up with kill() after the grace period.
        let _ = child;
    }
}

/// Extracts a `SocketAddr` from an RPC URL for TCP health checks.
fn parse_host_port(rpc_url: &str) -> Result<std::net::SocketAddr, NodeManagerError> {
    // Strip the scheme prefix.
    let without_scheme = rpc_url
        .strip_prefix("https://")
        .or_else(|| rpc_url.strip_prefix("http://"))
        .unwrap_or(rpc_url);

    // Strip any trailing path.
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);

    // If no port is specified, default based on scheme.
    let addr_str = if host_port.contains(':') {
        host_port.to_string()
    } else if rpc_url.starts_with("https://") {
        format!("{host_port}:443")
    } else {
        format!("{host_port}:80")
    };

    addr_str.parse().map_err(|e| {
        NodeManagerError::ConfigError(format!(
            "Cannot parse RPC URL '{rpc_url}' as socket address: {e}"
        ))
    })
}
