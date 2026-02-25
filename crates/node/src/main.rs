//! HyperCore Node - Main binary entry point
//!
//! This binary runs the complete HyperCore node including:
//! - CometBFT ABCI application (multi-node mode) or BlockProducer (single-node mode)
//! - HyperEVM execution environment with JSON-RPC server
//! - Gateway API server
//! - Indexer (optional)
//!
//! ## Consensus Modes
//!
//! The node supports two consensus modes:
//!
//! ### Single-Node Mode (default)
//! Uses the built-in BlockProducer for fast development and testing.
//! No external CometBFT process is required.
//!
//! ### CometBFT Mode (requires `cometbft` feature)
//! Connects to an external CometBFT process via ABCI for Byzantine fault-tolerant
//! consensus in a multi-node network.

/// Phase E1: Use mimalloc as the global allocator for 10-20% throughput improvement
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use hypercore_chain::{AbciService, HyperCoreApp};
#[cfg(feature = "persistence")]
use hypercore_chain::{extract_state, restore_state, restore_chain_state, restore_perp_engine_state};
#[cfg(feature = "p2p")]
use libp2p::Multiaddr;
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_evm::{EvmExecutor, EvmRpcServer};
use hypercore_gateway::{GatewayConfig, GatewayServer, RateLimitConfig, ValidationConfig, EventBroadcaster};
use hypercore_primitives::{new_shared_unified_state, BlockEvent};

#[cfg(feature = "persistence")]
use hypercore_persistence::{PersistenceBackend, PersistenceConfig, RocksDbBackend, StatePersister};

