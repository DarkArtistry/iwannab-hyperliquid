//! HyperCore Gateway - Standalone API server binary with BlockProducer
//!
//! Phase 2B: This binary now integrates with the chain crate for proper
//! transaction flow through blocks.
//!
//! ## Architecture
//!
//! ```text
//! Client ──HTTP/WS──▶ Gateway ──submit tx──▶ Mempool
//!                         │                     │
//!                         │                     ▼
//!                         │              BlockProducer
//!                         │                     │
//!                         │                     ▼
//!                         │              HyperCoreApp
//!                         │                     │
//!                         │         ┌───────────┴───────────┐
//!                         │         ▼                       ▼
//!                         │    EngineState             SpotEngine
//!                         │    (read info)             (read info)
//!                         ▼
//!                   Info Queries
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use hypercore_chain::{BlockProducer, BlockProducerConfig, HyperCoreApp, SharedMempool};
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_gateway::{GatewayConfig, GatewayServer, RateLimitConfig, ValidationConfig};
use hypercore_primitives::{AccountAddress, Decimal, Market, MarketConfig, SpotTokenDeployParams};

/// HyperCore Gateway CLI
#[derive(Parser)]
#[command(name = "hypercore-gateway")]
#[command(about = "HyperCore Gateway - API server for the exchange")]
#[command(version)]
struct Cli {
    /// HTTP listen address
    #[arg(long, default_value = "0.0.0.0:3000", env = "HTTP_ADDR")]
    http_addr: SocketAddr,

    /// Chain ID
    #[arg(long, default_value = "1337", env = "CHAIN_ID")]
    chain_id: u64,

    /// Enable WebSocket
    #[arg(long, default_value = "true")]
    enable_websocket: bool,

    /// Block time in milliseconds
    #[arg(long, default_value = "500", env = "BLOCK_TIME_MS")]
    block_time_ms: u64,

    /// Log level
    #[arg(long, default_value = "info", env = "RUST_LOG")]
    log_level: String,

    /// Disable rate limiting (for development/testing)
    #[arg(long, default_value = "false", env = "DISABLE_RATE_LIMIT")]
    disable_rate_limit: bool,

    /// Use development rate limits (more permissive)
    #[arg(long, default_value = "false", env = "DEV_RATE_LIMIT")]
    dev_rate_limit: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(&cli.log_level);

    tracing::info!("Starting HyperCore Gateway (Phase 2B)");
    tracing::info!("HTTP address: {}", cli.http_addr);
    tracing::info!("Chain ID: {}", cli.chain_id);
    tracing::info!("Block time: {}ms", cli.block_time_ms);

    // Create shared state components
    let engine = Arc::new(RwLock::new(EngineState::new()));
    let spot_engine = Arc::new(RwLock::new(SpotEngine::new()));
    let mempool = SharedMempool::new();

    // Initialize default perpetual markets
    initialize_markets(&engine).await;

    // Initialize spot markets and test balances
    initialize_spot_markets(&spot_engine).await;

    // Create HyperCoreApp
    // Note: In Phase 2B, we use a simplified app that doesn't need
    // the full shared state wiring yet. The app.rs transaction handlers
    // are designed to work with shared state when connected.
    let app = Arc::new(RwLock::new(HyperCoreApp::new()));

    // Create rate limit config based on CLI flags
    let rate_limit = if cli.disable_rate_limit {
        tracing::warn!("Rate limiting is DISABLED via CLI flag");
        RateLimitConfig::disabled()
    } else if cli.dev_rate_limit {
        tracing::info!("Using development rate limits (relaxed)");
        RateLimitConfig::development()
    } else {
        tracing::info!("Using default rate limits");
        RateLimitConfig::default()
    };

    // Create gateway config
    let config = GatewayConfig {
        http_addr: cli.http_addr,
        enable_websocket: cli.enable_websocket,
        chain_id: cli.chain_id,
        block_time_ms: cli.block_time_ms,
        rate_limit,
        validation: ValidationConfig::default(),
    };

    // Create gateway server with all components
    let server = GatewayServer::new(
        config,
        Arc::clone(&engine),
        Arc::clone(&spot_engine),
        mempool.clone(),
        Arc::clone(&app),
    );

    // Create and start the BlockProducer in background
    let block_producer = BlockProducer::new(
        Arc::clone(&app),
        mempool,
        BlockProducerConfig {
            block_time_ms: cli.block_time_ms,
            ..Default::default()
        },
    );

