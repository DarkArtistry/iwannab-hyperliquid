//! Database models

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Block model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Block {
    pub height: i64,
    pub hash: Vec<u8>,
    pub timestamp: i64,
    pub proposer: Option<Vec<u8>>,
    pub tx_count: i32,
}

/// Transaction model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Transaction {
    pub hash: Vec<u8>,
    pub block_height: i64,
    pub tx_index: i32,
    pub sender: Vec<u8>,
    pub tx_type: String,
    pub data: serde_json::Value,
    pub success: bool,
    pub error: Option<String>,
}

/// Account model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Account {
    pub address: Vec<u8>,
    pub balance: String,
    pub equity: String,
    pub margin_used: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Market model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Market {
    pub id: i16,
    pub name: String,
    pub tick_size: String,
    pub lot_size: String,
    pub max_leverage: i16,
    pub maker_fee: String,
    pub taker_fee: String,
    pub funding_interval: i32,
}

/// Position model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Position {
    pub account: Vec<u8>,
    pub market_id: i16,
    pub size: String,
    pub entry_price: String,
    pub unrealized_pnl: String,
    pub leverage: i16,
    pub liquidation_price: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Order model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Order {
    pub id: i64,
    pub account: Vec<u8>,
    pub market_id: i16,
    pub side: String,
    pub price: String,
    pub size: String,
    pub filled_size: String,
    pub status: String,
    pub order_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cloid: Option<String>,
}

/// Fill model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Fill {
    pub trade_id: i64,
    pub order_id: i64,
    pub account: Vec<u8>,
    pub market_id: i16,
    pub side: String,
    pub price: String,
    pub size: String,
    pub fee: String,
    pub is_taker: bool,
    pub realized_pnl: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Trade model (aggregated view of fills)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Trade {
    pub id: i64,
    pub market_id: i16,
    pub price: String,
    pub size: String,
    pub side: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub maker_order_id: i64,
    pub taker_order_id: i64,
}

/// Candle model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Candle {
    pub market_id: i16,
    pub interval: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub trades: i32,
}

/// Funding rate model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FundingRate {
    pub market_id: i16,
    pub rate: String,
    pub premium: String,
    pub mark_price: String,
    pub index_price: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Funding payment model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FundingPayment {
    pub account: Vec<u8>,
    pub market_id: i16,
    pub payment: String,
    pub position_size: String,
    pub funding_rate: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Liquidation model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Liquidation {
    pub id: i64,
    pub account: Vec<u8>,
    pub liquidator: Option<Vec<u8>>,
    pub market_id: i16,
    pub size: String,
    pub price: String,
    pub bankruptcy_price: Option<String>,
    pub insurance_fund_used: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// ADL event model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AdlEvent {
    pub id: i64,
    pub liquidated_account: Vec<u8>,
    pub counterparty: Vec<u8>,
    pub market_id: i16,
    pub size: String,
    pub price: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Insurance fund model
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct InsuranceFund {
    pub market_id: i16,
    pub balance: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
