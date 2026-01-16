//! Block events for indexing and real-time updates
//!
//! These events are emitted during block execution and consumed by:
//! - Indexer for database persistence
//! - WebSocket subscriptions for real-time updates
//! - Block explorers and analytics

use crate::{
    AccountAddress, CancelReason, Decimal, MarketId, OrderId, OrderSide, SignedAmount, Timestamp,
};
use serde::{Deserialize, Serialize};

// Custom serialization for i128 as string (JSON doesn't support i128 natively)
mod signed_amount_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &i128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Events emitted during block execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BlockEvent {
    /// Trade fill event (emitted for both maker and taker)
    #[serde(rename = "fill")]
    Fill(FillEvent),

    /// Order placed event
    #[serde(rename = "orderPlaced")]
    OrderPlaced(OrderPlacedEvent),

    /// Order canceled event
    #[serde(rename = "orderCanceled")]
    OrderCanceled(OrderCanceledEvent),

    /// Position updated event
    #[serde(rename = "positionUpdated")]
    PositionUpdated(PositionUpdatedEvent),

    /// Funding rate applied event
    #[serde(rename = "fundingApplied")]
    FundingApplied(FundingAppliedEvent),

    /// Liquidation event
    #[serde(rename = "liquidation")]
    Liquidation(LiquidationEvent),

    /// USD transfer event
    #[serde(rename = "usdTransfer")]
    UsdTransfer(UsdTransferEvent),

    /// Leverage update event
    #[serde(rename = "leverageUpdated")]
    LeverageUpdated(LeverageUpdatedEvent),
}

/// Fill event - emitted when an order is filled
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillEvent {
    /// Unique trade ID
    pub trade_id: u64,
    /// Order ID that was filled
    pub order_id: OrderId,
    /// Account that received the fill
    pub account: AccountAddress,
    /// Market ID
    pub market_id: MarketId,
    /// Order side (Buy/Sell)
    pub side: OrderSide,
    /// Fill price
    pub price: Decimal,
    /// Fill size
    pub size: Decimal,
    /// Fee charged
    pub fee: Decimal,
    /// Whether this account was the taker
    pub is_taker: bool,
    /// Realized PnL from this fill
    #[serde(with = "signed_amount_serde")]
    pub realized_pnl: SignedAmount,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// Order placed event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderPlacedEvent {
    /// Order ID
    pub order_id: OrderId,
    /// Account that placed the order
    pub account: AccountAddress,
    /// Market ID
    pub market_id: MarketId,
    /// Order side
    pub side: OrderSide,
    /// Limit price
    pub price: Decimal,
    /// Order size
    pub size: Decimal,
    /// Whether this is a reduce-only order
    pub reduce_only: bool,
    /// Client order ID (if provided)
    pub client_order_id: Option<String>,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// Order canceled event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderCanceledEvent {
    /// Order ID
    pub order_id: OrderId,
    /// Account that owned the order
    pub account: AccountAddress,
    /// Market ID
    pub market_id: MarketId,
    /// Reason for cancellation
    pub reason: CancelReason,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// Position updated event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionUpdatedEvent {
    /// Account
    pub account: AccountAddress,
    /// Market ID
    pub market_id: MarketId,
    /// New position size (positive = long, negative = short)
    #[serde(with = "signed_amount_serde")]
    pub size: SignedAmount,
    /// Entry notional value
    pub entry_notional: Decimal,
    /// Current leverage setting
    pub leverage: u8,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// Funding rate applied event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingAppliedEvent {
    /// Market ID
    pub market_id: MarketId,
    /// Funding rate (positive = longs pay shorts)
    #[serde(with = "signed_amount_serde")]
    pub funding_rate: SignedAmount,
    /// Mark price at funding time
    pub mark_price: Decimal,
    /// Index price at funding time
    pub index_price: Decimal,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// Liquidation event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidationEvent {
    /// Account being liquidated
    pub account: AccountAddress,
    /// Market ID
    pub market_id: MarketId,
    /// Position size being liquidated
    #[serde(with = "signed_amount_serde")]
    pub size: SignedAmount,
    /// Liquidation price
    pub price: Decimal,
    /// PnL realized from liquidation
    #[serde(with = "signed_amount_serde")]
    pub pnl: SignedAmount,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// USD transfer event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsdTransferEvent {
    /// Sender account
    pub from: AccountAddress,
    /// Recipient account
    pub to: AccountAddress,
    /// Amount transferred
    pub amount: Decimal,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

/// Leverage updated event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverageUpdatedEvent {
    /// Account
    pub account: AccountAddress,
    /// Market ID
    pub market_id: MarketId,
    /// New leverage value
    pub leverage: u8,
    /// Whether using cross margin
    pub is_cross: bool,
    /// Timestamp in milliseconds
    pub timestamp: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = BlockEvent::Fill(FillEvent {
            trade_id: 1,
            order_id: 100,
            account: AccountAddress::default(),
            market_id: 0,
            side: OrderSide::Buy,
            price: Decimal::from_raw(65000_000000, 6),
            size: Decimal::from_raw(1_000000, 6),
            fee: Decimal::from_raw(100, 6),
            is_taker: true,
            realized_pnl: 0,
            timestamp: 1234567890,
        });

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("fill"));
        assert!(json.contains("tradeId"));

        let parsed: BlockEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            BlockEvent::Fill(f) => assert_eq!(f.trade_id, 1),
            _ => panic!("Expected Fill event"),
        }
    }
}
