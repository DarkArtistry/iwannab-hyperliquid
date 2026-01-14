//! Order book implementation with price-time priority

use hypercore_primitives::{Decimal, Error, Order, OrderId, OrderKey, OrderSide, Result};
use serde::Serialize;
use std::collections::BTreeMap;

/// Level in the order book (aggregated view)
#[derive(Debug, Clone, Default, Serialize)]
pub struct PriceLevel {
    pub price: Decimal,
    pub size: Decimal,
    pub order_count: u32,
}

/// L2 orderbook snapshot
#[derive(Debug, Clone, Default, Serialize)]
pub struct L2Snapshot {
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
}

impl L2Snapshot {
    pub fn new(bids: Vec<PriceLevel>, asks: Vec<PriceLevel>) -> Self {
        Self { bids, asks }
    }
}

/// Order book for a single market
#[derive(Debug)]
pub struct OrderBook {
    /// Bid orders (buy), sorted by price descending then time ascending
    bids: BTreeMap<OrderKey, Order>,
    /// Ask orders (sell), sorted by price ascending then time ascending
    asks: BTreeMap<OrderKey, Order>,
    /// Index for fast order lookup by ID
    order_index: std::collections::HashMap<OrderId, OrderKey>,
    /// Best bid price cache
    best_bid: Option<Decimal>,
    /// Best ask price cache
    best_ask: Option<Decimal>,
}

impl OrderBook {
    /// Create a new empty order book
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            order_index: std::collections::HashMap::new(),
            best_bid: None,
            best_ask: None,
        }
    }

    /// Insert an order into the book
    pub fn insert(&mut self, order: Order) {
        let key = match order.side {
            OrderSide::Buy => OrderKey::bid(order.price, order.timestamp, order.id),
            OrderSide::Sell => OrderKey::ask(order.price, order.timestamp, order.id),
        };

        self.order_index.insert(order.id, key);

        match order.side {
            OrderSide::Buy => {
                self.bids.insert(key, order);
                self.update_best_bid();
            }
            OrderSide::Sell => {
                self.asks.insert(key, order);
                self.update_best_ask();
            }
        }
    }

    /// Remove an order from the book
    pub fn remove(&mut self, order_id: OrderId) -> Option<Order> {
        let key = self.order_index.remove(&order_id)?;

        let order = if key.price_key < 0 {
            // Bid (negative price key)
            let order = self.bids.remove(&key);
            self.update_best_bid();
            order
        } else {
            // Ask (positive price key)
            let order = self.asks.remove(&key);
            self.update_best_ask();
            order
        };

        order
    }

    /// Get an order by ID
    pub fn get(&self, order_id: OrderId) -> Option<&Order> {
        let key = self.order_index.get(&order_id)?;
        if key.price_key < 0 {
            self.bids.get(key)
        } else {
            self.asks.get(key)
        }
    }

    /// Get mutable reference to an order
    pub fn get_mut(&mut self, order_id: OrderId) -> Option<&mut Order> {
        let key = self.order_index.get(&order_id)?.clone();
        if key.price_key < 0 {
            self.bids.get_mut(&key)
        } else {
            self.asks.get_mut(&key)
        }
    }

    /// Get the best bid (highest buy price)
    pub fn best_bid(&self) -> Option<&Order> {
        self.bids.values().next()
    }

    /// Get the best ask (lowest sell price)
    pub fn best_ask(&self) -> Option<&Order> {
        self.asks.values().next()
    }

    /// Get the best bid price
    pub fn best_bid_price(&self) -> Option<Decimal> {
        self.best_bid
    }

    /// Get the best ask price
    pub fn best_ask_price(&self) -> Option<Decimal> {
        self.best_ask
    }

    /// Get the spread
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Get the mid price
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask).div_int(2)),
            _ => None,
        }
    }

    /// Check if book is empty
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    /// Get number of orders
    pub fn len(&self) -> usize {
        self.bids.len() + self.asks.len()
    }

    /// Get number of bid orders
    pub fn bid_count(&self) -> usize {
        self.bids.len()
    }

    /// Get number of ask orders
    pub fn ask_count(&self) -> usize {
        self.asks.len()
    }

    /// Iterate over bids (best first)
    pub fn bids_iter(&self) -> impl Iterator<Item = &Order> {
        self.bids.values()
    }

    /// Iterate over asks (best first)
    pub fn asks_iter(&self) -> impl Iterator<Item = &Order> {
        self.asks.values()
    }

    /// Get L2 book (aggregated by price level)
    pub fn get_l2(&self, depth: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
        let bids = self.aggregate_levels(&self.bids, depth, true);
        let asks = self.aggregate_levels(&self.asks, depth, false);
        (bids, asks)
    }

    /// Get L2 book snapshot
    pub fn get_l2_snapshot(&self, depth: usize) -> L2Snapshot {
        let (bids, asks) = self.get_l2(depth);
        L2Snapshot::new(bids, asks)
    }

    /// Update an order's remaining size after partial fill
    pub fn update_size(&mut self, order_id: OrderId, new_remaining: Decimal) -> Result<()> {
        let order = self.get_mut(order_id).ok_or(Error::OrderNotFound(order_id))?;
        order.remaining_size = new_remaining;
        Ok(())
    }

    /// Remove an order if it's fully filled
    pub fn remove_if_filled(&mut self, order_id: OrderId) -> Option<Order> {
        if let Some(order) = self.get(order_id) {
            if order.is_filled() {
                return self.remove(order_id);
            }
        }
        None
    }

    // Internal helpers

    fn update_best_bid(&mut self) {
        self.best_bid = self.bids.values().next().map(|o| o.price);
    }

    fn update_best_ask(&mut self) {
        self.best_ask = self.asks.values().next().map(|o| o.price);
    }

    fn aggregate_levels(
        &self,
        orders: &BTreeMap<OrderKey, Order>,
        depth: usize,
        is_bid: bool,
    ) -> Vec<PriceLevel> {
        let mut levels: Vec<PriceLevel> = Vec::new();
        let mut current_price: Option<Decimal> = None;

        for order in orders.values() {
            if levels.len() >= depth && current_price != Some(order.price) {
                break;
            }

            match current_price {
                Some(p) if p == order.price => {
                    if let Some(level) = levels.last_mut() {
                        level.size = level.size + order.remaining_size;
                        level.order_count += 1;
                    }
                }
                _ => {
                    if levels.len() < depth {
                        levels.push(PriceLevel {
                            price: order.price,
                            size: order.remaining_size,
                            order_count: 1,
                        });
                        current_price = Some(order.price);
                    }
                }
            }
        }

        levels
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

/// Order book snapshot for serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderBookSnapshot {
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
}

