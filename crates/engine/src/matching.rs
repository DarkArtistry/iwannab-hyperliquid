//! Deterministic order matching engine

use crate::orderbook::OrderBook;
use hypercore_primitives::{
    AccountAddress, CancelReason, Decimal, Error, Fill, MarketId, Order, OrderSide, OrderStatus,
    Position, RawAmount, RawPrice, Result, TimeInForce, Timestamp,
};
use std::collections::HashMap;

/// Matching engine handling order execution
pub struct MatchingEngine {
    /// Order books per market
    orderbooks: HashMap<MarketId, OrderBook>,
    /// Next order ID counter
    next_order_id: u64,
}

impl MatchingEngine {
    /// Create a new matching engine
    pub fn new() -> Self {
        Self {
            orderbooks: HashMap::new(),
            next_order_id: 1,
        }
    }

    /// Add an orderbook for a market
    pub fn add_orderbook(&mut self, market_id: MarketId) {
        self.orderbooks.insert(market_id, OrderBook::new());
    }

    /// Get orderbook for a market
    pub fn get_orderbook(&self, market_id: MarketId) -> Option<&OrderBook> {
        self.orderbooks.get(&market_id)
    }

    /// Get mutable orderbook
    pub fn get_orderbook_mut(&mut self, market_id: MarketId) -> Option<&mut OrderBook> {
        self.orderbooks.get_mut(&market_id)
    }

    /// Process an incoming order through the matching engine
    ///
    /// Returns the (possibly partially filled) order and list of fills
    pub fn process_order<F>(
        &self,
        mut order: Order,
        book: &mut OrderBook,
        get_position: F,
    ) -> Result<(Order, Vec<Fill>)>
    where
        F: Fn(AccountAddress) -> Option<Position>,
    {
        let mut fills = Vec::new();
        let timestamp = order.timestamp;

        // Match against opposite side of book
        match order.side {
            OrderSide::Buy => {
                Self::match_against_asks(&mut order, book, &mut fills, &get_position, timestamp)?;
            }
            OrderSide::Sell => {
                Self::match_against_bids(&mut order, book, &mut fills, &get_position, timestamp)?;
            }
        }

        // Handle unfilled portion based on order type
        Self::handle_unfilled(&mut order, book, &fills)?;

        Ok((order, fills))
    }

    /// Process an incoming order by market ID (convenience method for tests)
    ///
    /// This method avoids borrow conflicts by internally looking up the orderbook
    pub fn process_order_by_market<F>(
        &mut self,
        order: Order,
        market_id: MarketId,
        get_position: F,
    ) -> Result<(Order, Vec<Fill>)>
    where
        F: Fn(AccountAddress) -> Option<Position>,
    {
        let book = self
            .orderbooks
            .get_mut(&market_id)
            .ok_or(Error::MarketNotFound(market_id))?;

        let mut fills = Vec::new();
        let timestamp = order.timestamp;
        let mut order = order;

        // Match against opposite side of book
        match order.side {
            OrderSide::Buy => {
                Self::match_against_asks(&mut order, book, &mut fills, &get_position, timestamp)?;
            }
            OrderSide::Sell => {
                Self::match_against_bids(&mut order, book, &mut fills, &get_position, timestamp)?;
            }
        }

        // Handle unfilled portion based on order type
        Self::handle_unfilled(&mut order, book, &fills)?;

        Ok((order, fills))
    }

    /// Match a buy order against asks (static method)
    fn match_against_asks<F>(
        order: &mut Order,
        book: &mut OrderBook,
        fills: &mut Vec<Fill>,
        _get_position: &F,
        timestamp: Timestamp,
    ) -> Result<()>
    where
        F: Fn(AccountAddress) -> Option<Position>,
    {
        while !order.remaining_size.is_zero() {
            // Get best ask
            let best_ask = match book.best_ask() {
                Some(ask) => ask.clone(),
                None => break,
            };

            // Check if prices cross
            if best_ask.price > order.price {
                break;
            }

            // Self-trade prevention: cancel resting order
            if best_ask.owner == order.owner {
                book.remove(best_ask.id);
                // Emit cancel event (in production)
                continue;
            }

            // Calculate fill
            let fill_size = order.remaining_size.min(best_ask.remaining_size);
            let fill_price = best_ask.price; // Taker gets price improvement

            // Create fill
            let fill = Fill {
                market_id: order.market_id,
                maker: best_ask.owner,
                taker: order.owner,
                maker_order_id: best_ask.id,
                taker_order_id: order.id,
                price: fill_price.raw() as RawPrice,
                size: fill_size.raw() as RawAmount,
                maker_fee: 0, // Calculated by engine
                taker_fee: 0, // Calculated by engine
                timestamp,
                is_taker_buy: true,
            };

            fills.push(fill);

            // Update order sizes
            order.fill(fill_size);

            // Update or remove resting order
            let resting_remaining = best_ask.remaining_size - fill_size;
            if resting_remaining.is_zero() {
                book.remove(best_ask.id);
            } else {
                if let Some(resting) = book.get_mut(best_ask.id) {
                    resting.fill(fill_size);
                }
            }
        }

        Ok(())
    }

