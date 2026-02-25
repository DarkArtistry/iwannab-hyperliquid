//! HTTP/WebSocket server with BlockProducer integration
//!
//! Phase 2B: The gateway now integrates with the chain crate for proper
//! transaction flow. Exchange requests are converted to Transactions and
//! submitted to the mempool, where the BlockProducer picks them up.
//!
//! Phase 6: Added rate limiting middleware for DoS protection.

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

use crate::handlers::{handle_exchange, handle_exchange_batch, handle_info};
use crate::rate_limit::{RateLimiter, RateLimitConfig};
use crate::tx_router::TxRouter;
use crate::validation::{ValidationConfig, Validator};
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
    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,
    /// Input validation configuration
    pub validation: ValidationConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            http_addr: "0.0.0.0:4000".parse().unwrap(),
            enable_websocket: true,
            chain_id: 1337,
            block_time_ms: 200, // 200ms blocks
            rate_limit: RateLimitConfig::default(),
            validation: ValidationConfig::default(),
        }
    }
}

impl GatewayConfig {
    /// Create a config with rate limiting disabled (for testing)
    pub fn without_rate_limit(mut self) -> Self {
        self.rate_limit = RateLimitConfig::disabled();
        self
    }

    /// Create a config with development rate limits (relaxed)
    pub fn with_dev_rate_limit(mut self) -> Self {
        self.rate_limit = RateLimitConfig::development();
        self
    }

    /// Create a config with production rate limits (strict)
    pub fn with_prod_rate_limit(mut self) -> Self {
        self.rate_limit = RateLimitConfig::production();
        self
    }

    /// Create a config with validation disabled (for testing)
    pub fn without_validation(mut self) -> Self {
        self.validation = ValidationConfig::disabled();
        self
    }

    /// Create a config with strict validation (for production)
    pub fn with_strict_validation(mut self) -> Self {
        self.validation = ValidationConfig::strict();
        self
    }
}

/// Shared application state
///
/// Phase 2B: Now includes chain components for proper transaction flow.
/// Phase 6: Added validator for input validation.
/// Phase 8: Added TxRouter for CometBFT multi-node support.
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
    /// HyperCore application (for direct execution and info queries)
    pub app: Arc<RwLock<HyperCoreApp>>,
    /// Input validator
    pub validator: Arc<Validator>,
    /// Transaction router (Direct for single-node, CometBft for multi-node)
    pub tx_router: TxRouter,
}

