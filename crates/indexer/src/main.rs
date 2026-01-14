//! HyperCore Indexer - Standalone blockchain indexer binary
//!
//! This binary runs the indexer service that:
//! - Connects to the blockchain node
//! - Indexes blocks, transactions, and events
//! - Stores data in PostgreSQL for efficient querying

use std::sync::Arc;

use clap::Parser;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use hypercore_engine::EngineState;
use hypercore_indexer::{Database, Indexer};
use hypercore_primitives::{Decimal, Market, MarketConfig};

/// HyperCore Indexer CLI
#[derive(Parser)]
#[command(name = "hypercore-indexer")]
#[command(about = "HyperCore Indexer - Blockchain data indexing service")]
#[command(version)]
struct Cli {
    /// PostgreSQL database URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Run database migrations
    #[arg(long, default_value = "true")]
    run_migrations: bool,

    /// Log level
    #[arg(long, default_value = "info", env = "RUST_LOG")]
    log_level: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(&cli.log_level);

    tracing::info!("Starting HyperCore Indexer");

    // Connect to database
    tracing::info!("Connecting to database...");
    let db = Database::connect(&cli.database_url).await?;
    tracing::info!("Database connected");

    // Run migrations if requested
    if cli.run_migrations {
        tracing::info!("Running database migrations...");
        db.run_migrations().await?;
    }

    // Create engine state
    // In production, this would connect to the node RPC
    let engine = Arc::new(RwLock::new(EngineState::new()));

    // Initialize default markets
    initialize_markets(&engine).await;

    // Create and run indexer
    let mut indexer = Indexer::new(db, engine).await?;

    // Handle shutdown signal
    let shutdown = tokio::signal::ctrl_c();

    tokio::select! {
        result = indexer.run() => {
            if let Err(e) = result {
                tracing::error!("Indexer error: {}", e);
                return Err(e.into());
            }
        }
        _ = shutdown => {
            tracing::info!("Shutdown signal received");
        }
    }

    tracing::info!("Indexer shutdown complete");
    Ok(())
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

    tracing::info!("Initialized markets: BTC-PERP, ETH-PERP");
}
