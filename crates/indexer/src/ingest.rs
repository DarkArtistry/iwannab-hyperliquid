//! Block ingestion and event processing

use std::sync::Arc;

use hypercore_engine::EngineState;
use tokio::sync::RwLock;

use crate::db::Database;

/// Block indexer
pub struct Indexer {
    db: Database,
    engine: Arc<RwLock<EngineState>>,
    current_height: i64,
}

impl Indexer {
    /// Create new indexer
    pub async fn new(
        db: Database,
        engine: Arc<RwLock<EngineState>>,
    ) -> Result<Self, IndexerError> {
        // Get latest indexed height
        let current_height = db.get_latest_height().await?.unwrap_or(0);

        Ok(Self {
            db,
            engine,
            current_height,
        })
    }

    /// Run the indexer
    pub async fn run(&mut self) -> Result<(), IndexerError> {
        tracing::info!("Starting indexer from height {}", self.current_height);

        loop {
            // Check for new blocks
            let engine = self.engine.read().await;
            let latest_height = engine.current_height() as i64;
            drop(engine);

            if latest_height > self.current_height {
                // Index new blocks
                for height in (self.current_height + 1)..=latest_height {
                    self.index_block(height).await?;
                }
            }

            // Wait before checking again
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Index a single block
    async fn index_block(&mut self, height: i64) -> Result<(), IndexerError> {
        let engine = self.engine.read().await;

        // Get block data from engine
        let block_hash = engine.get_block_hash(height as u64)
            .unwrap_or([0u8; 32]);
        let timestamp = engine.get_block_timestamp(height as u64).unwrap_or(0);
        let tx_count = engine.get_block_tx_count(height as u64).unwrap_or(0) as i32;

        // Insert block
        self.db.insert_block(
            height,
            &block_hash,
            timestamp as i64,
            None,
            tx_count,
        ).await?;

        // Index transactions and events for this block
        // TODO: Convert JSON events to BlockEvent structs
        let _events = engine.get_block_events(height as u64);
        drop(engine);

        self.current_height = height;
        tracing::debug!("Indexed block {}", height);

        Ok(())
    }

    /// Process an event
    async fn process_event(&self, event: &BlockEvent) -> Result<(), IndexerError> {
        match event {
            BlockEvent::Fill(fill) => {
                self.db.insert_fill(
                    fill.trade_id as i64,
                    fill.order_id as i64,
                    fill.account.as_slice(),
                    fill.market_id as i16,
                    &fill.side.to_string(),
                    &fill.price.to_string(),
                    &fill.size.to_string(),
                    &fill.fee.to_string(),
                    fill.is_taker,
                    &fill.realized_pnl.to_string(),
                    fill.timestamp as i64,
                ).await?;
            }
            BlockEvent::OrderPlaced(order) => {
                self.db.insert_order(
                    order.id as i64,
                    order.owner.as_slice(),
                    order.market_id as i16,
                    &order.side.to_string(),
                    &order.price.to_string(),
                    &order.size.to_string(),
                    "0",
                    "open",
                    &order.order_type.to_string(),
                    order.timestamp as i64,
                    order.client_order_id.as_deref(),
                ).await?;
            }
            BlockEvent::OrderCanceled { market_id, order_id } => {
                // Update order status
                sqlx::query("UPDATE orders SET status = 'canceled' WHERE id = $1")
                    .bind(*order_id as i64)
                    .execute(self.db.pool())
                    .await
                    .map_err(|e| IndexerError::DatabaseError(e.to_string()))?;
            }
            BlockEvent::PositionUpdated(pos) => {
                self.db.upsert_position(
                    pos.account.as_slice(),
                    pos.market_id as i16,
                    &pos.size.to_string(),
                    &pos.entry_price.to_string(),
                    &pos.unrealized_pnl.to_string(),
                    pos.leverage as i16,
                    pos.liquidation_price.as_ref().map(|p| p.to_string()).as_deref(),
                ).await?;
            }
            BlockEvent::FundingApplied { market_id, rate, timestamp } => {
                let engine = self.engine.read().await;
                let state = engine.get_market_state(*market_id).cloned();
                drop(engine);

                if let Some(s) = state {
                    self.db.insert_funding_rate(
                        *market_id as i16,
                        &rate.to_string(),
                        &s.premium.to_string(),
                        &s.mark_price.to_string(),
                        &s.index_price.to_string(),
                        *timestamp as i64,
                    ).await?;
                }
            }
            BlockEvent::Liquidation(liq) => {
                // Insert liquidation event
                sqlx::query(
                    r#"
                    INSERT INTO liquidations (account, market_id, size, price, timestamp)
                    VALUES ($1, $2, $3, $4, to_timestamp($5))
                    "#,
                )
                .bind(liq.account.as_slice())
                .bind(liq.market_id as i16)
                .bind(&liq.size.to_string())
                .bind(&liq.price.to_string())
                .bind(liq.timestamp as f64 / 1000.0)
                .execute(self.db.pool())
                .await
                .map_err(|e| IndexerError::DatabaseError(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Get current indexed height
    pub fn current_height(&self) -> i64 {
        self.current_height
    }
}

/// Block events from engine
#[derive(Debug, Clone)]
pub enum BlockEvent {
    Fill(FillEvent),
    OrderPlaced(OrderEvent),
    OrderCanceled { market_id: u8, order_id: u64 },
    PositionUpdated(PositionEvent),
    FundingApplied { market_id: u8, rate: hypercore_primitives::Decimal, timestamp: u64 },
    Liquidation(LiquidationEvent),
}

#[derive(Debug, Clone)]
pub struct FillEvent {
    pub trade_id: u64,
    pub order_id: u64,
    pub account: hypercore_primitives::AccountAddress,
    pub market_id: u8,
    pub side: hypercore_primitives::OrderSide,
    pub price: hypercore_primitives::Decimal,
    pub size: hypercore_primitives::Decimal,
    pub fee: hypercore_primitives::Decimal,
    pub is_taker: bool,
    pub realized_pnl: hypercore_primitives::Decimal,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct OrderEvent {
    pub id: u64,
    pub owner: hypercore_primitives::AccountAddress,
    pub market_id: u8,
    pub side: hypercore_primitives::OrderSide,
    pub price: hypercore_primitives::Decimal,
    pub size: hypercore_primitives::Decimal,
    pub order_type: hypercore_primitives::OrderType,
    pub timestamp: u64,
    pub client_order_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PositionEvent {
    pub account: hypercore_primitives::AccountAddress,
    pub market_id: u8,
    pub size: hypercore_primitives::Decimal,
    pub entry_price: hypercore_primitives::Decimal,
    pub unrealized_pnl: hypercore_primitives::Decimal,
    pub leverage: u8,
    pub liquidation_price: Option<hypercore_primitives::Decimal>,
}

#[derive(Debug, Clone)]
pub struct LiquidationEvent {
    pub account: hypercore_primitives::AccountAddress,
    pub market_id: u8,
    pub size: hypercore_primitives::Decimal,
    pub price: hypercore_primitives::Decimal,
    pub timestamp: u64,
}

/// Indexer errors
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Engine error: {0}")]
    EngineError(String),
}

impl From<crate::db::DatabaseError> for IndexerError {
    fn from(e: crate::db::DatabaseError) -> Self {
        Self::DatabaseError(e.to_string())
    }
}
