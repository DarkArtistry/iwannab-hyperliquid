//! HTTP/WebSocket server with BlockProducer integration
//!
//! Phase 2B: The gateway now integrates with the chain crate for proper
//! transaction flow. Exchange requests are converted to Transactions and
//! submitted to the mempool, where the BlockProducer picks them up.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use hypercore_chain::{BlockProducer, HyperCoreApp};
use hypercore_chain::mempool::SharedMempool;
use hypercore_engine::{EngineState, SpotEngine};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::handlers::{handle_exchange, handle_info};
use crate::websocket::{ws_handler, WsManager};

/// Gateway server configuration
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// HTTP listen address
    pub http_addr: SocketAddr,
    /// Enable WebSocket
    pub enable_websocket: bool,
    /// Chain ID
    pub chain_id: u64,
    /// Block time in milliseconds (for BlockProducer)
    pub block_time_ms: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            http_addr: "0.0.0.0:4000".parse().unwrap(),
            enable_websocket: true,
            chain_id: 1337,
            block_time_ms: 500, // 500ms blocks
        }
    }
}

/// Shared application state
///
/// Phase 2B: Now includes chain components for proper transaction flow.
#[derive(Clone)]
pub struct AppState {
    /// Engine state for perpetual markets (read access for info queries)
    pub engine: Arc<RwLock<EngineState>>,
    /// Spot engine for HIP-1 tokens (read access for info queries)
    pub spot_engine: Arc<RwLock<SpotEngine>>,
    /// WebSocket manager for broadcasting updates
    pub ws_manager: Arc<WsManager>,
    /// Chain ID
    pub chain_id: u64,
    /// Mempool for submitting transactions
    pub mempool: SharedMempool,
    /// HyperCore application (for direct execution in development mode)
    pub app: Arc<RwLock<HyperCoreApp>>,
}

/// Gateway server
pub struct GatewayServer {
    config: GatewayConfig,
    engine: Arc<RwLock<EngineState>>,
    spot_engine: Arc<RwLock<SpotEngine>>,
    ws_manager: Arc<WsManager>,
    mempool: SharedMempool,
    app: Arc<RwLock<HyperCoreApp>>,
}

impl GatewayServer {
    /// Create new gateway server with chain integration
    pub fn new(
        config: GatewayConfig,
        engine: Arc<RwLock<EngineState>>,
        spot_engine: Arc<RwLock<SpotEngine>>,
        mempool: SharedMempool,
        app: Arc<RwLock<HyperCoreApp>>,
    ) -> Self {
        Self {
            config,
            engine,
            spot_engine,
            ws_manager: Arc::new(WsManager::new()),
            mempool,
            app,
        }
    }

    /// Create new gateway server (legacy mode without chain integration)
    ///
    /// This creates local EngineState/SpotEngine instances and a default app.
    /// Use `new()` for production with shared state.
    pub fn standalone(config: GatewayConfig) -> Self {
        let engine = Arc::new(RwLock::new(EngineState::new()));
        let spot_engine = Arc::new(RwLock::new(SpotEngine::new()));
        let mempool = SharedMempool::new();
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));

        Self {
            config,
            engine,
            spot_engine,
            ws_manager: Arc::new(WsManager::new()),
            mempool,
            app,
        }
    }

    /// Get the mempool for submitting transactions externally
    pub fn mempool(&self) -> SharedMempool {
        self.mempool.clone()
    }

    /// Get WebSocket manager for broadcasting updates
    pub fn ws_manager(&self) -> Arc<WsManager> {
        Arc::clone(&self.ws_manager)
    }

    /// Get the HyperCore app
    pub fn app(&self) -> Arc<RwLock<HyperCoreApp>> {
        Arc::clone(&self.app)
    }

    /// Create a BlockProducer for this gateway's state
    pub fn create_block_producer(&self) -> BlockProducer {
        use hypercore_chain::BlockProducerConfig;

        BlockProducer::new(
            Arc::clone(&self.app),
            self.mempool.clone(),
            BlockProducerConfig {
                block_time_ms: self.config.block_time_ms,
                ..Default::default()
            },
        )
    }

    /// Build the router
    fn build_router(&self) -> Router {
        let app_state = AppState {
            engine: Arc::clone(&self.engine),
            spot_engine: Arc::clone(&self.spot_engine),
            ws_manager: Arc::clone(&self.ws_manager),
            chain_id: self.config.chain_id,
            mempool: self.mempool.clone(),
            app: Arc::clone(&self.app),
        };

        // CORS configuration
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let mut router = Router::new()
            // Info endpoint
            .route("/info", post(handle_info))
            // Exchange endpoint
            .route("/exchange", post(handle_exchange))
            // Health check
            .route("/health", get(health_check));

        // Add WebSocket endpoint if enabled
        if self.config.enable_websocket {
            router = router.route("/ws", get(ws_handler));
        }

        router
            .with_state(app_state)
            .layer(cors)
            .layer(TraceLayer::new_for_http())
    }

    /// Run the server
    pub async fn run(self) -> Result<(), GatewayError> {
        let router = self.build_router();
        let addr = self.config.http_addr;

        tracing::info!("Starting gateway server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::BindError(e.to_string()))?;

        axum::serve(listener, router)
            .await
            .map_err(|e| GatewayError::ServerError(e.to_string()))?;

        Ok(())
    }

    /// Run with graceful shutdown
    pub async fn run_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<(), GatewayError> {
        let router = self.build_router();
        let addr = self.config.http_addr;

        tracing::info!("Starting gateway server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::BindError(e.to_string()))?;

        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e| GatewayError::ServerError(e.to_string()))?;

        Ok(())
    }
}

/// Health check handler
async fn health_check() -> &'static str {
    "OK"
}

/// Gateway errors
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("Failed to bind: {0}")]
    BindError(String),
    #[error("Server error: {0}")]
    ServerError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GatewayConfig::default();
        assert_eq!(config.http_addr.port(), 4000);
        assert!(config.enable_websocket);
        assert_eq!(config.block_time_ms, 500);
    }

    #[tokio::test]
    async fn test_standalone_server_creation() {
        let config = GatewayConfig::default();
        let server = GatewayServer::standalone(config);

        // Verify WebSocket manager is created
        let ws = server.ws_manager();
        assert!(Arc::strong_count(&ws) >= 2);

        // Verify we can create a block producer
        let _producer = server.create_block_producer();
    }
}
