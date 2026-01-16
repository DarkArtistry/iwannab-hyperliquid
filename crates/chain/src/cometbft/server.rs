//! CometBFT ABCI Server
//!
//! This module provides a TCP-based ABCI server that CometBFT can connect to.

use std::net::SocketAddr;
use tendermint_abci::ServerBuilder;
use tracing::{info, error};

use super::app::CometBftApp;

/// CometBFT ABCI Server
///
/// Wraps the tendermint-abci server and provides lifecycle management.
pub struct CometBftServer {
    /// The application to serve
    app: CometBftApp,
}

impl CometBftServer {
    /// Create a new ABCI server with the given application
    pub fn new(app: CometBftApp) -> Self {
        Self { app }
    }

    /// Start the ABCI server and block until shutdown
    ///
    /// This starts a TCP server that CometBFT connects to. The server
    /// handles all ABCI requests and delegates to the application.
    ///
    /// # Arguments
    /// * `addr` - The socket address to listen on (e.g., "0.0.0.0:26658")
    ///
    /// # Returns
    /// Returns an error if the server fails to start or encounters a fatal error.
    pub fn start(self, addr: SocketAddr) -> Result<(), CometBftServerError> {
        info!("Starting CometBFT ABCI server on {}", addr);

        let server = ServerBuilder::default()
            .bind(addr.to_string(), self.app)
            .map_err(|e| CometBftServerError::BindError(e.to_string()))?;

        info!("CometBFT ABCI server bound to {}", addr);
        info!("Waiting for CometBFT to connect...");

        server
            .listen()
            .map_err(|e| CometBftServerError::ListenError(e.to_string()))?;

        Ok(())
    }

    /// Start the ABCI server in a background thread
    ///
    /// Unlike `start()`, this method returns immediately after starting
    /// the server in a background thread.
    ///
    /// # Arguments
    /// * `addr` - The socket address to listen on
    ///
    /// # Returns
    /// Returns a handle that can be used to wait for or stop the server.
    pub fn start_background(self, addr: SocketAddr) -> Result<ServerHandle, CometBftServerError> {
        info!("Starting CometBFT ABCI server on {} (background)", addr);

        let (shutdown_tx, _shutdown_rx) = std::sync::mpsc::channel::<()>();

        let handle = std::thread::Builder::new()
            .name("abci-server".to_string())
            .spawn(move || {
                let server = match ServerBuilder::default()
                    .bind(addr.to_string(), self.app)
                {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to bind ABCI server: {}", e);
                        return;
                    }
                };

                info!("CometBFT ABCI server listening on {}", addr);

                if let Err(e) = server.listen() {
                    error!("ABCI server error: {}", e);
                }
            })
            .map_err(|e| CometBftServerError::SpawnError(e.to_string()))?;

        Ok(ServerHandle {
            thread: Some(handle),
            shutdown_tx,
        })
    }
}

/// Handle to a running ABCI server
pub struct ServerHandle {
    thread: Option<std::thread::JoinHandle<()>>,
    shutdown_tx: std::sync::mpsc::Sender<()>,
}

impl ServerHandle {
    /// Wait for the server to stop
    pub fn join(mut self) -> Result<(), CometBftServerError> {
        if let Some(handle) = self.thread.take() {
            handle.join()
                .map_err(|_| CometBftServerError::JoinError)?;
        }
        Ok(())
    }

    /// Signal the server to stop (note: current implementation may not support graceful shutdown)
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // Try to send shutdown signal
        let _ = self.shutdown_tx.send(());
    }
}

/// ABCI server configuration
#[derive(Debug, Clone)]
pub struct CometBftServerConfig {
    /// Address to bind the ABCI server
    pub bind_addr: SocketAddr,
    /// Number of read threads (default: 1)
    pub read_threads: usize,
}

impl Default for CometBftServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:26658".parse().unwrap(),
            read_threads: 1,
        }
    }
}

/// CometBFT server errors
#[derive(Debug, thiserror::Error)]
pub enum CometBftServerError {
    #[error("Failed to bind to address: {0}")]
    BindError(String),
    #[error("Server listen error: {0}")]
    ListenError(String),
    #[error("Failed to spawn server thread: {0}")]
    SpawnError(String),
    #[error("Failed to join server thread")]
    JoinError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::HyperCoreApp;

    #[test]
    fn test_server_config_default() {
        let config = CometBftServerConfig::default();
        assert_eq!(config.bind_addr.port(), 26658);
        assert_eq!(config.read_threads, 1);
    }

    #[test]
    fn test_server_creation() {
        let app = HyperCoreApp::new();
        let cometbft_app = CometBftApp::new(app);
        let _server = CometBftServer::new(cometbft_app);
        // Server creation should not fail
    }
}