impl From<&OrderBook> for OrderBookSnapshot {
    fn from(book: &OrderBook) -> Self {
        Self {
            bids: book.bids.values().cloned().collect(),
            asks: book.asks.values().cloned().collect(),
        }
    }
}

impl OrderBook {
    /// Restore from snapshot
    pub fn from_snapshot(snapshot: OrderBookSnapshot) -> Self {
        let mut book = Self::new();
        for order in snapshot.bids {
            book.insert(order);
        }
        for order in snapshot.asks {
            book.insert(order);
        }
        book
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use hypercore_primitives::{OrderRequest, OrderType};

    fn make_order(id: OrderId, side: OrderSide, price: &str, size: &str, timestamp: u64) -> Order {
        Order::new(
            id,
            Address::ZERO,
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
    fn test_orderbook_insert_and_retrieve() {
        let mut book = OrderBook::new();

        let order = make_order(1, OrderSide::Buy, "100", "1.0", 1000);
        book.insert(order);

        assert_eq!(book.len(), 1);
        assert!(book.get(1).is_some());
        assert_eq!(book.best_bid_price(), Some(Decimal::price("100")));
    }

    #[test]
    fn test_orderbook_price_priority() {
        let mut book = OrderBook::new();

        // Insert bids at different prices
        book.insert(make_order(1, OrderSide::Buy, "100", "1.0", 1000));
        book.insert(make_order(2, OrderSide::Buy, "101", "1.0", 1001));
        book.insert(make_order(3, OrderSide::Buy, "99", "1.0", 1002));

        // Best bid should be highest price
        assert_eq!(book.best_bid().unwrap().id, 2);
        assert_eq!(book.best_bid_price(), Some(Decimal::price("101")));
    }

    #[test]
    fn test_orderbook_time_priority() {
        let mut book = OrderBook::new();

        // Insert bids at same price, different times
        book.insert(make_order(1, OrderSide::Buy, "100", "1.0", 1002));
        book.insert(make_order(2, OrderSide::Buy, "100", "1.0", 1000)); // Earlier
        book.insert(make_order(3, OrderSide::Buy, "100", "1.0", 1001));

        // First order should be the earliest one
        assert_eq!(book.best_bid().unwrap().id, 2);
    }

    #[test]
    fn test_orderbook_remove() {
        let mut book = OrderBook::new();

        book.insert(make_order(1, OrderSide::Buy, "100", "1.0", 1000));
        book.insert(make_order(2, OrderSide::Buy, "101", "1.0", 1001));

        let removed = book.remove(2);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, 2);

        assert_eq!(book.len(), 1);
        assert_eq!(book.best_bid_price(), Some(Decimal::price("100")));
    }

    #[test]
    fn test_orderbook_l2() {
        let mut book = OrderBook::new();

        // Multiple orders at same price
        book.insert(make_order(1, OrderSide::Buy, "100", "1.0", 1000));
        book.insert(make_order(2, OrderSide::Buy, "100", "0.5", 1001));
        book.insert(make_order(3, OrderSide::Buy, "99", "2.0", 1002));

        let (bids, _asks) = book.get_l2(10);

        assert_eq!(bids.len(), 2);
        assert_eq!(bids[0].price, Decimal::price("100"));
        assert_eq!(bids[0].size, Decimal::size("1.5"));
        assert_eq!(bids[0].order_count, 2);
        assert_eq!(bids[1].price, Decimal::price("99"));
        assert_eq!(bids[1].size, Decimal::size("2.0"));
    }

    #[test]
    fn test_orderbook_spread() {
        let mut book = OrderBook::new();

        book.insert(make_order(1, OrderSide::Buy, "100", "1.0", 1000));
        book.insert(make_order(2, OrderSide::Sell, "101", "1.0", 1001));

        assert_eq!(book.spread(), Some(Decimal::price("1")));
        assert_eq!(book.mid_price(), Some(Decimal::price("100.5")));
    }
}
