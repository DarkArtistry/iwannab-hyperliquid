//! Engine state management

use crate::orderbook::OrderBook;
use hypercore_primitives::{
    AccountAddress, AccountState, Decimal, Fill, FundingPayment, MarginMode, Market, MarketId,
    MarketState, Order, OrderId, Position, SignedAmount, Timestamp, TriggerDirection,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Block metadata for history queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetadata {
    pub hash: [u8; 32],
    pub timestamp: u64,
    pub tx_count: u32,
    pub events: Vec<serde_json::Value>,
}

/// Engine state containing all runtime data
pub struct EngineState {
    /// Account states
    pub accounts: HashMap<AccountAddress, AccountState>,
    /// Positions per account per market
    positions: HashMap<AccountAddress, HashMap<MarketId, Position>>,
    /// Leverage settings per account per market
    leverage: HashMap<AccountAddress, HashMap<MarketId, u8>>,
    /// Markets configuration and state
    markets: HashMap<MarketId, Market>,
    /// Order books per market
    orderbooks: HashMap<MarketId, OrderBook>,
    /// Orders indexed by market and order ID
    orders: HashMap<MarketId, HashMap<OrderId, Order>>,
    /// Orders indexed by account
    account_orders: HashMap<AccountAddress, HashMap<MarketId, Vec<OrderId>>>,
    /// Insurance fund balance
    pub insurance_fund: SignedAmount,
    /// Next order ID
    next_order_id: OrderId,
    /// Fill history per account (recent fills)
    account_fills: HashMap<AccountAddress, Vec<Fill>>,
    /// Recent trades per market (for public trade feed)
    recent_trades: HashMap<MarketId, Vec<Fill>>,
    /// Funding payments per account
    funding_history: HashMap<AccountAddress, Vec<FundingPayment>>,
    /// Funding rate history per market
    market_funding_history: HashMap<MarketId, Vec<(Timestamp, SignedAmount)>>,
    /// Margin mode per account
    margin_mode: HashMap<AccountAddress, MarginMode>,
    /// Trigger orders with direction Above: market_id -> BTreeMap<trigger_price_raw, Vec<Order>>
    trigger_orders_above: HashMap<MarketId, BTreeMap<i128, Vec<Order>>>,
    /// Trigger orders with direction Below: market_id -> BTreeMap<trigger_price_raw, Vec<Order>>
    trigger_orders_below: HashMap<MarketId, BTreeMap<i128, Vec<Order>>>,
    /// Index for trigger order lookup: order_id -> (market_id, direction, trigger_price_raw)
    trigger_order_index: HashMap<OrderId, (MarketId, TriggerDirection, i128)>,
    /// Current block height
    block_height: u64,
    /// Block metadata (hash, timestamp, tx_count)
    block_metadata: HashMap<u64, BlockMetadata>,
}

impl EngineState {
    /// Create a new engine state
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            positions: HashMap::new(),
            leverage: HashMap::new(),
            markets: HashMap::new(),
            orderbooks: HashMap::new(),
            orders: HashMap::new(),
            account_orders: HashMap::new(),
            insurance_fund: 0,
            next_order_id: 1,
            account_fills: HashMap::new(),
            recent_trades: HashMap::new(),
            funding_history: HashMap::new(),
            market_funding_history: HashMap::new(),
            margin_mode: HashMap::new(),
            trigger_orders_above: HashMap::new(),
            trigger_orders_below: HashMap::new(),
            trigger_order_index: HashMap::new(),
            block_height: 0,
            block_metadata: HashMap::new(),
        }
    }

    // Account operations

    /// Get account state
    pub fn get_account(&self, address: AccountAddress) -> Option<&AccountState> {
        self.accounts.get(&address)
    }

    /// Get mutable account state
    pub fn get_account_mut(&mut self, address: AccountAddress) -> Option<&mut AccountState> {
        self.accounts.get_mut(&address)
    }

    /// Get or create account state
    pub fn get_or_create_account(&mut self, address: AccountAddress) -> &AccountState {
        self.accounts.entry(address).or_insert_with(AccountState::default)
    }

    /// Get mutable or create account state
    pub fn get_or_create_account_mut(&mut self, address: AccountAddress) -> &mut AccountState {
        self.accounts.entry(address).or_insert_with(AccountState::default)
    }

    /// Deposit funds to an account
    pub fn deposit(&mut self, address: AccountAddress, amount: u128) {
        let account = self.get_or_create_account_mut(address);
        account.balance += amount as SignedAmount;
    }

    /// Withdraw funds from an account
    pub fn withdraw(&mut self, address: AccountAddress, amount: u128) -> bool {
        if let Some(account) = self.accounts.get_mut(&address) {
            if account.balance >= amount as SignedAmount {
                account.balance -= amount as SignedAmount;
                return true;
            }
        }
        false
    }

    // Position operations

    /// Get position for account in market
    pub fn get_position(&self, address: AccountAddress, market_id: MarketId) -> Option<&Position> {
        self.positions.get(&address)?.get(&market_id)
    }

    /// Get mutable position
    pub fn get_position_mut(
        &mut self,
        address: AccountAddress,
        market_id: MarketId,
    ) -> Option<&mut Position> {
        self.positions.get_mut(&address)?.get_mut(&market_id)
    }

    /// Get or create position
    pub fn get_or_create_position(
        &mut self,
        address: AccountAddress,
        market_id: MarketId,
    ) -> &Position {
        self.positions
            .entry(address)
            .or_default()
            .entry(market_id)
            .or_insert_with(Position::new)
    }

    /// Get all positions for an account
    pub fn get_all_positions(&self, address: AccountAddress) -> Vec<(MarketId, Position)> {
        self.positions
            .get(&address)
            .map(|positions| {
                positions
                    .iter()
                    .map(|(id, pos)| (*id, pos.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    // Leverage operations

    /// Get leverage for account in market
    pub fn get_leverage(&self, address: AccountAddress, market_id: MarketId) -> u8 {
        self.leverage
            .get(&address)
            .and_then(|m| m.get(&market_id))
            .copied()
            .unwrap_or(10) // Default 10x
    }

    /// Set leverage for account in market
    pub fn set_leverage(&mut self, address: AccountAddress, market_id: MarketId, leverage: u8) {
        self.leverage
            .entry(address)
            .or_default()
            .insert(market_id, leverage);
    }

    // Margin mode operations

    /// Get margin mode for account
    pub fn get_margin_mode(&self, address: AccountAddress) -> MarginMode {
        self.margin_mode.get(&address).copied().unwrap_or_default()
    }

    /// Set margin mode for account
    pub fn set_margin_mode(&mut self, address: AccountAddress, mode: MarginMode) {
        self.margin_mode.insert(address, mode);
    }

    // Market operations

    /// Add a market
    pub fn add_market(&mut self, market: Market) {
        let id = market.id();
        self.markets.insert(id, market);
        self.orderbooks.insert(id, OrderBook::new());
        self.orders.insert(id, HashMap::new());
    }

    /// Get market
    pub fn get_market(&self, market_id: MarketId) -> Option<&Market> {
        self.markets.get(&market_id)
    }

    /// Get mutable market
    pub fn get_market_mut(&mut self, market_id: MarketId) -> Option<&mut Market> {
        self.markets.get_mut(&market_id)
    }

    /// Get all markets
    pub fn get_all_markets(&self) -> Vec<&Market> {
        self.markets.values().collect()
    }

    // Orderbook operations

    /// Get orderbook
    pub fn get_orderbook(&self, market_id: MarketId) -> Option<&OrderBook> {
        self.orderbooks.get(&market_id)
    }

    /// Get mutable orderbook
    pub fn get_orderbook_mut(&mut self, market_id: MarketId) -> Option<&mut OrderBook> {
        self.orderbooks.get_mut(&market_id)
    }

    // Order operations

    /// Get next order ID
    pub fn next_order_id(&mut self) -> OrderId {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }

    /// Add an order
    pub fn add_order(&mut self, order: Order) {
        let market_id = order.market_id;
        let order_id = order.id;
        let owner = order.owner;

        // Add to market orders
        if let Some(market_orders) = self.orders.get_mut(&market_id) {
            market_orders.insert(order_id, order.clone());
        }

        // Add to orderbook
        if let Some(book) = self.orderbooks.get_mut(&market_id) {
            book.insert(order);
        }

        // Add to account orders index
        self.account_orders
            .entry(owner)
            .or_default()
            .entry(market_id)
            .or_default()
            .push(order_id);
    }

    /// Get an order
    pub fn get_order(&self, market_id: MarketId, order_id: OrderId) -> Option<&Order> {
        self.orders.get(&market_id)?.get(&order_id)
    }

    /// Remove an order
    pub fn remove_order(&mut self, market_id: MarketId, order_id: OrderId) -> Option<Order> {
        // Remove from market orders
        let order = self.orders.get_mut(&market_id)?.remove(&order_id)?;

        // Remove from orderbook
        if let Some(book) = self.orderbooks.get_mut(&market_id) {
            book.remove(order_id);
        }

        // Remove from account orders index
        if let Some(account_markets) = self.account_orders.get_mut(&order.owner) {
            if let Some(market_orders) = account_markets.get_mut(&market_id) {
                market_orders.retain(|&id| id != order_id);
            }
        }

        Some(order)
    }

    /// Get orders by account in a market
    pub fn get_orders_by_account(
        &self,
        address: AccountAddress,
        market_id: MarketId,
    ) -> Vec<Order> {
        let order_ids = self
            .account_orders
            .get(&address)
            .and_then(|m| m.get(&market_id))
            .cloned()
            .unwrap_or_default();

        order_ids
            .iter()
            .filter_map(|id| self.get_order(market_id, *id).cloned())
            .collect()
    }

    /// Update an order's remaining size after partial fill
    ///
    /// Called after matching to sync the orders HashMap with the orderbook BTreeMap.
    pub fn update_order_remaining(&mut self, market_id: MarketId, order_id: OrderId, new_remaining: Decimal) {
        if let Some(order) = self.orders.get_mut(&market_id).and_then(|m| m.get_mut(&order_id)) {
            order.remaining_size = new_remaining;
            order.status = hypercore_primitives::OrderStatus::PartiallyFilled;
        }
    }

    /// Get order count for account in market
    pub fn get_order_count(&self, address: AccountAddress, market_id: MarketId) -> usize {
        self.account_orders
            .get(&address)
            .and_then(|m| m.get(&market_id))
            .map(|orders| orders.len())
            .unwrap_or(0)
    }

    // Insurance fund operations

    /// Add to insurance fund
    pub fn add_to_insurance_fund(&mut self, amount: SignedAmount) {
        self.insurance_fund += amount;
    }

    /// Use insurance fund
    pub fn use_insurance_fund(&mut self, amount: SignedAmount) -> bool {
        if self.insurance_fund >= amount {
            self.insurance_fund -= amount;
            true
        } else {
            false
        }
    }

    // Trigger order operations

    /// Store a trigger order (TP/SL) for later activation
    pub fn add_trigger_order(&mut self, order: Order) {
        let trigger_price = match order.trigger_price {
            Some(tp) => tp,
            None => return,
        };
        let direction = match order.trigger_direction {
            Some(d) => d,
            None => return,
        };

        let market_id = order.market_id;
        let order_id = order.id;
        let price_raw = trigger_price.raw();

        match direction {
            TriggerDirection::Above => {
                self.trigger_orders_above
                    .entry(market_id)
                    .or_default()
                    .entry(price_raw)
                    .or_default()
                    .push(order);
            }
            TriggerDirection::Below => {
                self.trigger_orders_below
                    .entry(market_id)
                    .or_default()
                    .entry(price_raw)
                    .or_default()
                    .push(order);
            }
        }

        self.trigger_order_index
            .insert(order_id, (market_id, direction, price_raw));
    }

    /// Remove a trigger order by ID
    pub fn remove_trigger_order(&mut self, order_id: OrderId) -> Option<Order> {
        let (market_id, direction, price_raw) = self.trigger_order_index.remove(&order_id)?;

        let map = match direction {
            TriggerDirection::Above => self.trigger_orders_above.get_mut(&market_id)?,
            TriggerDirection::Below => self.trigger_orders_below.get_mut(&market_id)?,
        };

        let orders = map.get_mut(&price_raw)?;
        let idx = orders.iter().position(|o| o.id == order_id)?;
        let order = orders.remove(idx);

        if orders.is_empty() {
            map.remove(&price_raw);
        }

        Some(order)
    }

    /// Get and remove all trigger orders with direction Above where trigger_price <= mark_price
    pub fn get_triggered_orders_above(
        &mut self,
        market_id: MarketId,
        mark_price: Decimal,
    ) -> Vec<Order> {
        let mut result = Vec::new();
        let mark_raw = mark_price.raw();

        if let Some(map) = self.trigger_orders_above.get_mut(&market_id) {
            let triggered_keys: Vec<i128> = map.range(..=mark_raw).map(|(k, _)| *k).collect();

            for key in triggered_keys {
                if let Some(orders) = map.remove(&key) {
                    for order in &orders {
                        self.trigger_order_index.remove(&order.id);
                    }
                    result.extend(orders);
                }
            }
        }

        result
    }

    /// Get and remove all trigger orders with direction Below where trigger_price >= mark_price
    pub fn get_triggered_orders_below(
        &mut self,
        market_id: MarketId,
        mark_price: Decimal,
    ) -> Vec<Order> {
        let mut result = Vec::new();
        let mark_raw = mark_price.raw();

        if let Some(map) = self.trigger_orders_below.get_mut(&market_id) {
            let triggered_keys: Vec<i128> = map.range(mark_raw..).map(|(k, _)| *k).collect();

            for key in triggered_keys {
                if let Some(orders) = map.remove(&key) {
                    for order in &orders {
                        self.trigger_order_index.remove(&order.id);
                    }
                    result.extend(orders);
                }
            }
        }

        result
    }

    /// Get all trigger orders for a specific user
    pub fn get_user_trigger_orders(&self, address: AccountAddress) -> Vec<&Order> {
        let mut result = Vec::new();

        for map in self.trigger_orders_above.values() {
            for orders in map.values() {
                for order in orders {
                    if order.owner == address {
                        result.push(order);
                    }
                }
            }
        }

        for map in self.trigger_orders_below.values() {
            for orders in map.values() {
                for order in orders {
                    if order.owner == address {
                        result.push(order);
                    }
                }
            }
        }

        result
    }

    /// Cancel all trigger orders for an account in a specific market
    pub fn cancel_trigger_orders_for_position(
        &mut self,
        account: AccountAddress,
        market_id: MarketId,
    ) {
        let mut to_remove = Vec::new();

        if let Some(map) = self.trigger_orders_above.get(&market_id) {
            for orders in map.values() {
                for order in orders {
                    if order.owner == account {
                        to_remove.push(order.id);
                    }
                }
            }
        }

        if let Some(map) = self.trigger_orders_below.get(&market_id) {
            for orders in map.values() {
                for order in orders {
                    if order.owner == account {
                        to_remove.push(order.id);
                    }
                }
            }
        }

        for order_id in to_remove {
            self.remove_trigger_order(order_id);
        }
    }

    // Additional helper methods

    /// Check if market exists
    pub fn has_market(&self, market_id: MarketId) -> bool {
        self.markets.contains_key(&market_id)
    }

    /// Get market state (alias for getting market's state field)
    pub fn get_market_state(&self, market_id: MarketId) -> Option<&MarketState> {
        self.markets.get(&market_id).map(|m| &m.state)
    }

    /// Get account balance as Decimal
    pub fn get_balance(&self, address: AccountAddress) -> Decimal {
        let raw = self.accounts.get(&address).map(|a| a.balance).unwrap_or(0);
        Decimal::from_raw(raw, Decimal::USDC_DECIMALS)
    }

    /// Current block height
    pub fn current_height(&self) -> u64 {
        self.block_height
    }

    /// Set current block height
    pub fn set_block_height(&mut self, height: u64) {
        self.block_height = height;
    }

    /// Get block hash
    pub fn get_block_hash(&self, height: u64) -> Option<[u8; 32]> {
        self.block_metadata.get(&height).map(|m| m.hash)
    }

    /// Get block timestamp
    pub fn get_block_timestamp(&self, height: u64) -> Option<u64> {
        self.block_metadata.get(&height).map(|m| m.timestamp)
    }

    /// Get block transaction count
    pub fn get_block_tx_count(&self, height: u64) -> Option<u32> {
        self.block_metadata.get(&height).map(|m| m.tx_count)
    }

    /// Get block events
    pub fn get_block_events(&self, height: u64) -> Vec<serde_json::Value> {
        self.block_metadata
            .get(&height)
            .map(|m| m.events.clone())
            .unwrap_or_default()
    }

    /// Store block metadata
    pub fn store_block_metadata(&mut self, height: u64, metadata: BlockMetadata) {
        self.block_metadata.insert(height, metadata);
        self.block_height = height;
    }

    // Fill and trade history methods

    /// Record a fill for both maker and taker
    pub fn record_fill(&mut self, fill: Fill) {
        const MAX_FILLS_PER_USER: usize = 1000;
        const MAX_TRADES_PER_MARKET: usize = 500;

        // Record for maker
        let maker_fills = self.account_fills.entry(fill.maker).or_insert_with(Vec::new);
        maker_fills.push(fill.clone());
        if maker_fills.len() > MAX_FILLS_PER_USER {
            maker_fills.remove(0);
        }

        // Record for taker
        let taker_fills = self.account_fills.entry(fill.taker).or_insert_with(Vec::new);
        taker_fills.push(fill.clone());
        if taker_fills.len() > MAX_FILLS_PER_USER {
            taker_fills.remove(0);
        }

        // Record in recent trades
        let trades = self.recent_trades.entry(fill.market_id).or_insert_with(Vec::new);
        trades.push(fill);
        if trades.len() > MAX_TRADES_PER_MARKET {
            trades.remove(0);
        }
    }

    /// Get fills for a user
    pub fn get_user_fills(&self, address: AccountAddress, limit: Option<usize>) -> Vec<&Fill> {
        self.account_fills
            .get(&address)
            .map(|fills| {
                let limit = limit.unwrap_or(100).min(fills.len());
                fills.iter().rev().take(limit).collect()
            })
            .unwrap_or_default()
    }

    /// Get recent trades for a market
    pub fn get_recent_trades(&self, market_id: MarketId, limit: Option<usize>) -> Vec<&Fill> {
        self.recent_trades
            .get(&market_id)
            .map(|trades| {
                let limit = limit.unwrap_or(50).min(trades.len());
                trades.iter().rev().take(limit).collect()
            })
            .unwrap_or_default()
    }

    // Funding history methods

    /// Record a funding payment
    pub fn record_funding_payment(&mut self, payment: FundingPayment) {
        const MAX_FUNDING_PER_USER: usize = 500;

        let history = self.funding_history.entry(payment.account).or_insert_with(Vec::new);
        history.push(payment);
        if history.len() > MAX_FUNDING_PER_USER {
            history.remove(0);
        }
    }

    /// Record market funding rate
    pub fn record_market_funding(&mut self, market_id: MarketId, timestamp: Timestamp, rate: SignedAmount) {
        const MAX_FUNDING_PER_MARKET: usize = 1000;

        let history = self.market_funding_history.entry(market_id).or_insert_with(Vec::new);
        history.push((timestamp, rate));
        if history.len() > MAX_FUNDING_PER_MARKET {
            history.remove(0);
        }
    }

    /// Get funding history for a user
    pub fn get_user_funding_history(&self, address: AccountAddress, limit: Option<usize>) -> Vec<&FundingPayment> {
        self.funding_history
            .get(&address)
            .map(|history| {
                let limit = limit.unwrap_or(100).min(history.len());
                history.iter().rev().take(limit).collect()
            })
            .unwrap_or_default()
    }

    /// Get funding history for a market
    pub fn get_market_funding_history(&self, market_id: MarketId, limit: Option<usize>) -> Vec<(Timestamp, SignedAmount)> {
        self.market_funding_history
            .get(&market_id)
            .map(|history| {
                let limit = limit.unwrap_or(100).min(history.len());
                history.iter().rev().take(limit).cloned().collect()
            })
            .unwrap_or_default()
    }

    /// Get all open orders for a user across all markets
    pub fn get_all_user_orders(&self, address: AccountAddress) -> Vec<&Order> {
        self.account_orders
            .get(&address)
            .map(|market_orders| {
                market_orders
                    .iter()
                    .flat_map(|(market_id, order_ids)| {
                        order_ids.iter().filter_map(|id| self.get_order(*market_id, *id))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get list of all market IDs (sorted for determinism)
    pub fn get_market_ids(&self) -> Vec<MarketId> {
        let mut ids: Vec<_> = self.markets.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Look up a market ID by its symbol name (e.g., "BTC-PERP")
    pub fn get_market_id_by_name(&self, symbol: &str) -> Option<MarketId> {
        self.markets.iter()
            .find(|(_, market)| market.config.symbol == symbol)
            .map(|(id, _)| *id)
    }

    /// Update mark price using EWMA (Exponential Weighted Moving Average)
    ///
    /// Instead of naively setting mark_price = last_trade_price, we use:
    ///   new_mark = decay * old_mark + (1 - decay) * trade_price
    ///
    /// This smooths out individual trade noise and provides a more stable
    /// mark price for funding rate calculations and liquidation checks.
    pub fn update_mark_price(&mut self, market_id: MarketId, trade_price: Decimal) {
        if let Some(market) = self.markets.get_mut(&market_id) {
            let old_mark = market.state.mark_price;
            if old_mark.is_zero() {
                // First trade — initialize directly
                market.state.mark_price = trade_price;
            } else {
                // EWMA: new = 0.9 * old + 0.1 * trade
                // Both prices must be in the same decimal scale
                let trade = trade_price.to_decimals(old_mark.decimals());
                let decay_num = Decimal::from_raw(9, 1); // 0.9
                let complement_num = Decimal::from_raw(1, 1); // 0.1
                let smoothed = old_mark * decay_num + trade * complement_num;
                market.state.mark_price = smoothed;
            }
        }
    }

    /// Compute impact mid price for a market
    ///
    /// Walks the orderbook on both sides up to `impact_notional` worth of depth,
    /// then returns median(impact_bid, impact_ask, current_mark_price).
    /// This is used for more robust mark price determination.
    pub fn compute_impact_mid(&self, market_id: MarketId, impact_notional: Decimal) -> Option<Decimal> {
        let book = self.orderbooks.get(&market_id)?;
        let market = self.markets.get(&market_id)?;

        let impact_bid = book.impact_price_buy(impact_notional)?;
        let impact_ask = book.impact_price_sell(impact_notional)?;

        let impact_mid = (impact_bid + impact_ask).div_int(2);
        let mark = market.state.mark_price;

        // median of three values
        let median = if (impact_mid >= impact_bid && impact_mid <= mark)
            || (impact_mid <= impact_bid && impact_mid >= mark)
        {
            impact_mid
        } else if (mark >= impact_mid && mark <= impact_bid)
            || (mark <= impact_mid && mark >= impact_bid)
        {
            mark
        } else {
            impact_bid
        };

        Some(median)
    }

    /// Get mid prices for all markets that have both a bid and an ask
    pub fn get_all_mid_prices(&self) -> Vec<(MarketId, Decimal)> {
        let mut result: Vec<_> = self.markets.iter()
            .filter_map(|(id, _)| {
                self.orderbooks.get(id)
                    .and_then(|book| book.mid_price())
                    .map(|mid| (*id, mid))
            })
            .collect();
        result.sort_by_key(|(id, _)| *id);
        result
    }

    // === Persistence helper methods ===

    /// Get all positions across all accounts (for persistence)
    pub fn get_all_positions_global(&self) -> &HashMap<AccountAddress, HashMap<MarketId, Position>> {
        &self.positions
    }

    /// Get all leverage settings across all accounts (for persistence)
    pub fn get_all_leverage_global(&self) -> &HashMap<AccountAddress, HashMap<MarketId, u8>> {
        &self.leverage
    }

    /// Get all orders across all markets (for Merkle tree computation)
    ///
    /// Returns a reference to the orders HashMap indexed by market_id then order_id.
    /// Used by AppState::compute_orders_root() for state commitment.
    pub fn get_all_orders_global(&self) -> &HashMap<MarketId, HashMap<OrderId, Order>> {
        &self.orders
    }

    /// Set a position directly (for state restore)
    pub fn set_position(&mut self, address: AccountAddress, market_id: MarketId, position: Position) {
        self.positions
            .entry(address)
            .or_default()
            .insert(market_id, position);
    }

    /// Get next order ID without incrementing (for persistence)
    pub fn peek_next_order_id(&self) -> OrderId {
        self.next_order_id
    }

    /// Set next order ID (for state restore)
    pub fn set_next_order_id(&mut self, id: OrderId) {
        self.next_order_id = id;
    }

    /// Get insurance fund as Decimal (for persistence)
    pub fn get_insurance_fund(&self) -> Decimal {
        Decimal::from_raw(self.insurance_fund, Decimal::USDC_DECIMALS)
    }

    /// Set insurance fund (for state restore)
    pub fn set_insurance_fund(&mut self, amount: Decimal) {
        self.insurance_fund = amount.raw();
    }

    /// Restore an order into ALL data structures (for state restore)
    ///
    /// Unlike `add_order()` which assumes the market's orders HashMap entry exists
    /// (created by `add_market()`), this method uses `or_default()` to be safe
    /// during restore when market setup order may vary.
    ///
    /// Populates:
    /// - `orders` HashMap (used by `get_all_orders_global()` → AppHash)
    /// - `orderbooks` BTreeMap (used for matching)
    /// - `account_orders` index (used for account queries)
    pub fn restore_order(&mut self, order: Order) {
        let market_id = order.market_id;
        let order_id = order.id;
        let owner = order.owner;

        // Add to orders HashMap (used by get_all_orders_global() → AppHash)
        self.orders.entry(market_id)
            .or_default()
            .insert(order_id, order.clone());

        // Add to orderbook BTreeMap (used for matching)
        if let Some(book) = self.orderbooks.get_mut(&market_id) {
            book.restore_order(order);
        }

        // Add to account_orders index (used for queries)
        self.account_orders
            .entry(owner)
            .or_default()
            .entry(market_id)
            .or_default()
            .push(order_id);
    }
}

impl Default for EngineState {
    fn default() -> Self {
        Self::new()
    }
}

/// State snapshot for persistence
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EngineStateSnapshot {
    pub accounts: Vec<(AccountAddress, AccountState)>,
    pub positions: Vec<(AccountAddress, MarketId, Position)>,
    pub leverage: Vec<(AccountAddress, MarketId, u8)>,
    pub markets: Vec<Market>,
    pub orders: Vec<Order>,
    pub insurance_fund: SignedAmount,
    pub next_order_id: OrderId,
    /// Margin mode per account (default: empty for backwards compatibility)
    #[serde(default)]
    pub margin_modes: Vec<(AccountAddress, MarginMode)>,
    /// Trigger orders (TP/SL) awaiting activation
    #[serde(default)]
    pub trigger_orders: Vec<Order>,
}

impl From<&EngineState> for EngineStateSnapshot {
    fn from(state: &EngineState) -> Self {
        let accounts: Vec<_> = state.accounts.iter().map(|(k, v)| (*k, v.clone())).collect();

        let positions: Vec<_> = state
            .positions
            .iter()
            .flat_map(|(addr, markets)| {
                markets.iter().map(move |(mid, pos)| (*addr, *mid, pos.clone()))
            })
            .collect();

        let leverage: Vec<_> = state
            .leverage
            .iter()
            .flat_map(|(addr, markets)| {
                markets.iter().map(move |(mid, lev)| (*addr, *mid, *lev))
            })
            .collect();

        let markets: Vec<_> = state.markets.values().cloned().collect();

        let orders: Vec<_> = state
            .orders
            .values()
            .flat_map(|m| m.values().cloned())
            .collect();

        let margin_modes: Vec<_> = state.margin_mode.iter().map(|(k, v)| (*k, *v)).collect();

        // Collect all trigger orders from both above and below maps
        let mut trigger_orders = Vec::new();
        for map in state.trigger_orders_above.values() {
            for orders in map.values() {
                trigger_orders.extend(orders.iter().cloned());
            }
        }
        for map in state.trigger_orders_below.values() {
            for orders in map.values() {
                trigger_orders.extend(orders.iter().cloned());
            }
        }

        Self {
            accounts,
            positions,
            leverage,
            markets,
            orders,
            insurance_fund: state.insurance_fund,
            next_order_id: state.next_order_id,
            margin_modes,
            trigger_orders,
        }
    }
}

impl EngineState {
    /// Create a snapshot of the current state for re-org handling
    ///
    /// This captures all consensus-critical state that needs to be reverted
    /// during a blockchain reorganization.
    pub fn snapshot(&self) -> EngineStateSnapshot {
        EngineStateSnapshot::from(self)
    }

    /// Restore state from a snapshot in-place
    ///
    /// This replaces all current state with the snapshot's state.
    /// Used during re-org to revert to a previous block's state.
    pub fn restore(&mut self, snapshot: EngineStateSnapshot) {
        // Clear existing state
        self.accounts.clear();
        self.positions.clear();
        self.leverage.clear();
        self.markets.clear();
        self.orderbooks.clear();
        self.orders.clear();
        self.account_orders.clear();
        self.margin_mode.clear();
        self.trigger_orders_above.clear();
        self.trigger_orders_below.clear();
        self.trigger_order_index.clear();

        // Restore from snapshot
        for (addr, account) in snapshot.accounts {
            self.accounts.insert(addr, account);
        }

        for (addr, market_id, position) in snapshot.positions {
            self.positions
                .entry(addr)
                .or_default()
                .insert(market_id, position);
        }

        for (addr, market_id, lev) in snapshot.leverage {
            self.leverage.entry(addr).or_default().insert(market_id, lev);
        }

        for market in snapshot.markets {
            self.add_market(market);
        }

        for order in snapshot.orders {
            self.add_order(order);
        }

        for (addr, mode) in snapshot.margin_modes {
            self.margin_mode.insert(addr, mode);
        }

        for trigger_order in snapshot.trigger_orders {
            self.add_trigger_order(trigger_order);
        }

        self.insurance_fund = snapshot.insurance_fund;
        self.next_order_id = snapshot.next_order_id;
    }

    /// Restore from snapshot (creates new instance)
    pub fn from_snapshot(snapshot: EngineStateSnapshot) -> Self {
        let mut state = Self::new();

        for (addr, account) in snapshot.accounts {
            state.accounts.insert(addr, account);
        }

        for (addr, market_id, position) in snapshot.positions {
            state
                .positions
                .entry(addr)
                .or_default()
                .insert(market_id, position);
        }

        for (addr, market_id, lev) in snapshot.leverage {
            state.leverage.entry(addr).or_default().insert(market_id, lev);
        }

        for market in snapshot.markets {
            state.add_market(market);
        }

        for order in snapshot.orders {
            state.add_order(order);
        }

        for (addr, mode) in snapshot.margin_modes {
            state.margin_mode.insert(addr, mode);
        }

        for trigger_order in snapshot.trigger_orders {
            state.add_trigger_order(trigger_order);
        }

        state.insurance_fund = snapshot.insurance_fund;
        state.next_order_id = snapshot.next_order_id;

        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use hypercore_primitives::{Decimal, MarketConfig};

    #[test]
    fn test_deposit_withdraw() {
        let mut state = EngineState::new();
        let account = Address::repeat_byte(1);

        state.deposit(account, 10000_000000);
        assert_eq!(state.get_account(account).unwrap().balance, 10000_000000);

        assert!(state.withdraw(account, 5000_000000));
        assert_eq!(state.get_account(account).unwrap().balance, 5000_000000);

        assert!(!state.withdraw(account, 10000_000000)); // Insufficient
        assert_eq!(state.get_account(account).unwrap().balance, 5000_000000);
    }

    #[test]
    fn test_position_management() {
        let mut state = EngineState::new();
        let account = Address::repeat_byte(1);

        // Create position
        let pos = state.get_or_create_position(account, 0);
        assert!(pos.is_empty());

        // Modify position
        if let Some(pos) = state.get_position_mut(account, 0) {
            pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);
        }

        // Verify
        let pos = state.get_position(account, 0).unwrap();
        assert!(pos.is_long());
    }

    #[test]
    fn test_leverage() {
        let mut state = EngineState::new();
        let account = Address::repeat_byte(1);

        // Default leverage
        assert_eq!(state.get_leverage(account, 0), 10);

        // Set leverage
        state.set_leverage(account, 0, 20);
        assert_eq!(state.get_leverage(account, 0), 20);

        // Other market still default
        assert_eq!(state.get_leverage(account, 1), 10);
    }

    #[test]
    fn test_market_operations() {
        let mut state = EngineState::new();

        let btc = Market::new(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        state.add_market(btc);

        let market = state.get_market(0).unwrap();
        assert_eq!(market.config.symbol, "BTC-PERP");
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let mut state = EngineState::new();
        let account = Address::repeat_byte(1);

        state.deposit(account, 10000_000000);
        state.set_leverage(account, 0, 20);

        let btc = Market::new(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        state.add_market(btc);

        let snapshot = EngineStateSnapshot::from(&state);
        let restored = EngineState::from_snapshot(snapshot);

        assert_eq!(restored.get_account(account).unwrap().balance, 10000_000000);
        assert_eq!(restored.get_leverage(account, 0), 20);
    }

    #[test]
    fn test_get_market_id_by_name() {
        let mut state = EngineState::new();

        let btc = Market::new(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        let eth = Market::new(MarketConfig::eth_perp(), Decimal::price("3500"), 0);
        state.add_market(btc);
        state.add_market(eth);

        assert_eq!(state.get_market_id_by_name("BTC-PERP"), Some(0));
        assert_eq!(state.get_market_id_by_name("ETH-PERP"), Some(1));
        assert_eq!(state.get_market_id_by_name("SOL-PERP"), None);
    }

    #[test]
    fn test_get_all_mid_prices() {
        use hypercore_primitives::{OrderRequest, OrderSide, OrderType};

        let mut state = EngineState::new();
        let account = Address::repeat_byte(1);

        let btc = Market::new(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        state.add_market(btc);

        // No orders — mid price should be empty
        assert!(state.get_all_mid_prices().is_empty());

        // Add a bid and an ask
        let bid = Order::new(
            state.next_order_id(),
            account,
            OrderRequest {
                market_id: 0,
                side: OrderSide::Buy,
                price: Decimal::price("64000"),
                size: Decimal::size("1.0"),
                order_type: OrderType::default(),
                reduce_only: false,
                client_order_id: None,
                trigger_price: None,
                trigger_direction: None,
            },
            1000,
        );
        state.add_order(bid);

        let ask = Order::new(
            state.next_order_id(),
            account,
            OrderRequest {
                market_id: 0,
                side: OrderSide::Sell,
                price: Decimal::price("66000"),
                size: Decimal::size("1.0"),
                order_type: OrderType::default(),
                reduce_only: false,
                client_order_id: None,
                trigger_price: None,
                trigger_direction: None,
            },
            1001,
        );
        state.add_order(ask);

        let mid_prices = state.get_all_mid_prices();
        assert_eq!(mid_prices.len(), 1);
        assert_eq!(mid_prices[0].0, 0); // market_id
        // Mid price = (64000 + 66000) / 2 = 65000
        assert_eq!(mid_prices[0].1, Decimal::price("65000"));
    }

    #[test]
    fn test_restore_order_populates_all_stores() {
        use hypercore_primitives::{OrderRequest, OrderSide, OrderType};

        let mut state = EngineState::new();
        let account = Address::repeat_byte(1);
        let market_id: MarketId = 0;

        // Add a market first (creates orderbook and orders HashMap entries)
        let btc = Market::new(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        state.add_market(btc);

        // Create an order to restore
        let order = Order::new(
            42,
            account,
            OrderRequest {
                market_id,
                side: OrderSide::Buy,
                price: Decimal::price("64000"),
                size: Decimal::size("1.0"),
                order_type: OrderType::default(),
                reduce_only: false,
                client_order_id: None,
                trigger_price: None,
                trigger_direction: None,
            },
            1000,
        );

        // Use restore_order (the method persistence uses)
        state.restore_order(order);

        // Verify orders HashMap is populated (used by get_all_orders_global → AppHash)
        let all_orders = state.get_all_orders_global();
        assert!(all_orders.contains_key(&market_id), "orders HashMap must contain market");
        assert!(all_orders[&market_id].contains_key(&42), "orders HashMap must contain order 42");

        // Verify orderbook is populated (used for matching)
        let orderbook = state.get_orderbook(market_id).unwrap();
        let book_orders = orderbook.get_all_orders();
        assert_eq!(book_orders.len(), 1, "orderbook must contain the restored order");

        // Verify account_orders index is populated (used for queries)
        let user_orders = state.get_orders_by_account(account, market_id);
        assert_eq!(user_orders.len(), 1, "account_orders must contain the restored order");
        assert_eq!(user_orders[0].id, 42);
    }
}
