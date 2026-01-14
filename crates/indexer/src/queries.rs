//! Database query helpers

use sqlx::PgPool;

use crate::models::*;

/// Query interface for indexed data
pub struct Queries<'a> {
    pool: &'a PgPool,
}

impl<'a> Queries<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Get account by address
    pub async fn get_account(&self, address: &[u8]) -> Result<Option<Account>, sqlx::Error> {
        sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE address = $1")
            .bind(address)
            .fetch_optional(self.pool)
            .await
    }

    /// Get positions for account
    pub async fn get_positions(&self, address: &[u8]) -> Result<Vec<Position>, sqlx::Error> {
        sqlx::query_as::<_, Position>(
            "SELECT * FROM positions WHERE account = $1 AND size != '0'"
        )
        .bind(address)
        .fetch_all(self.pool)
        .await
    }

    /// Get open orders for account
    pub async fn get_open_orders(&self, address: &[u8]) -> Result<Vec<Order>, sqlx::Error> {
        sqlx::query_as::<_, Order>(
            "SELECT * FROM orders WHERE account = $1 AND status = 'open' ORDER BY created_at DESC"
        )
        .bind(address)
        .fetch_all(self.pool)
        .await
    }

    /// Get fills for account
    pub async fn get_fills(
        &self,
        address: &[u8],
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: i32,
    ) -> Result<Vec<Fill>, sqlx::Error> {
        let mut query = String::from(
            "SELECT * FROM fills WHERE account = $1"
        );

        if start_time.is_some() {
            query.push_str(" AND timestamp >= to_timestamp($2)");
        }
        if end_time.is_some() {
            query.push_str(" AND timestamp <= to_timestamp($3)");
        }
        query.push_str(" ORDER BY timestamp DESC LIMIT $4");

        sqlx::query_as::<_, Fill>(&query)
            .bind(address)
            .bind(start_time.map(|t| t as f64 / 1000.0))
            .bind(end_time.map(|t| t as f64 / 1000.0))
            .bind(limit)
            .fetch_all(self.pool)
            .await
    }

    /// Get recent trades for market
    pub async fn get_recent_trades(
        &self,
        market_id: i16,
        limit: i32,
    ) -> Result<Vec<Trade>, sqlx::Error> {
        sqlx::query_as::<_, Trade>(
            "SELECT * FROM trades WHERE market_id = $1 ORDER BY timestamp DESC LIMIT $2"
        )
        .bind(market_id)
        .bind(limit)
        .fetch_all(self.pool)
        .await
    }

    /// Get candles
    pub async fn get_candles(
        &self,
        market_id: i16,
        interval: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: i32,
    ) -> Result<Vec<Candle>, sqlx::Error> {
        let mut query = String::from(
            "SELECT * FROM candles WHERE market_id = $1 AND interval = $2"
        );

        if start_time.is_some() {
            query.push_str(" AND timestamp >= to_timestamp($3)");
        }
        if end_time.is_some() {
            query.push_str(" AND timestamp <= to_timestamp($4)");
        }
        query.push_str(" ORDER BY timestamp ASC LIMIT $5");

        sqlx::query_as::<_, Candle>(&query)
            .bind(market_id)
            .bind(interval)
            .bind(start_time.map(|t| t as f64 / 1000.0))
            .bind(end_time.map(|t| t as f64 / 1000.0))
            .bind(limit)
            .fetch_all(self.pool)
            .await
    }

    /// Get funding history for market
    pub async fn get_funding_history(
        &self,
        market_id: i16,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: i32,
    ) -> Result<Vec<FundingRate>, sqlx::Error> {
        let mut query = String::from(
            "SELECT * FROM funding_rates WHERE market_id = $1"
        );

        if start_time.is_some() {
            query.push_str(" AND timestamp >= to_timestamp($2)");
        }
        if end_time.is_some() {
            query.push_str(" AND timestamp <= to_timestamp($3)");
        }
        query.push_str(" ORDER BY timestamp DESC LIMIT $4");

        sqlx::query_as::<_, FundingRate>(&query)
            .bind(market_id)
            .bind(start_time.map(|t| t as f64 / 1000.0))
            .bind(end_time.map(|t| t as f64 / 1000.0))
            .bind(limit)
            .fetch_all(self.pool)
            .await
    }

    /// Get funding payments for account
    pub async fn get_funding_payments(
        &self,
        address: &[u8],
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: i32,
    ) -> Result<Vec<FundingPayment>, sqlx::Error> {
        let mut query = String::from(
            "SELECT * FROM funding_payments WHERE account = $1"
        );

        if start_time.is_some() {
            query.push_str(" AND timestamp >= to_timestamp($2)");
        }
        if end_time.is_some() {
            query.push_str(" AND timestamp <= to_timestamp($3)");
        }
        query.push_str(" ORDER BY timestamp DESC LIMIT $4");

        sqlx::query_as::<_, FundingPayment>(&query)
            .bind(address)
            .bind(start_time.map(|t| t as f64 / 1000.0))
            .bind(end_time.map(|t| t as f64 / 1000.0))
            .bind(limit)
            .fetch_all(self.pool)
            .await
    }

    /// Get liquidations for account
    pub async fn get_liquidations(
        &self,
        address: &[u8],
        limit: i32,
    ) -> Result<Vec<Liquidation>, sqlx::Error> {
        sqlx::query_as::<_, Liquidation>(
            "SELECT * FROM liquidations WHERE account = $1 ORDER BY timestamp DESC LIMIT $2"
        )
        .bind(address)
        .bind(limit)
        .fetch_all(self.pool)
        .await
    }

    /// Get insurance fund balance for market
    pub async fn get_insurance_fund(&self, market_id: i16) -> Result<Option<InsuranceFund>, sqlx::Error> {
        sqlx::query_as::<_, InsuranceFund>(
            "SELECT * FROM insurance_fund WHERE market_id = $1 ORDER BY timestamp DESC LIMIT 1"
        )
        .bind(market_id)
        .fetch_optional(self.pool)
        .await
    }

    /// Get block by height
    pub async fn get_block(&self, height: i64) -> Result<Option<Block>, sqlx::Error> {
        sqlx::query_as::<_, Block>("SELECT * FROM blocks WHERE height = $1")
            .bind(height)
            .fetch_optional(self.pool)
            .await
    }

    /// Get transactions for block
    pub async fn get_block_transactions(&self, height: i64) -> Result<Vec<Transaction>, sqlx::Error> {
        sqlx::query_as::<_, Transaction>(
            "SELECT * FROM transactions WHERE block_height = $1 ORDER BY tx_index"
        )
        .bind(height)
        .fetch_all(self.pool)
        .await
    }

    /// Search orders by client order ID
    pub async fn find_order_by_cloid(
        &self,
        address: &[u8],
        cloid: &str,
    ) -> Result<Option<Order>, sqlx::Error> {
        sqlx::query_as::<_, Order>(
            "SELECT * FROM orders WHERE account = $1 AND cloid = $2"
        )
        .bind(address)
        .bind(cloid)
        .fetch_optional(self.pool)
        .await
    }

    /// Get market stats
    pub async fn get_market_stats(&self, market_id: i16) -> Result<MarketStats, sqlx::Error> {
        let row: (Option<String>, Option<String>, Option<i64>) = sqlx::query_as(
            r#"
            SELECT
                SUM(CAST(size AS DECIMAL)) as volume_24h,
                (SELECT close FROM candles WHERE market_id = $1 AND interval = '1d' ORDER BY timestamp DESC LIMIT 1) as last_price,
                COUNT(*) as trade_count_24h
            FROM trades
            WHERE market_id = $1 AND timestamp > NOW() - INTERVAL '24 hours'
            "#,
        )
        .bind(market_id)
        .fetch_one(self.pool)
        .await?;

        Ok(MarketStats {
            market_id,
            volume_24h: row.0.unwrap_or_else(|| "0".to_string()),
            last_price: row.1.unwrap_or_else(|| "0".to_string()),
            trade_count_24h: row.2.unwrap_or(0),
        })
    }
}

/// Market statistics
#[derive(Debug, Clone)]
pub struct MarketStats {
    pub market_id: i16,
    pub volume_24h: String,
    pub last_price: String,
    pub trade_count_24h: i64,
}
