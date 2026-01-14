//! Database connection and pool management

use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

/// Database connection wrapper
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create new database connection
    pub async fn connect(url: &str) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(|e| DatabaseError::ConnectionError(e.to_string()))?;

        Ok(Self { pool })
    }

    /// Get the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Run migrations
    pub async fn run_migrations(&self) -> Result<(), DatabaseError> {
        sqlx::migrate!("../../indexer-db/migrations")
            .run(&self.pool)
            .await
            .map_err(|e| DatabaseError::MigrationError(e.to_string()))?;

        tracing::info!("Database migrations complete");
        Ok(())
    }

    /// Check if database is healthy
    pub async fn health_check(&self) -> Result<(), DatabaseError> {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        Ok(())
    }

    /// Insert block
    pub async fn insert_block(
        &self,
        height: i64,
        hash: &[u8],
        timestamp: i64,
        proposer: Option<&[u8]>,
        tx_count: i32,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO blocks (height, hash, timestamp, proposer, tx_count)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (height) DO NOTHING
            "#,
        )
        .bind(height)
        .bind(hash)
        .bind(timestamp)
        .bind(proposer)
        .bind(tx_count)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Insert transaction
    pub async fn insert_transaction(
        &self,
        hash: &[u8],
        block_height: i64,
        tx_index: i32,
        sender: &[u8],
        tx_type: &str,
        data: &serde_json::Value,
        success: bool,
        error: Option<&str>,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO transactions (hash, block_height, tx_index, sender, tx_type, data, success, error)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (hash) DO NOTHING
            "#,
        )
        .bind(hash)
        .bind(block_height)
        .bind(tx_index)
        .bind(sender)
        .bind(tx_type)
        .bind(data)
        .bind(success)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Insert or update account
    pub async fn upsert_account(
        &self,
        address: &[u8],
        balance: &str,
        equity: &str,
        margin_used: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO accounts (address, balance, equity, margin_used, updated_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (address) DO UPDATE SET
                balance = EXCLUDED.balance,
                equity = EXCLUDED.equity,
                margin_used = EXCLUDED.margin_used,
                updated_at = NOW()
            "#,
        )
        .bind(address)
        .bind(balance)
        .bind(equity)
        .bind(margin_used)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Insert or update position
    pub async fn upsert_position(
        &self,
        account: &[u8],
        market_id: i16,
        size: &str,
        entry_price: &str,
        unrealized_pnl: &str,
        leverage: i16,
        liquidation_price: Option<&str>,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO positions (account, market_id, size, entry_price, unrealized_pnl, leverage, liquidation_price, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
            ON CONFLICT (account, market_id) DO UPDATE SET
                size = EXCLUDED.size,
                entry_price = EXCLUDED.entry_price,
                unrealized_pnl = EXCLUDED.unrealized_pnl,
                leverage = EXCLUDED.leverage,
                liquidation_price = EXCLUDED.liquidation_price,
                updated_at = NOW()
            "#,
        )
        .bind(account)
        .bind(market_id)
        .bind(size)
        .bind(entry_price)
        .bind(unrealized_pnl)
        .bind(leverage)
        .bind(liquidation_price)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Insert order
    pub async fn insert_order(
        &self,
        id: i64,
        account: &[u8],
        market_id: i16,
        side: &str,
        price: &str,
        size: &str,
        filled_size: &str,
        status: &str,
        order_type: &str,
        created_at: i64,
        cloid: Option<&str>,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO orders (id, account, market_id, side, price, size, filled_size, status, order_type, created_at, cloid)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, to_timestamp($10), $11)
            ON CONFLICT (id) DO UPDATE SET
                filled_size = EXCLUDED.filled_size,
                status = EXCLUDED.status
            "#,
        )
        .bind(id)
        .bind(account)
        .bind(market_id)
        .bind(side)
        .bind(price)
        .bind(size)
        .bind(filled_size)
        .bind(status)
        .bind(order_type)
        .bind(created_at as f64 / 1000.0)
        .bind(cloid)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Insert fill
    pub async fn insert_fill(
        &self,
        trade_id: i64,
        order_id: i64,
        account: &[u8],
        market_id: i16,
        side: &str,
        price: &str,
        size: &str,
        fee: &str,
        is_taker: bool,
        realized_pnl: &str,
        timestamp: i64,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO fills (trade_id, order_id, account, market_id, side, price, size, fee, is_taker, realized_pnl, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, to_timestamp($11))
            ON CONFLICT (trade_id, account) DO NOTHING
            "#,
        )
        .bind(trade_id)
        .bind(order_id)
        .bind(account)
        .bind(market_id)
        .bind(side)
        .bind(price)
        .bind(size)
        .bind(fee)
        .bind(is_taker)
        .bind(realized_pnl)
        .bind(timestamp as f64 / 1000.0)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Insert funding rate
    pub async fn insert_funding_rate(
        &self,
        market_id: i16,
        rate: &str,
        premium: &str,
        mark_price: &str,
        index_price: &str,
        timestamp: i64,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            INSERT INTO funding_rates (market_id, rate, premium, mark_price, index_price, timestamp)
            VALUES ($1, $2, $3, $4, $5, to_timestamp($6))
            "#,
        )
        .bind(market_id)
        .bind(rate)
        .bind(premium)
        .bind(mark_price)
        .bind(index_price)
        .bind(timestamp as f64 / 1000.0)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    /// Get latest block height
    pub async fn get_latest_height(&self) -> Result<Option<i64>, DatabaseError> {
        // MAX returns NULL when table is empty, so we need to handle that
        let row: Option<(Option<i64>,)> = sqlx::query_as("SELECT MAX(height) FROM blocks")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(row.and_then(|(h,)| h))
    }
}

/// Database errors
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Migration error: {0}")]
    MigrationError(String),
    #[error("Query error: {0}")]
    QueryError(String),
}