/// Consensus mode for the node
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConsensusMode {
    /// Single-node mode using BlockProducer (default, for development)
    SingleNode,
    /// Multi-node mode using CometBFT ABCI (requires cometbft feature)
    #[cfg(feature = "cometbft")]
    CometBft,
}

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

        /// Consensus mode: single-node (BlockProducer) or cometbft (ABCI server)
        #[arg(long, default_value = "single-node", env = "CONSENSUS_MODE")]
        consensus_mode: ConsensusMode,

        /// Block time in milliseconds (only used in single-node mode)
        #[arg(long, default_value = "200", env = "BLOCK_TIME_MS")]
        block_time_ms: u64,

        /// Enable indexer
        #[arg(long)]
        enable_indexer: bool,

        /// Database URL (for indexer)
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,

        /// Log level
        #[arg(long, default_value = "info", env = "RUST_LOG")]
        log_level: String,

        /// CometBFT RPC URL for broadcasting transactions (required in cometbft mode)
        /// Each node should point to its local CometBFT instance's RPC endpoint.
        #[cfg(feature = "cometbft")]
        #[arg(long, env = "COMETBFT_RPC_URL")]
        cometbft_rpc_url: Option<String>,

        /// Enable state persistence (requires 'persistence' feature)
        #[cfg(feature = "persistence")]
        #[arg(long)]
        enable_persistence: bool,

        /// Data directory for persistent storage
        #[cfg(feature = "persistence")]
        #[arg(long, default_value = "./data/chain", env = "DATA_DIR")]
        data_dir: String,

        /// Enable P2P attestation gossip (requires 'p2p' feature)
        #[cfg(feature = "p2p")]
        #[arg(long)]
        enable_p2p_attestation: bool,

        /// P2P listen address for attestation gossip
        #[cfg(feature = "p2p")]
        #[arg(long, default_value = "/ip4/0.0.0.0/tcp/26670", env = "P2P_LISTEN_ADDR")]
        p2p_listen_addr: String,

        /// Bootstrap peers for P2P attestation gossip (comma-separated multiaddrs)
        #[cfg(feature = "p2p")]
        #[arg(long, env = "P2P_BOOTSTRAP_PEERS")]
        p2p_bootstrap_peers: Option<String>,
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

        /// Data directory for persistent storage
        #[cfg(feature = "persistence")]
        #[arg(long, default_value = "./data/chain", env = "DATA_DIR")]
        data_dir: String,
    },

    /// Import state from snapshot
    Import {
        /// Input file path
        #[arg(long)]
        input: String,

        /// Data directory for persistent storage
        #[cfg(feature = "persistence")]
        #[arg(long, default_value = "./data/chain", env = "DATA_DIR")]
        data_dir: String,
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
            consensus_mode,
            block_time_ms,
            enable_indexer,
            database_url: _,
            log_level,
            #[cfg(feature = "cometbft")]
            cometbft_rpc_url,
            #[cfg(feature = "persistence")]
            enable_persistence,
            #[cfg(feature = "persistence")]
            data_dir,
            #[cfg(feature = "p2p")]
            enable_p2p_attestation,
            #[cfg(feature = "p2p")]
            p2p_listen_addr,
            #[cfg(feature = "p2p")]
            p2p_bootstrap_peers,
        } => {
            // Initialize logging
            init_logging(&log_level);

            tracing::info!("Starting HyperCore node");
            tracing::info!("Chain ID: {}", chain_id);
            tracing::info!("Consensus mode: {:?}", consensus_mode);
            tracing::info!("Gateway HTTP: {}", http_addr);
            tracing::info!("EVM RPC: {}", evm_rpc_addr);
            tracing::info!("ABCI: {}", abci_addr);

            // === Phase 4: State Persistence (optional) ===
            #[cfg(feature = "persistence")]
            let persistence = if enable_persistence {
                tracing::info!("Persistence enabled, data directory: {}", data_dir);
                let config = PersistenceConfig {
                    data_dir: data_dir.clone(),
                    create_if_missing: true,
                    ..Default::default()
                };
                match RocksDbBackend::open(&config) {
                    Ok(db) => {
                        let height = db.get_height().unwrap_or(0);
                        tracing::info!("Opened persistence at height {}", height);
                        Some(Arc::new(db))
                    }
                    Err(e) => {
                        tracing::error!("Failed to open persistence: {}", e);
                        tracing::warn!("Continuing without persistence");
                        None
                    }
                }
            } else {
                tracing::info!("Persistence disabled (in-memory mode)");
                None
            };

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

            // Track if state was restored from persistence
            #[cfg(feature = "persistence")]
            let mut state_restored = false;
            #[cfg(feature = "persistence")]
            let mut restored_height: u64 = 0;
            #[cfg(feature = "persistence")]
            let mut restored_timestamp: u64 = 0;
            #[cfg(feature = "persistence")]
            let mut restored_app_hash: [u8; 32] = [0u8; 32];
            #[cfg(feature = "persistence")]
            let mut restored_chain_data: Option<hypercore_persistence::PersistedState> = None;

            // === Phase 4B: State Restore from Persistence ===
            #[cfg(feature = "persistence")]
            if let Some(ref db) = persistence {
                let persister = StatePersister::new(db.as_ref());
                match persister.load_state() {
                    Ok(Some(persisted_state)) => {
                        tracing::info!("Found persisted state at height {}, restoring...", persisted_state.height);

                        // Restore state
                        match restore_state(
                            &persisted_state,
                            &unified_state,
                            &engine,
                            Some(&spot_engine),
                        ) {
                            Ok((height, timestamp, app_hash)) => {
                                tracing::info!(
                                    "State restored: height={}, timestamp={}, app_hash={:?}",
                                    height, timestamp, &app_hash[..8]
                                );
                                state_restored = true;
                                restored_height = height;
                                restored_timestamp = timestamp;
                                restored_app_hash = app_hash;
                                restored_chain_data = Some(persisted_state);
                            }
                            Err(e) => {
                                tracing::error!("Failed to restore state: {}", e);
                                tracing::warn!("Falling back to genesis initialization");
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::info!("No persisted state found, initializing from genesis");
                    }
                    Err(e) => {
                        tracing::error!("Failed to load persisted state: {}", e);
                        tracing::warn!("Falling back to genesis initialization");
                    }
                }
            }

            // Initialize from genesis if state was not restored
            #[cfg(feature = "persistence")]
            let should_init_genesis = !state_restored;
            #[cfg(not(feature = "persistence"))]
            let should_init_genesis = true;

            // Skip local genesis initialization in CometBFT mode.
            // CometBFT's init_chain callback handles genesis from the consensus layer,
            // initializing markets, tokens, and balances through the ABCI app.
            #[cfg(feature = "cometbft")]
            let should_init_genesis = should_init_genesis && !matches!(consensus_mode, ConsensusMode::CometBft);

            if should_init_genesis {
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

                // Initialize balances from genesis state
                // This replaces the runtime creditBalance() approach
                {
                    let genesis = create_genesis(chain_id)?;
                    initialize_genesis_balances(&unified_state, &genesis)?;
                }
            }

            // Create full perpetuals engine with matching for order execution.
            // The Engine has its own EngineState (with orderbook, positions, fills)
            // separate from the gateway's EngineState (which has mark prices).
            // IMPORTANT: This must be created unconditionally so both single-node
            // and CometBFT modes have a working perp engine.
            //
            // When state is restored from persistence, use the restored mark prices
            // instead of hardcoded defaults to ensure consistency with the shared
            // EngineState (which was already restored).
            let perp_engine = {
                use hypercore_engine::{Engine, EngineConfig};
                let mut full_engine = Engine::with_unified_state(
                    EngineConfig::default(),
                    Arc::clone(&unified_state),
                );

                // Read mark prices from restored engine state (if available)
                // NOTE: Use try_read() instead of blocking_read() because we are inside
                // #[tokio::main] async context. blocking_read() panics in this context.
                #[cfg(feature = "persistence")]
                let restored_market_state = if state_restored {
                    let eng = engine.try_read()
                        .expect("Engine lock should be uncontested during startup");
                    let btc_price = eng.get_market(0)
                        .map(|m| m.state.mark_price)
                        .unwrap_or_else(|| hypercore_primitives::Decimal::price("65000"));
                    let eth_price = eng.get_market(1)
                        .map(|m| m.state.mark_price)
                        .unwrap_or_else(|| hypercore_primitives::Decimal::price("3500"));
                    let btc_funding_time = eng.get_market(0)
                        .map(|m| m.state.next_funding_time)
                        .unwrap_or(0);
                    let eth_funding_time = eng.get_market(1)
                        .map(|m| m.state.next_funding_time)
                        .unwrap_or(0);
                    tracing::info!(
                        "Using restored market state for perp engine: BTC price={} funding_time={}, ETH price={} funding_time={}",
                        btc_price.to_string_trimmed(),
                        btc_funding_time,
                        eth_price.to_string_trimmed(),
                        eth_funding_time,
                    );
                    Some((btc_price, eth_price, btc_funding_time, eth_funding_time))
                } else {
                    None
                };

                #[cfg(feature = "persistence")]
                let (btc_price, eth_price, btc_funding_time, eth_funding_time) = restored_market_state.unwrap_or_else(|| {
                    (
                        hypercore_primitives::Decimal::price("65000"),
                        hypercore_primitives::Decimal::price("3500"),
                        0,
                        0,
                    )
                });
                #[cfg(not(feature = "persistence"))]
                let (btc_price, eth_price, btc_funding_time, eth_funding_time) = (
                    hypercore_primitives::Decimal::price("65000"),
                    hypercore_primitives::Decimal::price("3500"),
                    0,
                    0,
                );

                full_engine.add_market(
                    hypercore_primitives::MarketConfig::btc_perp(),
                    btc_price,
                    btc_funding_time,
                );
                full_engine.add_market(
                    hypercore_primitives::MarketConfig::eth_perp(),
                    eth_price,
                    eth_funding_time,
                );

                Arc::new(RwLock::new(full_engine))
            };

            // Restore perp_engine state from persistence (positions, orders, leverage, market state)
            #[cfg(feature = "persistence")]
            if state_restored {
                if let Some(ref persisted) = restored_chain_data {
                    restore_perp_engine_state(persisted, &perp_engine);
                }
            }

            tracing::info!("Initialized perp engine with matching (BTC-PERP, ETH-PERP)");

            // Create HyperCore application with SHARED unified state
            // This ensures all layers (SpotEngine, EVM, HyperCoreApp) use the same balance sheet.
            // Without this, USD transfers and other app-layer operations would fail due to
            // seeing different balances than what was initialized in genesis.
            let app = Arc::new(RwLock::new(HyperCoreApp::with_shared_state(
                Arc::clone(&unified_state),
                Arc::clone(&engine),
                Some(Arc::clone(&spot_engine)),
            )));
            {
                let mut app_guard = app.write().await;
                app_guard.state.perp_engine = Some(Arc::clone(&perp_engine));
            }

            // Create EVM executor with shared unified state
            let evm = Arc::new(RwLock::new(
                EvmExecutor::with_unified_state(Arc::clone(&engine), Arc::clone(&unified_state), chain_id)
            ));

            // Restore EVM state from persistence (if available)
            // CRITICAL: Without this, restarted nodes have empty EVM state, causing
            // different evm_root in compute_app_hash() and consensus failure.
            #[cfg(feature = "persistence")]
            if state_restored {
                if let Some(ref persisted) = restored_chain_data {
                    use hypercore_evm::{EvmAddress, B256, EvmU256};

                    tracing::info!(
                        "EVM state from persistence: {} accounts, {} storage slots, {} code entries, {} block hashes",
                        persisted.evm.accounts.len(),
                        persisted.evm.storage.len(),
                        persisted.evm.code.len(),
                        persisted.evm.block_hashes.len(),
                    );

                    if !persisted.evm.accounts.is_empty() ||
                       !persisted.evm.storage.is_empty() ||
                       !persisted.evm.code.is_empty() {
                        let mut evm_guard = evm.write().await;
                        let evm_state = evm_guard.state_mut();

                        // Restore EVM accounts (address, nonce, code_hash)
                        for entry in &persisted.evm.accounts {
                            let address = EvmAddress::from_slice(&entry.address);
                            let code_hash = entry.code_hash.map(B256::from);
                            evm_state.restore_account(address, entry.nonce, code_hash);
                        }

                        // Restore EVM code (code_hash, bytecode) - BEFORE storage
                        // because accounts reference code by hash
                        for entry in &persisted.evm.code {
                            let code_hash = B256::from(entry.code_hash);
                            evm_state.restore_code(code_hash, entry.bytecode.clone());
                        }

                        // Restore EVM storage (address, slot, value)
                        for entry in &persisted.evm.storage {
                            let address = EvmAddress::from_slice(&entry.address);
                            let slot = EvmU256::from_be_bytes(entry.slot);
                            let value = EvmU256::from_be_bytes(entry.value);
                            evm_state.restore_storage(address, slot, value);
                        }

                        // Restore EVM block hashes
                        for entry in &persisted.evm.block_hashes {
                            let hash = B256::from(entry.hash);
                            evm_state.restore_block_hash(entry.height, hash);
                        }

                        tracing::info!(
                            "Restored EVM state: {} accounts, {} storage slots, {} code entries, {} block hashes",
                            persisted.evm.accounts.len(),
                            persisted.evm.storage.len(),
                            persisted.evm.code.len(),
                            persisted.evm.block_hashes.len(),
                        );
                    } else {
                        tracing::warn!("EVM state in persistence is empty - EVM executor will start fresh");
                    }
                }
            }

            // Create EVM RPC server
            // In CometBFT mode, route eth_sendRawTransaction through consensus.
            // Read operations (eth_call, eth_getBalance, etc.) still use the local executor.
            #[cfg(feature = "cometbft")]
            let evm_rpc = if matches!(consensus_mode, ConsensusMode::CometBft) {
                let rpc_url = cometbft_rpc_url.clone().unwrap_or_else(|| {
                    "http://localhost:26657".to_string()
                });
                tracing::info!("EVM RPC in CometBFT mode: broadcasting to {}", rpc_url);
                EvmRpcServer::with_cometbft(Arc::clone(&evm), chain_id, rpc_url)
            } else {
                EvmRpcServer::new(Arc::clone(&evm), chain_id)
            };
            #[cfg(not(feature = "cometbft"))]
            let evm_rpc = EvmRpcServer::new(Arc::clone(&evm), chain_id);

            // Extract shared EVM stores for CometBFT receipt storage
            #[cfg(feature = "cometbft")]
            let evm_shared_stores = if matches!(consensus_mode, ConsensusMode::CometBft) {
                Some((
                    evm_rpc.shared_receipts(),
                    evm_rpc.shared_transactions(),
                    evm_rpc.shared_block_number(),
                ))
            } else {
                None
            };

            // Create mempool for transaction submission
            let mempool = hypercore_chain::SharedMempool::new();

            // Create gateway server with chain integration
            // Use development rate limits and validation for the node (more permissive)
            let gateway_config = GatewayConfig {
                http_addr,
                enable_websocket: true,
                chain_id,
                block_time_ms: 200, // 200ms blocks
                rate_limit: RateLimitConfig::development(),
                validation: ValidationConfig::default(),
            };

            // In CometBFT mode, route transactions through CometBFT consensus.
            // In single-node mode, execute transactions directly.
            #[cfg(feature = "cometbft")]
            let gateway = if matches!(consensus_mode, ConsensusMode::CometBft) {
                let rpc_url = cometbft_rpc_url.clone().unwrap_or_else(|| {
                    tracing::warn!("No COMETBFT_RPC_URL provided, defaulting to http://localhost:26657");
                    "http://localhost:26657".to_string()
                });
                tracing::info!("CometBFT RPC URL for tx routing: {}", rpc_url);
                let tx_router = hypercore_gateway::TxRouter::CometBft(
                    hypercore_gateway::CometBftRpcClient::new(rpc_url),
                );
                GatewayServer::with_cometbft(
                    gateway_config,
                    Arc::clone(&engine),
                    Arc::clone(&spot_engine),
                    mempool.clone(),
                    Arc::clone(&app),
                    tx_router,
                )
            } else {
                GatewayServer::new(
                    gateway_config,
                    Arc::clone(&engine),
                    Arc::clone(&spot_engine),
                    mempool.clone(),
                    Arc::clone(&app),
                )
            };
            #[cfg(not(feature = "cometbft"))]
            let gateway = GatewayServer::new(
                gateway_config,
                Arc::clone(&engine),
                Arc::clone(&spot_engine),
                mempool.clone(),
                Arc::clone(&app),
            );
            let ws_manager = gateway.ws_manager();

            // === WebSocket Event Broadcasting ===
            // Create a channel to send block events from the post-commit handler
            // to an async task that broadcasts them via WebSocket
            let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Vec<serde_json::Value>>(100);

            // Market names for event broadcasting
            let market_names = vec!["BTC-PERP".to_string(), "ETH-PERP".to_string()];

            // Spawn event broadcasting task
            let broadcaster_ws = Arc::clone(&ws_manager);
            let broadcast_markets = market_names.clone();
            tokio::spawn(async move {
                let broadcaster = EventBroadcaster::new(broadcaster_ws, broadcast_markets);

                while let Some(events) = event_rx.recv().await {
                    for event_json in events {
                        // Parse JSON back to BlockEvent
                        match serde_json::from_value::<BlockEvent>(event_json) {
                            Ok(event) => {
                                broadcaster.broadcast_event(&event).await;
                            }
                            Err(e) => {
                                tracing::debug!("Failed to parse block event for broadcasting: {}", e);
                            }
                        }
                    }
                }
                tracing::info!("Event broadcaster task stopped");
            });
            tracing::info!("WebSocket event broadcasting enabled");

            // Set up graceful shutdown
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

            // Handle both SIGINT (Ctrl+C) and SIGTERM (docker stop)
            tokio::spawn(async move {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = signal(SignalKind::terminate())
                        .expect("Failed to listen for SIGTERM");
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            tracing::info!("Received SIGINT (Ctrl+C), shutting down...");
                        }
                        _ = sigterm.recv() => {
                            tracing::info!("Received SIGTERM (docker stop), shutting down...");
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    tokio::signal::ctrl_c()
                        .await
                        .expect("Failed to listen for ctrl+c");
                    tracing::info!("Received shutdown signal");
                }
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

            // Start metrics server
            let metrics_app = axum::Router::new()
                .route("/metrics", axum::routing::get(|| async {
                    use prometheus::Encoder;
                    let encoder = prometheus::TextEncoder::new();
                    let metric_families = hypercore_engine::METRICS_REGISTRY.gather();
                    let mut buffer = Vec::new();
                    encoder.encode(&metric_families, &mut buffer).unwrap();
                    String::from_utf8(buffer).unwrap_or_default()
                }));
            let metrics_addr: std::net::SocketAddr = "0.0.0.0:9100".parse().unwrap();
            tracing::info!("Metrics server listening on {}", metrics_addr);
            tokio::spawn(async move {
                let listener = tokio::net::TcpListener::bind(metrics_addr).await.unwrap();
                axum::serve(listener, metrics_app).await.ok();
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

            // Start consensus engine based on mode
            let abci_handle = match consensus_mode {
                ConsensusMode::SingleNode => {
                    // Start BlockProducer for single-node consensus
                    let mut block_producer = hypercore_chain::BlockProducer::new(
                        Arc::clone(&app),
                        mempool,
                        hypercore_chain::BlockProducerConfig {
                            block_time_ms,
                            ..Default::default()
                        },
                    );

                    // Add post-commit handler for event broadcasting and optionally persistence
                    #[cfg(feature = "persistence")]
                    let persistence_opt = persistence.clone();

                    // Clone references for the handler
                    let handler_event_tx = event_tx.clone();

                    #[cfg(feature = "persistence")]
                    let state_unified = Arc::clone(&unified_state);
                    #[cfg(feature = "persistence")]
                    let state_engine = Arc::clone(&engine);
                    #[cfg(feature = "persistence")]
                    let state_spot = Arc::clone(&spot_engine);

                    block_producer = block_producer.with_post_commit_handler(Arc::new(
                        move |result: &hypercore_chain::BlockResult, app: &HyperCoreApp| {
                            // 1. Broadcast block events via WebSocket
                            let events = app.get_block_events(result.height);
                            if !events.is_empty() {
                                if let Err(e) = handler_event_tx.try_send(events) {
                                    tracing::warn!("Failed to send events to broadcaster: {}", e);
                                }
                            }

                            // 2. Persist state (if persistence is enabled)
                            #[cfg(feature = "persistence")]
                            if let Some(ref db) = persistence_opt {
                                // Extract state from runtime components
                                let persisted_state = extract_state(
                                    result.height,
                                    result.timestamp,
                                    result.app_hash,
                                    &state_unified,
                                    &state_engine,
                                    None, // perp_engine not available in single-node BlockProducer path
                                    Some(&state_spot),
                                    &app.get_nonces(),
                                    &app.get_cloid_index(),
                                    &app.get_block_metadata(),
                                );

                                // Persist state
                                let persister = StatePersister::new(db.as_ref());
                                match persister.persist_state(&persisted_state) {
                                    Ok(()) => {
                                        tracing::debug!(
                                            "Persisted state at height {} ({} balances)",
                                            result.height,
                                            persisted_state.core.balances.len()
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to persist state: {}", e);
                                    }
                                }
                            }
                        },
                    ));

                    #[cfg(feature = "persistence")]
                    if persistence.is_some() {
                        tracing::info!("Post-commit handler enabled with event broadcasting and persistence");
                    } else {
                        tracing::info!("Post-commit handler enabled with event broadcasting");
                    }
                    #[cfg(not(feature = "persistence"))]
                    tracing::info!("Post-commit handler enabled with event broadcasting");

                    // === P2P Attestation Gossip (optional) ===
                    #[cfg(feature = "p2p")]
                    if enable_p2p_attestation {
                        use hypercore_chain::{
                            AttestationGossip, GossipConfig, AttestationKeyPair,
                            AttestationCollector, AttestationConfig,
                            DivergenceHandler, DivergenceConfig, DivergencePolicy,
                        };

                        tracing::info!("Setting up P2P attestation gossip layer...");
                        tracing::info!("P2P listen address: {}", p2p_listen_addr);

                        // Parse bootstrap peers
                        let bootstrap_peers: Vec<libp2p::Multiaddr> = p2p_bootstrap_peers
                            .as_ref()
                            .map(|s| {
                                s.split(',')
                                    .filter_map(|addr| addr.trim().parse().ok())
                                    .collect()
                            })
                            .unwrap_or_default();

                        if !bootstrap_peers.is_empty() {
                            tracing::info!("Bootstrap peers: {:?}", bootstrap_peers);
                        }

                        // Create attestation key pair (for signing our attestations)
                        let attestation_key = AttestationKeyPair::generate();
                        tracing::info!(
                            "Generated attestation key: {}",
                            attestation_key.public_key_hex()
                        );

                        // Create gossip configuration
                        let gossip_config = GossipConfig {
                            listen_addr: p2p_listen_addr.parse().expect("Invalid P2P listen address"),
                            bootstrap_peers,
                            ..Default::default()
                        };

                        // Create the gossip service
                        match AttestationGossip::new(gossip_config).await {
                            Ok((gossip, broadcast_tx, mut incoming_rx)) => {
                                // Create attestation collector and divergence handler
                                let (alert_tx, alert_rx) = tokio::sync::mpsc::channel(100);
                                let collector = std::sync::Arc::new(AttestationCollector::new(
                                    alert_tx,
                                    AttestationConfig::default(),
                                    Some(attestation_key.public_key()),
                                ));

                                // Register our own validator (for testing - in production, this comes from CometBFT)
                                collector.add_validator(attestation_key.public_key(), 100).await;

                                let handler = DivergenceHandler::new(DivergenceConfig {
                                    policy: DivergencePolicy::Halt,
                                    halt_only_on_minority: true,
                                });

                                // Wire up block producer with attestation
                                block_producer = block_producer
                                    .with_attestation(attestation_key, std::sync::Arc::clone(&collector))
                                    .with_attestation_broadcast(broadcast_tx)
                                    .with_halt_flag(handler.halted_flag());

                                // Run the gossip service in a dedicated thread
                                // (libp2p swarm may not be Send-safe across tokio spawns)
                                std::thread::spawn(move || {
                                    let rt = tokio::runtime::Builder::new_current_thread()
                                        .enable_all()
                                        .build()
                                        .expect("Failed to create gossip runtime");

                                    rt.block_on(async move {
                                        tracing::info!("Starting P2P attestation gossip service...");
                                        if let Err(e) = gossip.run().await {
                                            tracing::error!("Attestation gossip error: {}", e);
                                        }
                                    });
                                });

                                // Spawn task to forward incoming attestations to collector
                                let collector_for_incoming = std::sync::Arc::clone(&collector);
                                tokio::spawn(async move {
                                    while let Some(attestation) = incoming_rx.recv().await {
                                        if let Err(e) = collector_for_incoming.process_attestation(attestation).await {
                                            tracing::debug!("Failed to process incoming attestation: {}", e);
                                        }
                                    }
                                });

                                // Spawn divergence handler
                                tokio::spawn(async move {
                                    handler.run(alert_rx).await;
                                });

                                tracing::info!("P2P attestation gossip enabled");
                            }
                            Err(e) => {
                                tracing::error!("Failed to create P2P attestation gossip: {}", e);
                                tracing::warn!("Continuing without P2P attestation");
                            }
                        }
                    }

                    #[cfg(feature = "p2p")]
                    if !enable_p2p_attestation {
                        tracing::info!("P2P attestation gossip disabled");
                    }

                    let _abci_service = AbciService::new(Arc::clone(&app));
                    tokio::spawn(async move {
                        tracing::info!("BlockProducer started with {}ms block time", block_time_ms);
                        block_producer.start().await;
                    })
                }

                #[cfg(feature = "cometbft")]
                ConsensusMode::CometBft => {
                    // Start CometBFT ABCI server for multi-node consensus.
                    // The ABCI app uses the SAME shared engine/spot_engine/unified_state
                    // as the gateway, so info queries return up-to-date state after
                    // CometBFT's FinalizeBlock executes transactions.
                    use hypercore_chain::{CometBftApp, CometBftServer};

                    let mut cometbft_hypercore_app = HyperCoreApp::with_shared_state(
                        Arc::clone(&unified_state),
                        Arc::clone(&engine),
                        Some(Arc::clone(&spot_engine)),
                    );
                    cometbft_hypercore_app.state.perp_engine = Some(Arc::clone(&perp_engine));

                    // Apply persisted state to the CometBFT app so Info() returns
                    // the correct height and CometBFT only replays missed blocks.
                    #[cfg(feature = "persistence")]
                    if state_restored {
                        if let Some(ref persisted) = restored_chain_data {
                            cometbft_hypercore_app.state.restore_chain_state(
                                restored_height, restored_timestamp, restored_app_hash,
                            );
                            let mut nonces_map = std::collections::HashMap::new();
                            let mut cloid_map = std::collections::HashMap::new();
                            let mut block_hashes_map = std::collections::HashMap::new();
                            restore_chain_state(persisted, &mut nonces_map, &mut cloid_map, &mut block_hashes_map);
                            cometbft_hypercore_app.state.restore_nonces(nonces_map);
                            cometbft_hypercore_app.state.restore_cloid_mappings(cloid_map);
                            cometbft_hypercore_app.state.restore_block_hashes(block_hashes_map);
                            tracing::info!(
                                "CometBFT app state restored: height={}, app_hash={:?}",
                                restored_height, &restored_app_hash[..8]
                            );
                        }
                    }

                    let mut cometbft_app = CometBftApp::new(cometbft_hypercore_app)
                        .with_evm_executor(Arc::clone(&evm));
                    if let Some((receipts, transactions, block_number)) = evm_shared_stores {
                        cometbft_app = cometbft_app.with_evm_receipt_store(receipts, transactions, block_number);
                    }

                    // Post-restore AppHash verification (Phase 4)
                    // After ALL state is restored (unified, engine, perp_engine, EVM, chain meta),
                    // recompute AppHash and compare against the stored value. This catches any
                    // persistence/restoration bugs that would cause consensus divergence.
                    #[cfg(feature = "persistence")]
                    if state_restored {
                        let (matches, recomputed) = cometbft_app.verify_app_hash(&restored_app_hash);
                        if !matches {
                            tracing::error!(
                                "CRITICAL: AppHash mismatch after state restore! stored={} computed={}",
                                hex::encode(&restored_app_hash[..]),
                                hex::encode(&recomputed[..]),
                            );
                            tracing::error!(
                                "This node's state diverges from the network. It may produce different \
                                 blocks and be rejected by peers. Consider state sync or re-genesis."
                            );
                        } else {
                            tracing::info!(
                                "AppHash verified: restored state produces matching hash {}",
                                hex::encode(&restored_app_hash[..8]),
                            );
                        }
                    }

                    // Pass persistence backend so FinalizeBlock can persist state
                    #[cfg(feature = "persistence")]
                    if let Some(ref db) = persistence {
                        cometbft_app = cometbft_app.with_persistence(Arc::clone(db));

                        // Set up snapshot manager for ABCI state sync
                        let snapshot_dir = std::path::PathBuf::from(&data_dir).join("snapshots");
                        let restore_dir = std::path::PathBuf::from(&data_dir).join("snapshot_restore");
                        let snapshot_interval = 1000; // create snapshot every 1000 blocks
                        match hypercore_persistence::SnapshotManager::with_arc_db(
                            Arc::clone(db),
                            &snapshot_dir,
                            snapshot_interval,
                        ) {
                            Ok(manager) => {
                                let existing = manager.list_snapshots().len();
                                tracing::info!(
                                    "Snapshot manager initialized: dir={}, interval={}, existing={}",
                                    snapshot_dir.display(), snapshot_interval, existing
                                );
                                cometbft_app = cometbft_app.with_snapshot_manager(manager, restore_dir);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to create snapshot manager: {}", e);
                                tracing::warn!("State sync will not be available");
                            }
                        }
                    }

                    let server = CometBftServer::new(cometbft_app);

                    tokio::task::spawn_blocking(move || {
                        tracing::info!("Starting CometBFT ABCI server on {}", abci_addr);
                        if let Err(e) = server.start(abci_addr) {
                            tracing::error!("CometBFT ABCI server error: {}", e);
                        }
                    })
                }
            };

            // Start indexer if enabled
            #[cfg(feature = "indexer")]
            let _indexer_handle = if enable_indexer {
                if let Some(db_url) = database_url {
                    tracing::info!("Starting indexer with database: {}", db_url);

                    // Connect to database and start indexer
                    let indexer_engine = Arc::clone(&engine);
                    let db_url_clone = db_url.clone();

                    Some(tokio::spawn(async move {
                        match hypercore_indexer::Database::connect(&db_url_clone).await {
                            Ok(db) => {
                                // Run migrations
                                if let Err(e) = db.run_migrations().await {
                                    tracing::error!("Failed to run indexer migrations: {}", e);
                                    return;
                                }

                                // Create and run indexer
                                match hypercore_indexer::Indexer::new(db, indexer_engine).await {
                                    Ok(mut indexer) => {
                                        tracing::info!("Indexer started successfully");
                                        if let Err(e) = indexer.run().await {
                                            tracing::error!("Indexer error: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to create indexer: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to connect to indexer database: {}", e);
                            }
                        }
                    }))
                } else {
                    tracing::warn!("Indexer enabled but no DATABASE_URL provided");
                    None
                }
            } else {
                None
            };

            #[cfg(not(feature = "indexer"))]
            if enable_indexer {
                tracing::warn!("Indexer requested but 'indexer' feature is not enabled. Recompile with --features indexer");
            }

            // Start price feed updater (mock for devnet)
            // In CometBFT mode, skip mock price feed - it causes lock contention
            // with the ABCI app and price updates should come through consensus.
            #[cfg(feature = "cometbft")]
            let _is_cometbft = matches!(consensus_mode, ConsensusMode::CometBft);
            #[cfg(not(feature = "cometbft"))]
            let _is_cometbft = false;

            if !_is_cometbft {
                let price_engine = Arc::clone(&engine);
                let _price_handle = tokio::spawn(async move {
                    mock_price_feed(price_engine).await;
                });

                // Start funding rate processor
                let funding_engine = Arc::clone(&engine);
                let _funding_handle = tokio::spawn(async move {
                    funding_processor(funding_engine).await;
                });
            } else {
                tracing::info!("Skipping mock price feed and funding processor in CometBFT mode");
            }

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

            // Graceful shutdown: flush and close persistence
            #[cfg(feature = "persistence")]
            if let Some(db) = persistence {
                tracing::info!("Flushing persistence to disk...");
                if let Err(e) = db.flush() {
                    tracing::error!("Failed to flush persistence: {}", e);
                }
                if let Err(e) = db.close() {
                    tracing::error!("Failed to close persistence: {}", e);
                }
                tracing::info!("Persistence shutdown complete");
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

        Commands::Export {
            output,
            height: _height,
            #[cfg(feature = "persistence")]
            data_dir,
        } => {
            init_logging("info");
            tracing::info!("Exporting state to {}", output);

            #[cfg(feature = "persistence")]
            {
                // Open RocksDB backend
                let config = PersistenceConfig {
                    data_dir: data_dir.clone(),
                    create_if_missing: false,
                    ..Default::default()
                };

                let backend = RocksDbBackend::open(&config)
                    .map_err(|e| anyhow::anyhow!("Failed to open persistence backend: {}. Make sure the data directory exists and was previously initialized.", e))?;

                let persister = StatePersister::new(&backend);

                // Load state from persistence
                let state = persister.load_state()
                    .map_err(|e| anyhow::anyhow!("Failed to load state: {}", e))?
                    .ok_or_else(|| anyhow::anyhow!("No persisted state found. Export requires existing state."))?;

                tracing::info!("Loaded state at height {}", state.height);

                // Serialize to JSON
                let json = serde_json::to_string_pretty(&state)
                    .map_err(|e| anyhow::anyhow!("Failed to serialize state: {}", e))?;

                // Write to output file
                std::fs::write(&output, json)
                    .map_err(|e| anyhow::anyhow!("Failed to write export file: {}", e))?;

                tracing::info!(
                    "Exported state at height {} to {} ({} bytes)",
                    state.height,
                    output,
                    std::fs::metadata(&output)?.len()
                );
                tracing::info!(
                    "State contains: {} balances, {} positions, {} orders",
                    state.core.balances.len(),
                    state.core.positions.len(),
                    state.core.orders.len() + state.spot.orders.len()
                );
            }

            #[cfg(not(feature = "persistence"))]
            {
                #[allow(unreachable_code)]
                return Err(anyhow::anyhow!(
                    "Export requires the 'persistence' feature. Build with: cargo build --features persistence"
                ));
            }

            #[allow(unreachable_code)]
            Ok(())
        }

        Commands::Import {
            input,
            #[cfg(feature = "persistence")]
            data_dir,
        } => {
            init_logging("info");
            tracing::info!("Importing state from {}", input);

            #[cfg(feature = "persistence")]
            {
                // Read JSON from input file
                let json = std::fs::read_to_string(&input)
                    .map_err(|e| anyhow::anyhow!("Failed to read import file: {}", e))?;

                // Deserialize to PersistedState
                let state: hypercore_persistence::PersistedState = serde_json::from_str(&json)
                    .map_err(|e| anyhow::anyhow!("Failed to parse import file: {}", e))?;

                tracing::info!("Parsed state at height {}", state.height);

                // Validate the state
                state.validate()
                    .map_err(|e| anyhow::anyhow!("State validation failed: {}", e))?;

                tracing::info!("State validated successfully");

                // Open RocksDB backend (create if missing for import)
                let config = PersistenceConfig {
                    data_dir: data_dir.clone(),
                    create_if_missing: true,
                    ..Default::default()
                };

                let backend = RocksDbBackend::open(&config)
                    .map_err(|e| anyhow::anyhow!("Failed to open persistence backend: {}", e))?;

                let persister = StatePersister::new(&backend);

                // Check if there's existing state
                if persister.has_state().unwrap_or(false) {
                    let existing_height = persister.get_height().unwrap_or(0);
                    tracing::warn!(
                        "Existing state at height {} will be overwritten with imported state at height {}",
                        existing_height,
                        state.height
                    );
                }

                // Persist the imported state
                persister.persist_state(&state)
                    .map_err(|e| anyhow::anyhow!("Failed to persist imported state: {}", e))?;

                // Flush to ensure data is written
                persister.flush()
                    .map_err(|e| anyhow::anyhow!("Failed to flush persistence: {}", e))?;

                tracing::info!(
                    "Imported state at height {} from {} to {}",
                    state.height,
                    input,
                    data_dir
                );
                tracing::info!(
                    "State contains: {} balances, {} positions, {} orders",
                    state.core.balances.len(),
                    state.core.positions.len(),
                    state.core.orders.len() + state.spot.orders.len()
                );
            }

            #[cfg(not(feature = "persistence"))]
            {
                #[allow(unreachable_code)]
                return Err(anyhow::anyhow!(
                    "Import requires the 'persistence' feature. Build with: cargo build --features persistence"
                ));
            }

            #[allow(unreachable_code)]
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
///
/// This function deploys the TEST token and creates the TEST-USDC market.
/// Initial balances are NOT credited here - they come from genesis state.
///
/// For genesis-based initialization, balances are set in `init_from_genesis()`.
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

    tracing::info!("Initialized spot engine (balances from genesis)");
    Ok(())
}

/// Initialize balances from genesis state for single-node mode
///
/// This function parses the genesis JSON and credits initial balances.
/// In CometBFT mode, this is handled by `HyperCoreApp::init_from_genesis()`.
fn initialize_genesis_balances(
    unified_state: &hypercore_primitives::SharedUnifiedState,
    genesis_json: &serde_json::Value,
) -> anyhow::Result<()> {
    use hypercore_primitives::Decimal;

    let app_state = genesis_json.get("app_state")
        .ok_or_else(|| anyhow::anyhow!("Missing app_state in genesis"))?;

    let empty_vec = vec![];
    let balances = app_state.get("balances")
        .and_then(|b| b.as_array())
        .unwrap_or(&empty_vec);

    if balances.is_empty() {
        tracing::warn!("No balances in genesis state");
        return Ok(());
    }

    let mut unified = unified_state.write().unwrap();

    for balance_entry in balances {
        // Parse address
        let address_str = balance_entry.get("address")
            .and_then(|a| a.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing address in balance entry"))?;

        let address = parse_address(address_str)?;

        // Parse token index
        let token = balance_entry.get("token")
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u8;

        // Parse amount
        let amount_str = balance_entry.get("amount")
            .and_then(|a| a.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing amount in balance entry"))?;

        let decimals = if token == 0 { 6 } else { 18 };
        // IMPORTANT: Use from_str_exact with correct decimals, NOT from_str (which uses 8 decimals)
        let amount = Decimal::from_str_exact(amount_str, decimals)
            .unwrap_or_else(|| Decimal::from_raw(0, decimals));

        if amount.raw() == 0 {
            continue;
        }

        // Parse view (default to "core")
        let view = balance_entry.get("view")
            .and_then(|v| v.as_str())
            .unwrap_or("core");

        match view {
            "core" => {
                unified.credit(address, token, amount);
                tracing::info!(
                    "Genesis: Credited {:?} token {} amount {} to core_view",
                    address, token, amount.to_string_trimmed()
                );
            }
            "evm" => {
                unified.credit_evm(address, token, amount);
                tracing::info!(
                    "Genesis: Credited {:?} token {} amount {} to evm_view",
                    address, token, amount.to_string_trimmed()
                );
            }
            _ => {
                tracing::warn!("Unknown view '{}', defaulting to core", view);
                unified.credit(address, token, amount);
            }
        }
    }

    tracing::info!("Initialized {} balance entries from genesis", balances.len());
    Ok(())
}

/// Parse hex address string to AccountAddress
fn parse_address(hex_str: &str) -> anyhow::Result<hypercore_primitives::AccountAddress> {
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if hex_str.len() != 40 {
        return Err(anyhow::anyhow!("Invalid address length: {}", hex_str.len()));
    }

    let bytes = hex::decode(hex_str)?;
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes);
    Ok(hypercore_primitives::AccountAddress::from(addr))
}

/// Create genesis state with proper initial balances
///
/// This generates a genesis configuration that includes:
/// - Perpetual markets (BTC-PERP, ETH-PERP)
/// - Spot tokens (TEST token)
/// - Initial balances for test accounts (Alice, Bob, Charlie)
///
/// All validators must use identical genesis to reach consensus.
fn create_genesis(chain_id: u64) -> anyhow::Result<serde_json::Value> {
    // Test accounts (standard Anvil/Hardhat accounts)
    let alice = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let bob = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    let charlie = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";
    // User's MetaMask wallet
    let user = "0xdeA3c06EEe614bF84e74d505173822236c8Ad135";

    Ok(serde_json::json!({
        "chain_id": format!("hypercore-{}", chain_id),
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
            "chain_id": format!("hypercore-{}", chain_id),
            "markets": [
                {
                    "id": 0,
                    "symbol": "BTC-PERP",
                    "max_leverage": 50,
                    "initial_mark_price": "65000"
                },
                {
                    "id": 1,
                    "symbol": "ETH-PERP",
                    "max_leverage": 50,
                    "initial_mark_price": "3500"
                }
            ],
            "spot_tokens": [
                {
                    "index": 1,
                    "name": "Bitcoin",
                    "symbol": "BTC",
                    "wei_decimals": 8,
                    "sz_decimals": 3,
                    "max_supply": "21000000"
                },
                {
                    "index": 2,
                    "name": "Ethereum",
                    "symbol": "ETH",
                    "wei_decimals": 18,
                    "sz_decimals": 4,
                    "max_supply": "120000000"
                }
            ],
            "balances": [
                // Alice: USDC + BTC + ETH
                { "address": alice, "token": 0, "amount": "100000", "view": "core" },
                { "address": alice, "token": 1, "amount": "10", "view": "core" },
                { "address": alice, "token": 2, "amount": "100", "view": "core" },
                // Bob: USDC + BTC + ETH
                { "address": bob, "token": 0, "amount": "100000", "view": "core" },
                { "address": bob, "token": 1, "amount": "10", "view": "core" },
                { "address": bob, "token": 2, "amount": "100", "view": "core" },
                // Charlie: USDC + BTC + ETH
                { "address": charlie, "token": 0, "amount": "100000", "view": "core" },
                { "address": charlie, "token": 1, "amount": "10", "view": "core" },
                { "address": charlie, "token": 2, "amount": "100", "view": "core" },
                // Admin wallet: 10M USDC + 100 BTC + 1000 ETH (core) + 100K USDC (evm)
                { "address": user, "token": 0, "amount": "10000000", "view": "core" },
                { "address": user, "token": 0, "amount": "100000", "view": "evm" },
                { "address": user, "token": 1, "amount": "100", "view": "core" },
                { "address": user, "token": 2, "amount": "1000", "view": "core" }
            ]
        }
    }))
}

/// Mock price feed for devnet (simulates price movements)
async fn mock_price_feed(engine: Arc<RwLock<EngineState>>) {
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
async fn funding_processor(engine: Arc<RwLock<EngineState>>) {
    use hypercore_engine::FundingEngine;
    use hypercore_primitives::FundingPayment;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Check funding every minute
    let interval = tokio::time::Duration::from_secs(60);
    let funding_engine = FundingEngine::default();

    loop {
        tokio::time::sleep(interval).await;

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Get all markets that need funding settlement
        let mut engine_guard = engine.write().await;
        let market_ids = engine_guard.get_market_ids();

        for market_id in market_ids {
            if let Some(market) = engine_guard.get_market_mut(market_id) {
                // Check if funding should be settled
                if !funding_engine.should_settle(&market.state, current_time) {
                    continue;
                }

                // Calculate funding rate (using mark price as index for now)
                let index_price = market.state.mark_price;
                let funding_rate = funding_engine.calculate_funding_rate(&market.state, index_price);

                tracing::debug!(
                    "Settling funding for market {}: rate={}, mark={}",
                    market.config.symbol,
                    funding_rate.to_string_trimmed(),
                    market.state.mark_price.to_string_trimmed()
                );

                // Apply funding to the market
                funding_engine.settle_funding(market, funding_rate, current_time);

                // Record market funding history
                engine_guard.record_market_funding(
                    market_id,
                    current_time,
                    funding_rate.raw(),
                );
            }
        }

        // Apply funding payments to positions
        // For each account with positions, calculate and apply funding payment
        let accounts: Vec<_> = engine_guard.accounts.keys().cloned().collect();
        for account in accounts {
            for market_id in engine_guard.get_market_ids() {
                if let Some(position) = engine_guard.get_position(account, market_id) {
                    if position.size.is_zero() {
                        continue;
                    }

                    if let Some(market) = engine_guard.get_market(market_id) {
                        // Calculate funding payment
                        let payment = funding_engine.calculate_funding_payment(
                            position.size,
                            position.last_funding_index,
                            market.state.funding_accumulator,
                        );

                        if !payment.is_zero() {
                            // Record funding payment
                            let funding_payment = FundingPayment {
                                market_id,
                                account,
                                payment: payment.raw(),
                                position_size: position.size.raw(),
                                funding_rate: market.state.funding_rate.raw(),
                                timestamp: current_time,
                            };
                            engine_guard.record_funding_payment(funding_payment);

                            tracing::debug!(
                                "Applied funding {} to {} for market {}",
                                payment.to_string_trimmed(),
                                account,
                                market_id
                            );
                        }
                    }
                }

                // Update position's last funding index
                let funding_accumulator = engine_guard
                    .get_market(market_id)
                    .map(|m| m.state.funding_accumulator);
                if let (Some(position), Some(accumulator)) = (
                    engine_guard.get_position_mut(account, market_id),
                    funding_accumulator,
                ) {
                    position.last_funding_index = accumulator;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_genesis() {
        let genesis = create_genesis(1337).unwrap();
        assert_eq!(genesis["chain_id"], "hypercore-1337");
        assert!(genesis["app_state"]["markets"].as_array().unwrap().len() == 2);
    }
}
