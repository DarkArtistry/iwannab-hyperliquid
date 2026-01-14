//! Order types and structures

use crate::{AccountAddress, Decimal, Timestamp};
use serde::{Deserialize, Serialize};

/// Unique order identifier
pub type OrderId = u64;

/// Client-provided order identifier
pub type ClientOrderId = String;

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    pub fn opposite(&self) -> Self {
        match self {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        }
    }

    pub fn sign(&self) -> i64 {
        match self {
            OrderSide::Buy => 1,
            OrderSide::Sell => -1,
        }
    }
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "buy"),
            OrderSide::Sell => write!(f, "sell"),
        }
    }
}

/// Time-in-force for limit orders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good-til-canceled
    Gtc,
    /// Immediate-or-cancel
    Ioc,
    /// Fill-or-kill
    Fok,
    /// Post-only (Add-liquidity-only)
    Alo,
}

/// Order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Limit order with TIF
    Limit { tif: TimeInForce },
    /// Market order (executes as IOC at worst price)
    Market,
}

impl Default for OrderType {
    fn default() -> Self {
        OrderType::Limit { tif: TimeInForce::Gtc }
    }
}

impl std::fmt::Display for OrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderType::Limit { tif } => write!(f, "limit:{:?}", tif),
            OrderType::Market => write!(f, "market"),
        }
    }
}

/// Order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Order is open on the book
    Open,
    /// Order is partially filled
    PartiallyFilled,
    /// Order is completely filled
    Filled,
    /// Order was canceled
    Canceled,
    /// Order was rejected
    Rejected,
}

/// Cancel reason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancelReason {
    /// User requested cancellation
    UserRequested,
    /// Self-trade prevention
    SelfTrade,
    /// IOC unfilled portion
    IocUnfilled,
    /// FOK could not be fully filled
    FokNotFilled,
    /// Post-only would have crossed
    PostOnlyWouldCross,
    /// Reduce-only would increase position
    ReduceOnlyViolation,
    /// Insufficient margin
    InsufficientMargin,
    /// Market closed
    MarketClosed,
    /// Liquidation
    Liquidation,
    /// Position closed
    PositionClosed,
}

/// Order request (input from user)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    /// Market ID
    pub market_id: u8,
    /// Buy or sell
    pub side: OrderSide,
    /// Limit price (for limit orders)
    pub price: Decimal,
    /// Order size
    pub size: Decimal,
    /// Order type and TIF
    pub order_type: OrderType,
    /// Reduce-only flag
    pub reduce_only: bool,
    /// Client order ID (optional)
    pub client_order_id: Option<ClientOrderId>,
}

/// Order stored in the orderbook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Unique order ID
    pub id: OrderId,
    /// Owner address
    pub owner: AccountAddress,
    /// Market ID
    pub market_id: u8,
    /// Side
    pub side: OrderSide,
    /// Limit price
    pub price: Decimal,
    /// Original order size
    pub original_size: Decimal,
    /// Remaining size
    pub remaining_size: Decimal,
    /// Order type
    pub order_type: OrderType,
    /// Reduce-only flag
    pub reduce_only: bool,
    /// Post-only flag (derived from order_type)
    pub post_only: bool,
    /// Client order ID
    pub client_order_id: Option<ClientOrderId>,
    /// Creation timestamp
    pub timestamp: Timestamp,
    /// Current status
    pub status: OrderStatus,
}

impl Order {
    /// Create a new order from a request
    pub fn new(
        id: OrderId,
        owner: AccountAddress,
        request: OrderRequest,
        timestamp: Timestamp,
    ) -> Self {
        let post_only = matches!(request.order_type, OrderType::Limit { tif: TimeInForce::Alo });

        Self {
            id,
            owner,
            market_id: request.market_id,
            side: request.side,
            price: request.price,
            original_size: request.size,
            remaining_size: request.size,
            order_type: request.order_type,
            reduce_only: request.reduce_only,
            post_only,
            client_order_id: request.client_order_id,
            timestamp,
            status: OrderStatus::Open,
        }
    }

