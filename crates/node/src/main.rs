//! HyperCore Node - Main binary entry point
//!
//! This binary runs the complete HyperCore node including:
//! - CometBFT ABCI application
//! - HyperEVM execution environment with JSON-RPC server
//! - Gateway API server
//! - Indexer (optional)

use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use hypercore_chain::{AbciService, HyperCoreApp};
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_evm::{EvmExecutor, EvmRpcServer};
use hypercore_gateway::{GatewayConfig, GatewayServer};
use hypercore_primitives::new_shared_unified_state;

/// HyperCore Node CLI
#[derive(Parser)]
#[command(name = "hypercore")]
#[command(about = "HyperCore - High-performance perpetual futures exchange")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the node
    Start {
        /// Gateway HTTP listen address (Phase 2A: runs in same process as EVM RPC for shared unified state)
        #[arg(long, default_value = "0.0.0.0:3000", env = "HTTP_ADDR")]
        http_addr: SocketAddr,

        /// CometBFT ABCI listen address
        #[arg(long, default_value = "0.0.0.0:26658", env = "ABCI_ADDR")]
        abci_addr: SocketAddr,

        /// EVM JSON-RPC listen address
        #[arg(long, default_value = "0.0.0.0:8545", env = "EVM_RPC_ADDR")]
        evm_rpc_addr: SocketAddr,

        /// Chain ID
        #[arg(long, default_value = "1337", env = "CHAIN_ID")]
        chain_id: u64,

        /// Enable indexer
        #[arg(long)]
        enable_indexer: bool,

        /// Database URL (for indexer)
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,

        /// Log level
        #[arg(long, default_value = "info", env = "RUST_LOG")]
        log_level: String,
    },

    /// Initialize genesis state
    Init {
        /// Output genesis file path
        #[arg(long, default_value = "genesis.json")]
        output: String,

        /// Chain ID
        #[arg(long, default_value = "1337")]
        chain_id: u64,
    },

    /// Export state snapshot
    Export {
        /// Output file path
        #[arg(long)]
        output: String,

        /// Block height (default: latest)
        #[arg(long)]
        height: Option<u64>,
    },

    /// Import state from snapshot
    Import {
        /// Input file path
        #[arg(long)]
        input: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            http_addr,
            abci_addr,
            evm_rpc_addr,
            chain_id,
            enable_indexer,
            database_url,
            log_level,
        } => {
            // Initialize logging
            init_logging(&log_level);

            tracing::info!("Starting HyperCore node");
            tracing::info!("Chain ID: {}", chain_id);
            tracing::info!("Gateway HTTP: {}", http_addr);
            tracing::info!("EVM RPC: {}", evm_rpc_addr);
            tracing::info!("ABCI: {}", abci_addr);

            // === Phase 2A: Shared Unified State ===
            // Create a single unified state that is shared between SpotEngine and EVM.
            // This ensures both layers operate on the same master balance sheet.
            let unified_state = new_shared_unified_state();
            tracing::info!("Created shared unified state for HyperCore and HyperEVM");

            // Create shared engine state (perpetuals)
            let engine = Arc::new(RwLock::new(EngineState::new()));

            // Create spot engine with shared unified state
            let spot_engine = Arc::new(RwLock::new(
                SpotEngine::with_unified_state(Arc::clone(&unified_state))
            ));

            // Initialize with default markets
            {
                let mut eng = engine.write().await;
                initialize_default_markets(&mut eng)?;
            }

            // Initialize spot engine with default tokens
            {
                let mut spot_eng = spot_engine.write().await;
                initialize_spot_markets(&mut spot_eng)?;
            }

            // Create HyperCore application
            let app = Arc::new(RwLock::new(HyperCoreApp::new()));

            // Create EVM executor with shared unified state
            let evm = Arc::new(RwLock::new(
                EvmExecutor::with_unified_state(Arc::clone(&engine), Arc::clone(&unified_state), chain_id)
            ));

            // Create EVM RPC server
            let evm_rpc = EvmRpcServer::new(Arc::clone(&evm), chain_id);

            // Create mempool for transaction submission
            let mempool = hypercore_chain::SharedMempool::new();

            // Create gateway server with chain integration
            let gateway_config = GatewayConfig {
                http_addr,
                enable_websocket: true,
                chain_id,
                block_time_ms: 500, // 500ms blocks
            };
            let gateway = GatewayServer::new(
                gateway_config,
                Arc::clone(&engine),
                Arc::clone(&spot_engine),
                mempool.clone(),
                Arc::clone(&app),
            );
            let ws_manager = gateway.ws_manager();

            // Set up graceful shutdown
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

            // Handle Ctrl+C
            tokio::spawn(async move {
                tokio::signal::ctrl_c()
                    .await
                    .expect("Failed to listen for ctrl+c");
                tracing::info!("Received shutdown signal");
                let _ = shutdown_tx.send(());
            });

            // Start gateway server
            let gateway_handle = tokio::spawn(async move {
                if let Err(e) = gateway.run_with_shutdown(async {
                    let _ = shutdown_rx.await;
                }).await {
                    tracing::error!("Gateway error: {}", e);
                }
            });

            // Start EVM RPC server
            let evm_rpc_handle = tokio::spawn(async move {
                match evm_rpc.start(evm_rpc_addr).await {
                    Ok(handle) => {
                        tracing::info!("EVM RPC server started on {}", evm_rpc_addr);
                        // Keep the handle alive (the server runs until stopped)
                        handle.stopped().await;
                        tracing::info!("EVM RPC server stopped");
                    }
                    Err(e) => {
                        tracing::error!("Failed to start EVM RPC server: {}", e);
                    }
                }
            });

            // Start BlockProducer for single-node consensus
            // In production, this would be replaced with CometBFT integration via ABCI
            let block_producer = hypercore_chain::BlockProducer::new(
                Arc::clone(&app),
                mempool,
                hypercore_chain::BlockProducerConfig {
                    block_time_ms: 500,
                    ..Default::default()
                },
            );
            let _abci_service = AbciService::new(Arc::clone(&app)); // Keep for future ABCI integration
            let abci_handle = tokio::spawn(async move {
                tracing::info!("BlockProducer started with 500ms block time");
                block_producer.start().await;
            });

            // Start indexer if enabled
            if enable_indexer {
                if let Some(db_url) = database_url {
                    tracing::info!("Starting indexer with database: {}", db_url);
                    // In production, start the indexer service
                } else {
                    tracing::warn!("Indexer enabled but no DATABASE_URL provided");
                }
            }

            // Start price feed updater (mock for devnet)
            let price_engine = Arc::clone(&engine);
            let _price_handle = tokio::spawn(async move {
                mock_price_feed(price_engine).await;
            });

            // Start funding rate processor
            let funding_engine = Arc::clone(&engine);
            let _funding_handle = tokio::spawn(async move {
                funding_processor(funding_engine).await;
            });

            // Wait for services
            tokio::select! {
                _ = gateway_handle => {
                    tracing::info!("Gateway stopped");
                }
                _ = evm_rpc_handle => {
                    tracing::info!("EVM RPC server stopped");
                }
                _ = abci_handle => {
                    tracing::info!("ABCI server stopped");
                }
            }

            tracing::info!("HyperCore node shutdown complete");
            Ok(())
        }

        Commands::Init { output, chain_id } => {
            init_logging("info");

            let genesis = create_genesis(chain_id)?;
            let json = serde_json::to_string_pretty(&genesis)?;

            std::fs::write(&output, json)?;
            tracing::info!("Genesis file written to {}", output);

            Ok(())
        }

        Commands::Export { output, height } => {
            init_logging("info");
            tracing::info!("Exporting state to {}", output);

            // In production, load state from persistent storage
            // and export at the specified height

            Ok(())
        }

        Commands::Import { input } => {
            init_logging("info");
            tracing::info!("Importing state from {}", input);

            // In production, load snapshot and restore state

            Ok(())
        }
    }
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

