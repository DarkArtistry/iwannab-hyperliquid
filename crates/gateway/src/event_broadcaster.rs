//! Event Broadcaster - Connects block events to WebSocket broadcasting
//!
//! This module provides the integration layer between the trading engine's
//! block events and the WebSocket server for real-time client updates.

use std::sync::Arc;

use hypercore_primitives::{
    AccountAddress, BlockEvent, FillEvent, LiquidationEvent, MarketId,
    OrderCanceledEvent, OrderPlacedEvent, OrderSide, PositionUpdatedEvent,
};

use crate::websocket::{
    FillUpdate, OrderUpdate, PositionUpdate, TradeUpdate, WsManager,
};

/// Event broadcaster that forwards block events to WebSocket clients
pub struct EventBroadcaster {
    ws_manager: Arc<WsManager>,
    market_names: Vec<String>,
}

impl EventBroadcaster {
    /// Create a new event broadcaster
    pub fn new(ws_manager: Arc<WsManager>, market_names: Vec<String>) -> Self {
        Self {
            ws_manager,
            market_names,
        }
    }

    /// Get market name from market ID
    fn get_market_name(&self, market_id: MarketId) -> String {
        self.market_names
            .get(market_id as usize)
            .cloned()
            .unwrap_or_else(|| format!("MARKET-{}", market_id))
    }

    /// Format address as hex string (checksummed EIP-55)
    fn format_address(addr: &AccountAddress) -> String {
        format!("{}", addr)
    }

    /// Format SignedAmount (i128) to string
    fn format_signed_amount(amount: i128) -> String {
        // Convert from raw value (scaled by 10^6 for USDC decimals)
        let scale = 1_000_000i128;
        let abs_val = amount.abs();
        let integer = abs_val / scale;
        let decimal = abs_val % scale;

        if decimal == 0 {
            if amount < 0 {
                format!("-{}", integer)
            } else {
                format!("{}", integer)
            }
        } else {
            let decimal_str = format!("{:06}", decimal).trim_end_matches('0').to_string();
            if amount < 0 {
                format!("-{}.{}", integer, decimal_str)
            } else {
                format!("{}.{}", integer, decimal_str)
            }
        }
    }

    /// Broadcast a block event to relevant WebSocket subscribers
    pub async fn broadcast_event(&self, event: &BlockEvent) {
        match event {
            BlockEvent::Fill(fill) => self.broadcast_fill(fill).await,
            BlockEvent::OrderPlaced(order) => self.broadcast_order_placed(order).await,
            BlockEvent::OrderCanceled(canceled) => self.broadcast_order_canceled(canceled).await,
            BlockEvent::PositionUpdated(position) => self.broadcast_position(position).await,
            BlockEvent::Liquidation(liquidation) => self.broadcast_liquidation(liquidation).await,
            // Other events don't need WebSocket broadcasting yet
            _ => {}
        }
    }

    /// Broadcast multiple events
    pub async fn broadcast_events(&self, events: &[BlockEvent]) {
        for event in events {
            self.broadcast_event(event).await;
        }
    }

    /// Broadcast a fill event
    ///
    /// Note: Each FillEvent represents one side of a trade (for one account).
    /// The engine emits two FillEvents per trade - one for maker, one for taker.
    async fn broadcast_fill(&self, fill: &FillEvent) {
        let coin = self.get_market_name(fill.market_id);
        let user = Self::format_address(&fill.account);

        // Determine side string
        let side = match fill.side {
            OrderSide::Buy => "B".to_string(),
            OrderSide::Sell => "A".to_string(),
        };

        // Create fill update for this account
        let fill_update = FillUpdate {
            coin: coin.clone(),
            px: fill.price.to_string_trimmed(),
            sz: fill.size.to_string_trimmed(),
            side: side.clone(),
            time: fill.timestamp,
            fee: fill.fee.to_string_trimmed(),
            oid: fill.order_id,
            crossed: fill.is_taker, // Taker crossed the spread
            closed_pnl: Self::format_signed_amount(fill.realized_pnl),
        };

        // Broadcast to the user
        self.ws_manager.broadcast_fill(&user, fill_update).await;

        // Also broadcast as a trade to the market channel (only for takers to avoid duplicates)
        if fill.is_taker {
            let trade = TradeUpdate {
                coin,
                side,
                px: fill.price.to_string_trimmed(),
                sz: fill.size.to_string_trimmed(),
                time: fill.timestamp,
                hash: format!("{:016x}", fill.trade_id),
            };

            self.ws_manager
                .broadcast_trade(&self.get_market_name(fill.market_id), trade)
                .await;
        }
    }