    /// Check if order is fully filled
    pub fn is_filled(&self) -> bool {
        self.remaining_size.is_zero()
    }

    /// Check if order should rest on book
    pub fn should_rest(&self) -> bool {
        match self.order_type {
            OrderType::Limit { tif } => matches!(tif, TimeInForce::Gtc | TimeInForce::Alo),
            OrderType::Market => false,
        }
    }

    /// Check if order is IOC
    pub fn is_ioc(&self) -> bool {
        matches!(self.order_type, OrderType::Limit { tif: TimeInForce::Ioc } | OrderType::Market)
    }

    /// Check if order is FOK
    pub fn is_fok(&self) -> bool {
        matches!(self.order_type, OrderType::Limit { tif: TimeInForce::Fok })
    }

    /// Fill a portion of the order
    pub fn fill(&mut self, size: Decimal) {
        debug_assert!(size <= self.remaining_size);
        self.remaining_size = self.remaining_size - size;
        if self.remaining_size.is_zero() {
            self.status = OrderStatus::Filled;
        } else {
            self.status = OrderStatus::PartiallyFilled;
        }
    }

    /// Cancel the order
    pub fn cancel(&mut self) {
        self.status = OrderStatus::Canceled;
    }

    /// Calculate filled size
    pub fn filled_size(&self) -> Decimal {
        self.original_size - self.remaining_size
    }

    /// Calculate notional value
    pub fn notional(&self) -> Decimal {
        self.remaining_size * self.price
    }
}

/// Order key for orderbook sorting (price-time priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderKey {
    /// Price (inverted for bids so larger prices come first)
    pub price_key: i128,
    /// Timestamp for FIFO at same price
    pub timestamp: Timestamp,
    /// Order ID as tiebreaker
    pub order_id: OrderId,
}

impl OrderKey {
    /// Create key for a bid order (price descending)
    pub fn bid(price: Decimal, timestamp: Timestamp, order_id: OrderId) -> Self {
        Self {
            price_key: -price.raw(), // Negate for descending order
            timestamp,
            order_id,
        }
    }

    /// Create key for an ask order (price ascending)
    pub fn ask(price: Decimal, timestamp: Timestamp, order_id: OrderId) -> Self {
        Self {
            price_key: price.raw(),
            timestamp,
            order_id,
        }
    }
}

impl Ord for OrderKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.price_key
            .cmp(&other.price_key)
            .then(self.timestamp.cmp(&other.timestamp))
            .then(self.order_id.cmp(&other.order_id))
    }
}

impl PartialOrd for OrderKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;

    #[test]
    fn test_order_key_bid_ordering() {
        // Higher prices should come first for bids
        let k1 = OrderKey::bid(Decimal::price("100"), 1, 1);
        let k2 = OrderKey::bid(Decimal::price("101"), 2, 2);
        assert!(k2 < k1); // 101 should be before 100

        // Same price: earlier timestamp first
        let k3 = OrderKey::bid(Decimal::price("100"), 1, 3);
        let k4 = OrderKey::bid(Decimal::price("100"), 2, 4);
        assert!(k3 < k4);
    }

    #[test]
    fn test_order_key_ask_ordering() {
        // Lower prices should come first for asks
        let k1 = OrderKey::ask(Decimal::price("100"), 1, 1);
        let k2 = OrderKey::ask(Decimal::price("101"), 2, 2);
        assert!(k1 < k2); // 100 should be before 101
    }

    #[test]
    fn test_order_fill() {
        let mut order = Order::new(
            1,
            Address::ZERO,
            OrderRequest {
                market_id: 0,
                side: OrderSide::Buy,
                price: Decimal::price("100"),
                size: Decimal::size("10"),
                order_type: OrderType::default(),
                reduce_only: false,
                client_order_id: None,
            },
            1000,
        );

        assert_eq!(order.status, OrderStatus::Open);

        order.fill(Decimal::size("3"));
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
        assert_eq!(order.remaining_size.to_string_trimmed(), "7");

        order.fill(Decimal::size("7"));
        assert_eq!(order.status, OrderStatus::Filled);
        assert!(order.is_filled());
    }
}