/// Initialize default markets
fn initialize_default_markets(engine: &mut EngineState) -> anyhow::Result<()> {
    use hypercore_primitives::{Decimal, Market, MarketConfig};

    // BTC-PERP
    let btc_market = Market::new(
        MarketConfig::btc_perp(),
        Decimal::price("65000"),
        0,
    );
    engine.add_market(btc_market);

    // ETH-PERP
    let eth_market = Market::new(
        MarketConfig::eth_perp(),
        Decimal::price("3500"),
        0,
    );
    engine.add_market(eth_market);

    tracing::info!("Initialized 2 markets: BTC-PERP, ETH-PERP");
    Ok(())
}

/// Initialize spot markets (HIP-1 tokens)
fn initialize_spot_markets(spot_engine: &mut SpotEngine) -> anyhow::Result<()> {
    use hypercore_primitives::{Decimal, SpotTokenDeployParams, AccountAddress};

    // USDC is implicitly at index 0
    tracing::info!("USDC token at index 0 (implicit)");

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
        genesis_allocations: vec![], // No genesis allocations for devnet
        erc20_address: None, // No linked ERC20 contract
    };

    // Deploy test token (automatically creates TEST-USDC market)
    match spot_engine.deploy_token(test_token_params, deployer, now) {
        Ok(token) => {
            tracing::info!("Deployed {} token at index {} with market {}-USDC",
                          token.symbol, token.index, token.symbol);
        }
        Err(e) => {
            tracing::warn!("Failed to deploy test token: {}", e);
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
        spot_engine.state.credit_balance(account, 0, usdc_amount);
        // Credit TEST tokens (token index 1)
        spot_engine.state.credit_balance(account, 1, test_token_amount);
        tracing::info!("Credited {} ({:?}) with {} USDC and {} TEST",
                      name, account, usdc_amount.to_string_trimmed(), test_token_amount.to_string_trimmed());
    }

    tracing::info!("Initialized spot engine with test balances");
    Ok(())
}