    /// Broadcast an order placed event
    async fn broadcast_order_placed(&self, order: &OrderPlacedEvent) {
        let user = Self::format_address(&order.account);
        let coin = self.get_market_name(order.market_id);

        let update = OrderUpdate {
            coin,
            oid: order.order_id,
            status: "open".to_string(),
            sz: order.size.to_string_trimmed(),
            filled_sz: "0".to_string(),
            limit_px: order.price.to_string_trimmed(),
            side: match order.side {
                OrderSide::Buy => "B".to_string(),
                OrderSide::Sell => "A".to_string(),
            },
            timestamp: order.timestamp,
        };

        self.ws_manager.broadcast_order(&user, update).await;
    }

    /// Broadcast an order canceled event
    async fn broadcast_order_canceled(&self, canceled: &OrderCanceledEvent) {
        let user = Self::format_address(&canceled.account);
        let coin = self.get_market_name(canceled.market_id);

        let update = OrderUpdate {
            coin,
            oid: canceled.order_id,
            status: format!("canceled:{:?}", canceled.reason).to_lowercase(),
            sz: "0".to_string(),
            filled_sz: "0".to_string(),
            limit_px: "0".to_string(),
            side: "".to_string(),
            timestamp: canceled.timestamp,
        };

        self.ws_manager.broadcast_order(&user, update).await;
    }

    /// Broadcast a position updated event
    async fn broadcast_position(&self, position: &PositionUpdatedEvent) {
        let user = Self::format_address(&position.account);
        let coin = self.get_market_name(position.market_id);

        // Convert signed amount (i128) to string
        let szi = Self::format_signed_amount(position.size);

        let update = PositionUpdate {
            coin,
            szi,
            entry_px: position.entry_notional.to_string_trimmed(),
            unrealized_pnl: "0".to_string(), // Would need mark price to calculate
            leverage: position.leverage,
            liquidation_px: None, // Would need to calculate
        };

        self.ws_manager.broadcast_position(&user, update).await;
    }

    /// Broadcast a liquidation event
    async fn broadcast_liquidation(&self, liquidation: &LiquidationEvent) {
        let user = Self::format_address(&liquidation.account);
        let coin = self.get_market_name(liquidation.market_id);

        // Determine if was long or short from size sign (positive = was long)
        let was_long = liquidation.size > 0;
        let abs_size = liquidation.size.abs();

        // Broadcast a position update indicating liquidation
        let update = PositionUpdate {
            coin: coin.clone(),
            szi: "0".to_string(), // Position closed
            entry_px: "0".to_string(),
            unrealized_pnl: Self::format_signed_amount(liquidation.pnl),
            leverage: 0,
            liquidation_px: Some(liquidation.price.to_string_trimmed()),
        };

        self.ws_manager.broadcast_position(&user, update).await;

        // Also broadcast the fill from liquidation
        // Closing a long = sell, closing a short = buy
        let fill = FillUpdate {
            coin,
            px: liquidation.price.to_string_trimmed(),
            sz: Self::format_signed_amount(abs_size),
            side: if was_long { "A".to_string() } else { "B".to_string() },
            time: liquidation.timestamp,
            fee: "0".to_string(),
            oid: 0, // Liquidation has no order ID
            crossed: true,
            closed_pnl: Self::format_signed_amount(liquidation.pnl),
        };

        self.ws_manager.broadcast_fill(&user, fill).await;
    }
}

/// Shared event broadcaster
pub type SharedEventBroadcaster = Arc<EventBroadcaster>;

#[cfg(test)]
mod tests {
    use super::*;
    use hypercore_primitives::Decimal;