    // Spawn BlockProducer task
    let producer_handle = tokio::spawn(async move {
        block_producer.start().await;
    });

    tracing::info!("BlockProducer started in background");

    // Handle shutdown signal
    let shutdown = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C handler");
        tracing::info!("Shutdown signal received");
    };

    // Run gateway server
    let server_result = server.run_with_shutdown(shutdown).await;

    // Stop block producer (it will stop on next tick since we've shut down)
    producer_handle.abort();

    tracing::info!("Gateway shutdown complete");
    server_result.map_err(Into::into)
}

/// Initialize logging
fn init_logging(level: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Initialize default perpetual markets
async fn initialize_markets(engine: &Arc<RwLock<EngineState>>) {
    let mut eng = engine.write().await;

    // BTC-PERP
    let btc_market = Market::new(
        MarketConfig::btc_perp(),
        Decimal::price("65000"),
        0,
    );
    eng.add_market(btc_market);

    // ETH-PERP
    let eth_market = Market::new(
        MarketConfig::eth_perp(),
        Decimal::price("3500"),
        0,
    );
    eng.add_market(eth_market);

    tracing::info!("Initialized perpetual markets: BTC-PERP, ETH-PERP");
}

/// Initialize spot markets (HIP-1 tokens) and test account balances
async fn initialize_spot_markets(spot_engine: &Arc<RwLock<SpotEngine>>) {
    let mut eng = spot_engine.write().await;

    // Deploy a test token (TEST at index 1)
    // The market TEST-USDC is automatically created when the token is deployed
    let deployer = AccountAddress::from([0u8; 20]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let test_token_params = SpotTokenDeployParams {
        name: "Test Token".to_string(),
        symbol: "TEST".to_string(),
        wei_decimals: 18,
        sz_decimals: 4,
        max_supply: Decimal::from_raw(1_000_000_000_000_000_000_000_000_000i128, 18), // 1 billion
        genesis_allocations: vec![],
        erc20_address: None,
    };

    // Deploy test token (automatically creates TEST-USDC market)
    match eng.deploy_token(test_token_params, deployer, now) {
        Ok(token) => {
            tracing::info!("Deployed {} token at index {} with market {}-USDC",
                          token.symbol, token.index, token.symbol);
        }
        Err(e) => {
            tracing::warn!("Failed to deploy test token: {}", e);
            return;
        }
    }

    // Credit test accounts with USDC and TEST for E2E testing
    // These are the standard Anvil/Hardhat test accounts
    let test_accounts: [(AccountAddress, &str); 3] = [
        // Alice - 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
        (AccountAddress::from([
            0xf3, 0x9F, 0xd6, 0xe5, 0x1a, 0xad, 0x88, 0xF6, 0xF4, 0xce,
            0x6a, 0xB8, 0x82, 0x72, 0x79, 0xcf, 0xfF, 0xb9, 0x22, 0x66
        ]), "Alice"),
        // Bob - 0x70997970C51812dc3A010C7d01b50e0d17dc79C8
        (AccountAddress::from([
            0x70, 0x99, 0x79, 0x70, 0xC5, 0x18, 0x12, 0xdc, 0x3A, 0x01,
            0x0C, 0x7d, 0x01, 0xb5, 0x0e, 0x0d, 0x17, 0xdc, 0x79, 0xC8
        ]), "Bob"),
        // Charlie - 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC
        (AccountAddress::from([
            0x3C, 0x44, 0xCd, 0xDd, 0xB6, 0xa9, 0x00, 0xfa, 0x2b, 0x58,
            0x5d, 0xd2, 0x99, 0xe0, 0x3d, 0x12, 0xFA, 0x42, 0x93, 0xBC
        ]), "Charlie"),
    ];

    let usdc_amount = Decimal::from_str_exact("100000", 6).unwrap(); // 100,000 USDC
    let test_token_amount = Decimal::from_str_exact("10000", 18).unwrap(); // 10,000 TEST

    for (account, name) in test_accounts {
        // Credit USDC (token index 0)
        eng.state.credit_balance(account, 0, usdc_amount);
        // Credit TEST tokens (token index 1)
        eng.state.credit_balance(account, 1, test_token_amount);
        tracing::info!("Credited {} with {} USDC and {} TEST",
                      name, usdc_amount.to_string_trimmed(), test_token_amount.to_string_trimmed());
    }

    tracing::info!("Initialized spot engine with TEST token and test balances");
}