/// Create genesis state
fn create_genesis(chain_id: u64) -> anyhow::Result<serde_json::Value> {
    use hypercore_primitives::Decimal;

    Ok(serde_json::json!({
        "chain_id": chain_id,
        "initial_height": 1,
        "consensus_params": {
            "block": {
                "max_bytes": 22020096,
                "max_gas": -1
            },
            "evidence": {
                "max_age_num_blocks": 100000,
                "max_age_duration": 172800000000000_i64,
                "max_bytes": 1048576
            },
            "validator": {
                "pub_key_types": ["ed25519"]
            }
        },
        "app_state": {
            "markets": [
                {
                    "id": 0,
                    "config": {
                        "name": "BTC-PERP",
                        "tick_size": "0.1",
                        "lot_size": "0.001",
                        "max_leverage": 50,
                        "initial_margin_fraction": "0.02",
                        "maintenance_margin_fraction": "0.01",
                        "maker_fee": "0.0002",
                        "taker_fee": "0.0005",
                        "funding_interval": 28800,
                        "max_funding_rate": "0.0005"
                    }
                },
                {
                    "id": 1,
                    "config": {
                        "name": "ETH-PERP",
                        "tick_size": "0.01",
                        "lot_size": "0.01",
                        "max_leverage": 50,
                        "initial_margin_fraction": "0.02",
                        "maintenance_margin_fraction": "0.01",
                        "maker_fee": "0.0002",
                        "taker_fee": "0.0005",
                        "funding_interval": 28800,
                        "max_funding_rate": "0.0005"
                    }
                }
            ],
            "balances": {}
        }
    }))
}

/// Mock price feed for devnet (simulates price movements)
async fn mock_price_feed(engine: Arc<RwLock<EngineState>>) {
    use rand::Rng;
    use hypercore_primitives::Decimal;

    let mut btc_price = 50000.0_f64;
    let mut eth_price = 3000.0_f64;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Random walk using simple deterministic fluctuation
        // (Using thread_rng before await causes Send issues)
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as f64;
        let btc_fluctuation = ((time / 1000.0).sin() * 0.0005) as f64;
        let eth_fluctuation = ((time / 1000.0).cos() * 0.0005) as f64;

        btc_price *= 1.0 + btc_fluctuation;
        eth_price *= 1.0 + eth_fluctuation;

        // Update mark prices - TODO: implement update_mark_price on EngineState
        {
            let mut eng = engine.write().await;
            if let Some(market) = eng.get_market_mut(0) {
                if let Ok(price) = Decimal::from_str(&format!("{:.2}", btc_price)) {
                    market.state.mark_price = price;
                }
            }
        }
        {
            let mut eng = engine.write().await;
            if let Some(market) = eng.get_market_mut(1) {
                if let Ok(price) = Decimal::from_str(&format!("{:.2}", eth_price)) {
                    market.state.mark_price = price;
                }
            }
        }
    }
}

/// Funding rate processor
async fn funding_processor(_engine: Arc<RwLock<EngineState>>) {
    // Check funding every minute
    let interval = tokio::time::Duration::from_secs(60);

    loop {
        tokio::time::sleep(interval).await;

        // TODO: Implement funding rate application
        // For now, just log that we would apply funding
        tracing::debug!("Funding check triggered (not implemented)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_genesis() {
        let genesis = create_genesis(1337).unwrap();
        assert_eq!(genesis["chain_id"], 1337);
        assert!(genesis["app_state"]["markets"].as_array().unwrap().len() == 2);
    }
}