    #[test]
    fn test_format_address() {
        let addr = AccountAddress::from([0x12; 20]);
        let formatted = EventBroadcaster::format_address(&addr);
        assert!(formatted.starts_with("0x"));
        assert_eq!(formatted.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_format_signed_amount() {
        // Positive amount
        assert_eq!(EventBroadcaster::format_signed_amount(1_000_000), "1");
        assert_eq!(EventBroadcaster::format_signed_amount(1_500_000), "1.5");
        assert_eq!(EventBroadcaster::format_signed_amount(123_456_789), "123.456789");

        // Negative amount
        assert_eq!(EventBroadcaster::format_signed_amount(-1_000_000), "-1");
        assert_eq!(EventBroadcaster::format_signed_amount(-1_500_000), "-1.5");

        // Zero
        assert_eq!(EventBroadcaster::format_signed_amount(0), "0");

        // Small amounts
        assert_eq!(EventBroadcaster::format_signed_amount(1), "0.000001");
        assert_eq!(EventBroadcaster::format_signed_amount(100_000), "0.1");
    }

    #[tokio::test]
    async fn test_broadcast_fill() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string(), "ETH-PERP".to_string()],
        );

        // Create a fill event (per-account event)
        let fill = FillEvent {
            trade_id: 1,
            order_id: 100,
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            side: OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            fee: Decimal::usdc("3.25"),
            is_taker: true,
            realized_pnl: 100_000_000, // $100 in micros
            timestamp: 1706054400000,
        };

