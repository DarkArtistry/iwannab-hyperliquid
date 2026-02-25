//! Candle (OHLCV) aggregation from trade fills
//!
//! Aggregates fills into candles at various intervals:
//! - 1m, 5m, 15m, 1h, 4h, 1d

use chrono::{DateTime, Duration, TimeZone, Utc};
use sqlx::PgPool;

/// Supported candle intervals
pub const INTERVALS: &[&str] = &["1m", "5m", "15m", "1h", "4h", "1d"];

/// Candle aggregator
pub struct CandleAggregator {
    pool: PgPool,
}

impl CandleAggregator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Update candles for a fill
    pub async fn update_from_fill(
        &self,
        market_id: i16,
        price: &str,
        size: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<(), CandleError> {
        for interval in INTERVALS {
            self.upsert_candle(market_id, interval, price, size, timestamp)
                .await?;
        }
        Ok(())
    }

    /// Upsert a candle for a specific interval
    async fn upsert_candle(
        &self,
        market_id: i16,
        interval: &str,
        price: &str,
        size: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<(), CandleError> {
        let (open_time, close_time) = calculate_candle_bounds(timestamp, interval);

        // Upsert candle: update OHLCV if exists, otherwise insert new
        sqlx::query(
            r#"
            INSERT INTO candles (market_id, interval, open_time, close_time, open, high, low, close, volume, trade_count)
            VALUES ($1, $2, $3, $4, $5, $5, $5, $5, $6, 1)
            ON CONFLICT (market_id, interval, open_time)
            DO UPDATE SET
                high = GREATEST(candles.high, EXCLUDED.open),
                low = LEAST(candles.low, EXCLUDED.open),
                close = EXCLUDED.open,
                volume = candles.volume + EXCLUDED.volume,
                trade_count = candles.trade_count + 1
            "#,
        )
        .bind(market_id)
        .bind(interval)
        .bind(open_time)
        .bind(close_time)
        .bind(price)
        .bind(size)
        .execute(&self.pool)
        .await
        .map_err(|e| CandleError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Backfill candles from existing fills (for initial setup or recovery)
    pub async fn backfill_from_fills(&self, market_id: i16) -> Result<u64, CandleError> {
        // Get all fills for the market ordered by timestamp
        let fills: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as(
            r#"
            SELECT price::TEXT, size::TEXT, timestamp
            FROM fills
            WHERE market_id = $1
            ORDER BY timestamp ASC
            "#,
        )
        .bind(market_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CandleError::DatabaseError(e.to_string()))?;

        let count = fills.len() as u64;
        for (price, size, timestamp) in fills {
            self.update_from_fill(market_id, &price, &size, timestamp)
                .await?;
        }

        Ok(count)
    }
}

/// Calculate candle open/close times for a given timestamp and interval
fn calculate_candle_bounds(timestamp: DateTime<Utc>, interval: &str) -> (DateTime<Utc>, DateTime<Utc>) {
    let duration = match interval {
        "1m" => Duration::minutes(1),
        "5m" => Duration::minutes(5),
        "15m" => Duration::minutes(15),
        "1h" => Duration::hours(1),
        "4h" => Duration::hours(4),
        "1d" => Duration::days(1),
        _ => Duration::minutes(1), // Default to 1m
    };

    let duration_secs = duration.num_seconds();
    let ts_secs = timestamp.timestamp();
    let open_secs = (ts_secs / duration_secs) * duration_secs;

    let open_time = Utc.timestamp_opt(open_secs, 0).unwrap();
    let close_time = open_time + duration;

    (open_time, close_time)
}

/// Candle aggregation errors
#[derive(Debug, thiserror::Error)]
pub enum CandleError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candle_bounds_1m() {
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 34, 56).unwrap();
        let (open, close) = calculate_candle_bounds(ts, "1m");

        assert_eq!(open, Utc.with_ymd_and_hms(2024, 1, 15, 12, 34, 0).unwrap());
        assert_eq!(close, Utc.with_ymd_and_hms(2024, 1, 15, 12, 35, 0).unwrap());
    }

    #[test]
    fn test_candle_bounds_5m() {
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 34, 56).unwrap();
        let (open, close) = calculate_candle_bounds(ts, "5m");

        assert_eq!(open, Utc.with_ymd_and_hms(2024, 1, 15, 12, 30, 0).unwrap());
        assert_eq!(close, Utc.with_ymd_and_hms(2024, 1, 15, 12, 35, 0).unwrap());
    }

    #[test]
    fn test_candle_bounds_1h() {
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 34, 56).unwrap();
        let (open, close) = calculate_candle_bounds(ts, "1h");

        assert_eq!(open, Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap());
        assert_eq!(close, Utc.with_ymd_and_hms(2024, 1, 15, 13, 0, 0).unwrap());
    }

    #[test]
    fn test_candle_bounds_1d() {
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 34, 56).unwrap();
        let (open, close) = calculate_candle_bounds(ts, "1d");

        assert_eq!(open, Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap());
        assert_eq!(close, Utc.with_ymd_and_hms(2024, 1, 16, 0, 0, 0).unwrap());
    }
}