    /// Match a sell order against bids (static method)
    fn match_against_bids<F>(
        order: &mut Order,
        book: &mut OrderBook,
        fills: &mut Vec<Fill>,
        _get_position: &F,
        timestamp: Timestamp,
    ) -> Result<()>
    where
        F: Fn(AccountAddress) -> Option<Position>,
    {
        while !order.remaining_size.is_zero() {
            // Get best bid
            let best_bid = match book.best_bid() {
                Some(bid) => bid.clone(),
                None => break,
            };

            // Check if prices cross
            if best_bid.price < order.price {
                break;
            }

            // Self-trade prevention: cancel resting order
            if best_bid.owner == order.owner {
                book.remove(best_bid.id);
                continue;
            }

            // Calculate fill
            let fill_size = order.remaining_size.min(best_bid.remaining_size);
            let fill_price = best_bid.price;

            // Create fill
            let fill = Fill {
                market_id: order.market_id,
                maker: best_bid.owner,
                taker: order.owner,
                maker_order_id: best_bid.id,
                taker_order_id: order.id,
                price: fill_price.raw() as RawPrice,
                size: fill_size.raw() as RawAmount,
                maker_fee: 0,
                taker_fee: 0,
                timestamp,
                is_taker_buy: false,
            };

            fills.push(fill);

            // Update order sizes
            order.fill(fill_size);

            // Update or remove resting order
            let resting_remaining = best_bid.remaining_size - fill_size;
            if resting_remaining.is_zero() {
                book.remove(best_bid.id);
            } else {
                if let Some(resting) = book.get_mut(best_bid.id) {
                    resting.fill(fill_size);
                }
            }
        }

        Ok(())
    }

    /// Handle unfilled portion of order (static method)
    fn handle_unfilled(
        order: &mut Order,
        book: &mut OrderBook,
        fills: &[Fill],
    ) -> Result<()> {
        if order.remaining_size.is_zero() {
            return Ok(());
        }

        // Check post-only
        if order.post_only && !fills.is_empty() {
            return Err(Error::PostOnlyWouldCross);
        }

        // Check FOK
        if order.is_fok() && !fills.is_empty() {
            return Err(Error::FokNotFilled);
        }

        // For limit GTC/ALO: add to book
        if order.should_rest() {
            book.insert(order.clone());
        } else {
            // IOC/Market: cancel remaining
            order.status = OrderStatus::Canceled;
        }

        Ok(())
    }

    /// Remove an order from the book
    pub fn remove_from_book(&mut self, market_id: MarketId, order: &Order) -> Result<()> {
        let book = self
            .orderbooks
            .get_mut(&market_id)
            .ok_or(Error::MarketNotFound(market_id))?;

        book.remove(order.id);
        Ok(())
    }

    /// Get next order ID
    pub fn next_order_id(&mut self) -> u64 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a matching operation
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub order: Order,
    pub fills: Vec<Fill>,
    pub canceled_orders: Vec<(u64, CancelReason)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use hypercore_primitives::{OrderRequest, OrderType};

    fn make_order(
        id: u64,
        owner: Address,
        side: OrderSide,
        price: &str,
        size: &str,
        timestamp: u64,
    ) -> Order {
        Order::new(
            id,
            owner,
            OrderRequest {
                market_id: 0,
                side,
                price: Decimal::price(price),
                size: Decimal::size(size),
                order_type: OrderType::default(),
                reduce_only: false,
                client_order_id: None,
            },
            timestamp,
        )
    }

    #[test]
    fn test_no_match() {
        let mut engine = MatchingEngine::new();
        engine.add_orderbook(0);

        // Setup: Add ask at 101
        {
            let book = engine.get_orderbook_mut(0).unwrap();
            let ask = make_order(1, Address::repeat_byte(1), OrderSide::Sell, "101", "1.0", 1000);
            book.insert(ask);
        }

        // Buy at 100 shouldn't match
        let buy = make_order(2, Address::repeat_byte(2), OrderSide::Buy, "100", "1.0", 1001);

        let (order, fills) = engine
            .process_order_by_market(buy, 0, |_| None)
            .unwrap();

        assert!(fills.is_empty());
        assert_eq!(order.remaining_size, Decimal::size("1.0"));
    }

