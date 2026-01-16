//! Block ingestion and event processing

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use hypercore_engine::EngineState;
use hypercore_primitives::BlockEvent;
use tokio::sync::RwLock;

use crate::candles::CandleAggregator;
use crate::db::Database;

/// Block indexer
pub struct Indexer {
    db: Database,
    engine: Arc<RwLock<EngineState>>,
    candle_aggregator: CandleAggregator,
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

        // Create candle aggregator
        let candle_aggregator = CandleAggregator::new(db.pool().clone());

        Ok(Self {
            db,
            engine,
            candle_aggregator,
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

        // Get events for this block
        let json_events = engine.get_block_events(height as u64);
        drop(engine);

        // Process each event
        let mut event_count = 0;
        for json_event in json_events {
            // Try to deserialize as BlockEvent
            match serde_json::from_value::<BlockEvent>(json_event.clone()) {
                Ok(event) => {
                    if let Err(e) = self.process_event(&event).await {
                        tracing::warn!("Failed to process event: {:?}", e);
                    }
                    event_count += 1;
                }
                Err(e) => {
                    // Log but continue - some events may have different formats
                    tracing::debug!(
                        "Could not parse event as BlockEvent: {} - raw: {}",
                        e,
                        serde_json::to_string(&json_event).unwrap_or_default()
                    );
                }
            }
        }

        self.current_height = height;
        if event_count > 0 {
            tracing::debug!("Indexed block {} with {} events", height, event_count);
        } else {
            tracing::debug!("Indexed block {}", height);
        }

        Ok(())
    }

    /// Process an event
    async fn process_event(&self, event: &BlockEvent) -> Result<(), IndexerError> {
        match event {
            BlockEvent::Fill(fill) => {
                // Insert fill into database
                self.db.insert_fill(
                    fill.trade_id as i64,
                    fill.order_id as i64,
                    fill.account.as_slice(),
                    fill.market_id as i16,
                    &format!("{:?}", fill.side),
                    &fill.price.to_string_trimmed(),
                    &fill.size.to_string_trimmed(),
                    &fill.fee.to_string_trimmed(),
                    fill.is_taker,
                    &fill.realized_pnl.to_string(),
                    fill.timestamp as i64,
                ).await?;

                // Update candles (only for taker fills to avoid double-counting)
                if fill.is_taker {
                    let timestamp = Utc.timestamp_millis_opt(fill.timestamp as i64)
                        .single()
                        .unwrap_or_else(Utc::now);
                    if let Err(e) = self.candle_aggregator.update_from_fill(
                        fill.market_id as i16,
                        &fill.price.to_string_trimmed(),
                        &fill.size.to_string_trimmed(),
                        timestamp,
                    ).await {
                        tracing::warn!("Failed to update candles: {}", e);
                    }
                }
            }
            BlockEvent::OrderPlaced(order) => {
                self.db.insert_order(
                    order.order_id as i64,
                    order.account.as_slice(),
                    order.market_id as i16,
                    &format!("{:?}", order.side),
                    &order.price.to_string_trimmed(),
                    &order.size.to_string_trimmed(),
                    "0",
                    "open",
                    if order.reduce_only { "reduce_only" } else { "limit" },
                    order.timestamp as i64,
                    order.client_order_id.as_deref(),
                ).await?;
            }
            BlockEvent::OrderCanceled(cancel) => {
                // Update order status
                sqlx::query("UPDATE orders SET status = 'canceled' WHERE id = $1")
                    .bind(cancel.order_id as i64)
                    .execute(self.db.pool())
                    .await
                    .map_err(|e| IndexerError::DatabaseError(e.to_string()))?;
            }
            BlockEvent::PositionUpdated(pos) => {
                self.db.upsert_position(
                    pos.account.as_slice(),
                    pos.market_id as i16,
                    &pos.size.to_string(),
                    &pos.entry_notional.to_string_trimmed(),
                    "0", // unrealized PnL calculated on query
                    pos.leverage as i16,
                    None, // liquidation price calculated on query
                ).await?;
            }
            BlockEvent::FundingApplied(funding) => {
                self.db.insert_funding_rate(
                    funding.market_id as i16,
                    &funding.funding_rate.to_string(),
                    "0", // premium index not stored in event
                    &funding.mark_price.to_string_trimmed(),
                    &funding.index_price.to_string_trimmed(),
                    funding.timestamp as i64,
                ).await?;
            }
            BlockEvent::Liquidation(liq) => {
                // Insert liquidation event
                sqlx::query(
                    r#"
                    INSERT INTO liquidations (account, market_id, size, price, pnl, timestamp)
                    VALUES ($1, $2, $3, $4, $5, to_timestamp($6))
                    "#,
                )
                .bind(liq.account.as_slice())
                .bind(liq.market_id as i16)
                .bind(&liq.size.to_string())
                .bind(&liq.price.to_string_trimmed())
                .bind(&liq.pnl.to_string())
                .bind(liq.timestamp as f64 / 1000.0)
                .execute(self.db.pool())
                .await
                .map_err(|e| IndexerError::DatabaseError(e.to_string()))?;
            }
            BlockEvent::UsdTransfer(transfer) => {
                // Log transfer but don't need to store in separate table
                // Balances are tracked in accounts table
                tracing::debug!(
                    "USD transfer: {:?} -> {:?}: {}",
                    transfer.from, transfer.to, transfer.amount.to_string_trimmed()
                );
            }
            BlockEvent::LeverageUpdated(update) => {
                // Log leverage update
                tracing::debug!(
                    "Leverage update: {:?} market {} -> {}x",
                    update.account, update.market_id, update.leverage
                );
            }
        }

        Ok(())
    }

    /// Get current indexed height
    pub fn current_height(&self) -> i64 {
        self.current_height
    }
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