        // This should not panic
        broadcaster.broadcast_fill(&fill).await;
    }

    #[tokio::test]
    async fn test_broadcast_order_placed() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        let order = OrderPlacedEvent {
            order_id: 123,
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            side: OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            reduce_only: false,
            client_order_id: Some("test".to_string()),
            timestamp: 1706054400000,
        };

        // This should not panic
        broadcaster.broadcast_order_placed(&order).await;
    }

    #[tokio::test]
    async fn test_broadcast_liquidation() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        // Create a liquidation event for a long position
        let liquidation = LiquidationEvent {
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            size: 100_000_000, // 0.1 BTC (positive = was long)
            price: Decimal::price("60000"),
            pnl: -500_000_000, // Lost $500
            timestamp: 1706054400000,
        };

        // This should not panic
        broadcaster.broadcast_liquidation(&liquidation).await;
    }

    #[tokio::test]
    async fn test_broadcast_position_update() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        let position = PositionUpdatedEvent {
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            size: 500_000_000, // 0.5 BTC long
            entry_notional: Decimal::usdc("32500"), // Entry notional
            leverage: 10,
            timestamp: 1706054400000,
        };

        // This should not panic
        broadcaster.broadcast_position(&position).await;
    }

    #[test]
    fn test_get_market_name() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string(), "ETH-PERP".to_string(), "SOL-PERP".to_string()],
        );

        // Valid market IDs
        assert_eq!(broadcaster.get_market_name(0), "BTC-PERP");
        assert_eq!(broadcaster.get_market_name(1), "ETH-PERP");
        assert_eq!(broadcaster.get_market_name(2), "SOL-PERP");
    }

    #[test]
    fn test_get_market_name_fallback() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        // Unknown market ID should return fallback
        assert_eq!(broadcaster.get_market_name(99), "MARKET-99");
        assert_eq!(broadcaster.get_market_name(255), "MARKET-255");
    }

    #[tokio::test]
    async fn test_broadcast_order_canceled() {
        use hypercore_primitives::CancelReason;

        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        let canceled = OrderCanceledEvent {
            order_id: 456,
            account: AccountAddress::from([2u8; 20]),
            market_id: 0,
            reason: CancelReason::UserRequested,
            timestamp: 1706054400000,
        };

        // This should not panic
        broadcaster.broadcast_order_canceled(&canceled).await;
    }

    #[tokio::test]
    async fn test_broadcast_events() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        let fill = FillEvent {
            trade_id: 1,
            order_id: 100,
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            side: OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            fee: Decimal::usdc("3.25"),
            is_taker: true,
            realized_pnl: 0,
            timestamp: 1706054400000,
        };

        let position = PositionUpdatedEvent {
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            size: 100_000_000,
            entry_notional: Decimal::usdc("6500"),
            leverage: 10,
            timestamp: 1706054400000,
        };

        let events = vec![
            BlockEvent::Fill(fill),
            BlockEvent::PositionUpdated(position),
        ];

        // Broadcast multiple events - should not panic
        broadcaster.broadcast_events(&events).await;
    }

    #[tokio::test]
    async fn test_broadcast_event_dispatch() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        // Test that each event type is dispatched correctly
        let fill = BlockEvent::Fill(FillEvent {
            trade_id: 1,
            order_id: 100,
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            side: OrderSide::Sell,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            fee: Decimal::usdc("3.25"),
            is_taker: false, // Maker fill
            realized_pnl: 50_000_000, // $50 profit
            timestamp: 1706054400000,
        });

        broadcaster.broadcast_event(&fill).await;

        // Test order placed event
        let order = BlockEvent::OrderPlaced(OrderPlacedEvent {
            order_id: 200,
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            side: OrderSide::Sell,
            price: Decimal::price("70000"),
            size: Decimal::size("0.5"),
            reduce_only: true,
            client_order_id: Some("my-order-1".to_string()),
            timestamp: 1706054400000,
        });

        broadcaster.broadcast_event(&order).await;
    }

    #[test]
    fn test_format_signed_amount_edge_cases() {
        // Large positive amount
        assert_eq!(EventBroadcaster::format_signed_amount(1_000_000_000_000), "1000000");

        // Large negative amount
        assert_eq!(EventBroadcaster::format_signed_amount(-1_000_000_000_000), "-1000000");

        // Amount with trailing zeros in decimal part
        assert_eq!(EventBroadcaster::format_signed_amount(1_100_000), "1.1");
        assert_eq!(EventBroadcaster::format_signed_amount(1_010_000), "1.01");
        assert_eq!(EventBroadcaster::format_signed_amount(1_001_000), "1.001");

        // Very small amounts
        assert_eq!(EventBroadcaster::format_signed_amount(10), "0.00001");
        assert_eq!(EventBroadcaster::format_signed_amount(-10), "-0.00001");

        // Max precision
        assert_eq!(EventBroadcaster::format_signed_amount(123_456), "0.123456");
    }

    #[tokio::test]
    async fn test_broadcast_fill_maker_vs_taker() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        // Taker fill (should also broadcast trade)
        let taker_fill = FillEvent {
            trade_id: 1,
            order_id: 100,
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            side: OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            fee: Decimal::usdc("3.25"),
            is_taker: true,
            realized_pnl: 0,
            timestamp: 1706054400000,
        };

        broadcaster.broadcast_fill(&taker_fill).await;

        // Maker fill (should NOT broadcast trade to avoid duplicates)
        let maker_fill = FillEvent {
            trade_id: 1,
            order_id: 101,
            account: AccountAddress::from([2u8; 20]),
            market_id: 0,
            side: OrderSide::Sell,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            fee: Decimal::usdc("-0.65"), // Maker rebate
            is_taker: false,
            realized_pnl: -100_000_000, // $100 loss
            timestamp: 1706054400000,
        };

        broadcaster.broadcast_fill(&maker_fill).await;
    }

    #[tokio::test]
    async fn test_broadcast_liquidation_short_position() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        // Liquidation of a short position (negative size)
        let liquidation = LiquidationEvent {
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            size: -100_000_000, // Negative = was short
            price: Decimal::price("70000"),
            pnl: -300_000_000, // Lost $300
            timestamp: 1706054400000,
        };

        // This should not panic
        broadcaster.broadcast_liquidation(&liquidation).await;
    }

    #[tokio::test]
    async fn test_broadcast_position_negative_size() {
        let ws_manager = Arc::new(WsManager::new());
        let broadcaster = EventBroadcaster::new(
            ws_manager,
            vec!["BTC-PERP".to_string()],
        );

        // Short position
        let position = PositionUpdatedEvent {
            account: AccountAddress::from([1u8; 20]),
            market_id: 0,
            size: -250_000_000, // 0.25 BTC short
            entry_notional: Decimal::usdc("16250"),
            leverage: 5,
            timestamp: 1706054400000,
        };

        broadcaster.broadcast_position(&position).await;
    }
}