    #[test]
    fn test_full_match() {
        let mut engine = MatchingEngine::new();
        engine.add_orderbook(0);

        // Setup: Add ask at 100
        {
            let book = engine.get_orderbook_mut(0).unwrap();
            let ask = make_order(1, Address::repeat_byte(1), OrderSide::Sell, "100", "1.0", 1000);
            book.insert(ask);
        }

        // Buy at 100 should match
        let buy = make_order(2, Address::repeat_byte(2), OrderSide::Buy, "100", "1.0", 1001);

        let (order, fills) = engine
            .process_order_by_market(buy, 0, |_| None)
            .unwrap();

        assert_eq!(fills.len(), 1);
        assert!(order.is_filled());
        assert_eq!(fills[0].price, Decimal::price("100").raw() as u128);
        assert_eq!(fills[0].size, Decimal::size("1.0").raw() as u128);
    }

    #[test]
    fn test_partial_match() {
        let mut engine = MatchingEngine::new();
        engine.add_orderbook(0);

        // Setup: Add ask for 0.5
        {
            let book = engine.get_orderbook_mut(0).unwrap();
            let ask = make_order(1, Address::repeat_byte(1), OrderSide::Sell, "100", "0.5", 1000);
            book.insert(ask);
        }

        // Buy for 1.0 should partially fill
        let buy = make_order(2, Address::repeat_byte(2), OrderSide::Buy, "100", "1.0", 1001);

        let (order, fills) = engine
            .process_order_by_market(buy, 0, |_| None)
            .unwrap();

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].size, Decimal::size("0.5").raw() as u128);
        assert_eq!(order.remaining_size, Decimal::size("0.5"));
    }

    #[test]
    fn test_price_improvement() {
        let mut engine = MatchingEngine::new();
        engine.add_orderbook(0);

        // Setup: Add ask at 99 (better price)
        {
            let book = engine.get_orderbook_mut(0).unwrap();
            let ask = make_order(1, Address::repeat_byte(1), OrderSide::Sell, "99", "1.0", 1000);
            book.insert(ask);
        }

        // Buy at 100 should get filled at 99
        let buy = make_order(2, Address::repeat_byte(2), OrderSide::Buy, "100", "1.0", 1001);

        let (_, fills) = engine
            .process_order_by_market(buy, 0, |_| None)
            .unwrap();

        assert_eq!(fills[0].price, Decimal::price("99").raw() as u128);
    }

    #[test]
    fn test_self_trade_prevention() {
        let mut engine = MatchingEngine::new();
        engine.add_orderbook(0);

        let same_owner = Address::repeat_byte(1);

        // Setup: Add asks
        {
            let book = engine.get_orderbook_mut(0).unwrap();
            // Add ask from owner
            let ask = make_order(1, same_owner, OrderSide::Sell, "100", "1.0", 1000);
            book.insert(ask);

            // Add another ask from different owner
            let ask2 = make_order(2, Address::repeat_byte(2), OrderSide::Sell, "101", "1.0", 1001);
            book.insert(ask2);
        }

        // Buy from same owner should skip own order
        let buy = make_order(3, same_owner, OrderSide::Buy, "101", "1.0", 1002);

        let (_order, fills) = engine
            .process_order_by_market(buy, 0, |_| None)
            .unwrap();

        // Should match with order 2, not order 1
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].maker_order_id, 2);
        assert_eq!(fills[0].price, Decimal::price("101").raw() as u128);
    }

    #[test]
    fn test_multiple_fills() {
        let mut engine = MatchingEngine::new();
        engine.add_orderbook(0);

        // Setup: Add multiple asks
        {
            let book = engine.get_orderbook_mut(0).unwrap();
            book.insert(make_order(1, Address::repeat_byte(1), OrderSide::Sell, "100", "0.3", 1000));
            book.insert(make_order(2, Address::repeat_byte(2), OrderSide::Sell, "100", "0.3", 1001));
            book.insert(make_order(3, Address::repeat_byte(3), OrderSide::Sell, "101", "0.5", 1002));
        }

        // Large buy should eat through multiple levels
        let buy = make_order(4, Address::repeat_byte(4), OrderSide::Buy, "101", "1.0", 1003);

        let (order, fills) = engine
            .process_order_by_market(buy, 0, |_| None)
            .unwrap();

        assert_eq!(fills.len(), 3);
        assert!(order.is_filled());

        // Verify FIFO at same price
        assert_eq!(fills[0].maker_order_id, 1);
        assert_eq!(fills[1].maker_order_id, 2);
        assert_eq!(fills[2].maker_order_id, 3);
    }
}