/// Gateway server
pub struct GatewayServer {
    config: GatewayConfig,
    engine: Arc<RwLock<EngineState>>,
    spot_engine: Arc<RwLock<SpotEngine>>,
    ws_manager: Arc<WsManager>,
    mempool: SharedMempool,
    app: Arc<RwLock<HyperCoreApp>>,
    rate_limiter: RateLimiter,
    validator: Arc<Validator>,
    tx_router: TxRouter,
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
        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let validator = Arc::new(Validator::new(config.validation.clone()));
        let tx_router = TxRouter::Direct(Arc::clone(&app));
        Self {
            config,
            engine,
            spot_engine,
            ws_manager: Arc::new(WsManager::new()),
            mempool,
            app,
            rate_limiter,
            validator,
            tx_router,
        }
    }

    /// Create new gateway server with CometBFT transaction routing
    ///
    /// In CometBFT mode, transactions are broadcast to the local CometBFT node
    /// instead of being executed directly. The engine/spot_engine are shared with
    /// the CometBFT ABCI app so info queries return up-to-date state.
    pub fn with_cometbft(
        config: GatewayConfig,
        engine: Arc<RwLock<EngineState>>,
        spot_engine: Arc<RwLock<SpotEngine>>,
        mempool: SharedMempool,
        app: Arc<RwLock<HyperCoreApp>>,
        tx_router: TxRouter,
    ) -> Self {
        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let validator = Arc::new(Validator::new(config.validation.clone()));
        Self {
            config,
            engine,
            spot_engine,
            ws_manager: Arc::new(WsManager::new()),
            mempool,
            app,
            rate_limiter,
            validator,
            tx_router,
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
        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let validator = Arc::new(Validator::new(config.validation.clone()));
        let tx_router = TxRouter::Direct(Arc::clone(&app));

        Self {
            config,
            engine,
            spot_engine,
            ws_manager: Arc::new(WsManager::new()),
            mempool,
            app,
            rate_limiter,
            validator,
            tx_router,
        }
    }

    /// Get the rate limiter for monitoring
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    /// Get the validator for monitoring
    pub fn validator(&self) -> &Validator {
        &self.validator
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
            validator: Arc::clone(&self.validator),
            tx_router: self.tx_router.clone(),
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
            // Batch exchange endpoint (multiple actions in one HTTP request)
            .route("/exchange/batch", post(handle_exchange_batch))
            // Binary exchange endpoint (Phase C3: ~6x less bandwidth)
            .route("/exchange/binary", post(crate::handlers::handle_exchange_binary))
            // Health check
            .route("/health", get(health_check));

        // Add WebSocket endpoint if enabled
        if self.config.enable_websocket {
            router = router.route("/ws", get(ws_handler));
        }

        // Apply layers in order: rate limiting -> CORS -> tracing
        // Rate limiting is first so it can reject requests before any processing
        router
            .with_state(app_state)
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .layer(self.rate_limiter.layer())
    }

    /// Run the server
    pub async fn run(self) -> Result<(), GatewayError> {
        let router = self.build_router();
        let addr = self.config.http_addr;

        // Start rate limiter cleanup task
        if self.config.rate_limit.enabled {
            self.rate_limiter.start_cleanup_task();
            tracing::info!(
                "Rate limiting enabled: {} req/min global, {} req/min for /exchange, {} req/min for /info",
                self.config.rate_limit.requests_per_minute,
                self.config.rate_limit.exchange_requests_per_minute,
                self.config.rate_limit.info_requests_per_minute
            );
        } else {
            tracing::warn!("Rate limiting is DISABLED");
        }

        tracing::info!("Starting gateway server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::BindError(e.to_string()))?;

        // Phase C2: Enable TCP_NODELAY for lower latency
        tracing::info!("TCP_NODELAY enabled for lower latency");

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

        // Start rate limiter cleanup task
        if self.config.rate_limit.enabled {
            self.rate_limiter.start_cleanup_task();
            tracing::info!(
                "Rate limiting enabled: {} req/min global, {} req/min for /exchange, {} req/min for /info",
                self.config.rate_limit.requests_per_minute,
                self.config.rate_limit.exchange_requests_per_minute,
                self.config.rate_limit.info_requests_per_minute
            );
        } else {
            tracing::warn!("Rate limiting is DISABLED");
        }

        tracing::info!("Starting gateway server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::BindError(e.to_string()))?;

        // Phase C2: Enable TCP_NODELAY for lower latency
        tracing::info!("TCP_NODELAY enabled for lower latency");

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
        assert_eq!(config.block_time_ms, 200);
        assert!(config.rate_limit.enabled);
    }

    #[test]
    fn test_config_without_rate_limit() {
        let config = GatewayConfig::default().without_rate_limit();
        assert!(!config.rate_limit.enabled);
    }

    #[test]
    fn test_config_with_dev_rate_limit() {
        let config = GatewayConfig::default().with_dev_rate_limit();
        assert!(config.rate_limit.enabled);
        assert_eq!(config.rate_limit.requests_per_minute, 1000);
    }

    #[test]
    fn test_config_with_prod_rate_limit() {
        let config = GatewayConfig::default().with_prod_rate_limit();
        assert!(config.rate_limit.enabled);
        assert_eq!(config.rate_limit.requests_per_minute, 60);
    }

    #[tokio::test]
    async fn test_standalone_server_creation() {
        let config = GatewayConfig::default();
        let server = GatewayServer::standalone(config);

        // Verify WebSocket manager is created
        let ws = server.ws_manager();
        assert!(Arc::strong_count(&ws) >= 2);

        // Verify rate limiter is created
        let limiter = server.rate_limiter();
        assert_eq!(limiter.state().tracked_ips(), 0);

        // Verify we can create a block producer
        let _producer = server.create_block_producer();
    }

    #[tokio::test]
    async fn test_server_with_disabled_rate_limiting() {
        let config = GatewayConfig::default().without_rate_limit();
        let server = GatewayServer::standalone(config);

        // Rate limiter should exist but be disabled
        let limiter = server.rate_limiter();
        assert!(!limiter.state().config().enabled);
    }
}
