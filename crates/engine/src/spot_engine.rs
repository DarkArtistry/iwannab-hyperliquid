//! Spot Trading Engine
//!
//! Implements HIP-1 style spot token trading with built-in orderbooks.
//!
//! ## Phase 2A: Unified State Integration
//!
//! This engine now uses the UnifiedState model for balance management:
//! - Balances are stored in UnifiedState with separate Core and EVM views
//! - SpotEngine reads from the `core_view` for trading
//! - Order reservations are tracked separately in `reserved` map
//!
//! Key differences from perpetuals:
//! - No leverage or margin
//! - Balance-based (not position-based)
//! - Simpler fee model (no funding, no liquidation)

use crate::orderbook::OrderBook;
use hypercore_primitives::{
    AccountAddress, Decimal, Error, Order, OrderId, OrderRequest, OrderSide, OrderStatus,
    Result, SpotBalance, SpotFill, SpotMarket, SpotMarketConfig, SpotMarketId, SpotToken,
    SpotTokenDeployParams, Timestamp, TokenIndex,
    SharedUnifiedState, new_shared_unified_state,
};
use std::collections::HashMap;

// Helper macro for invalid input errors
macro_rules! invalid_input {
    ($msg:expr) => {
        Error::Internal(format!("Invalid input: {}", $msg))
    };
}

/// Spot engine state
///
/// ## Unified State Model (Phase 2A)
///
/// Balance storage has been refactored to use UnifiedState:
/// - `unified_state`: Shared reference to the master balance sheet
/// - `reserved`: Local tracking of amounts reserved in open orders
///
/// The available balance for trading is: `unified_state.core_view - reserved`
pub struct SpotEngineState {
    /// Registered spot tokens
    tokens: HashMap<TokenIndex, SpotToken>,
    /// Spot markets (token/USDC pairs)
    markets: HashMap<SpotMarketId, SpotMarket>,
    /// Orderbooks for spot markets
    orderbooks: HashMap<SpotMarketId, OrderBook>,
    /// Reference to unified state (master balance sheet)
    unified_state: SharedUnifiedState,
    /// Reserved balances for open orders (tracked separately)
    /// This is order-specific state, not part of the unified balance
    reserved: HashMap<(AccountAddress, TokenIndex), Decimal>,
    /// Orders by market
    orders: HashMap<SpotMarketId, HashMap<OrderId, Order>>,
    /// Orders by account
    account_orders: HashMap<AccountAddress, HashMap<SpotMarketId, Vec<OrderId>>>,
    /// Next token index (starts at 1, 0 is reserved for USDC)
    next_token_index: TokenIndex,
    /// Next order ID
    next_order_id: OrderId,
    /// Next market ID (starts at 128 for spot markets)
    next_market_id: SpotMarketId,
}

impl SpotEngineState {
    /// Create new spot engine state with USDC token
    pub fn new() -> Self {
        Self::with_unified_state(new_shared_unified_state())
    }

    /// Create new spot engine state with shared unified state
    pub fn with_unified_state(unified_state: SharedUnifiedState) -> Self {
        let mut state = Self {
            tokens: HashMap::new(),
            markets: HashMap::new(),
            orderbooks: HashMap::new(),
            unified_state,
            reserved: HashMap::new(),
            orders: HashMap::new(),
            account_orders: HashMap::new(),
            next_token_index: 1,  // 0 is reserved for USDC
            next_order_id: 1_000_000, // Start spot orders at 1M to avoid conflicts with perp orders
            next_market_id: 128, // Spot markets start at 128
        };

        // Register USDC as the native quote token
        let usdc = SpotToken::usdc();
        state.tokens.insert(0, usdc);

        state
    }

    /// Get the shared unified state reference
    pub fn unified_state(&self) -> &SharedUnifiedState {
        &self.unified_state
    }

    // Token operations

    /// Deploy a new spot token (HIP-1 style)
    pub fn deploy_token(
        &mut self,
        params: SpotTokenDeployParams,
        deployer: AccountAddress,
        timestamp: Timestamp,
    ) -> Result<SpotToken> {
        // Validate parameters
        if params.symbol.len() > 6 {
            return Err(invalid_input!("Symbol must be <= 6 characters"));
        }
        if params.sz_decimals + 5 > params.wei_decimals {
            return Err(invalid_input!("szDecimals + 5 must be <= weiDecimals"));
        }

        // Allocate token index
        let token_index = self.next_token_index;
        self.next_token_index += 1;

        // Create token
        let mut token = SpotToken::new(
            token_index,
            params.name,
            params.symbol.clone(),
            params.wei_decimals,
            params.sz_decimals,
            params.max_supply,
            deployer,
        );
        token.created_at = timestamp;

        // Link to ERC20 if provided
        if let Some(erc20_addr) = params.erc20_address {
            token.link_erc20(erc20_addr);
        }

        // Process genesis allocations
        for (recipient, amount) in &params.genesis_allocations {
            self.credit_balance(*recipient, token_index, *amount);
        }

        // If no genesis allocations, give all to deployer
        if params.genesis_allocations.is_empty() {
            self.credit_balance(deployer, token_index, params.max_supply);
        }

        // Store token
        self.tokens.insert(token_index, token.clone());

        // Create spot market for this token (token/USDC pair)
        let market_id = self.next_market_id;
        self.next_market_id += 1;

        let market_config = SpotMarketConfig::new_usdc_pair(&token, market_id);
        let market = SpotMarket::new(market_config);
        self.markets.insert(market_id, market);
        self.orderbooks.insert(market_id, OrderBook::new());
        self.orders.insert(market_id, HashMap::new());

        Ok(token)
    }

    /// Get a token by index
    pub fn get_token(&self, index: TokenIndex) -> Option<&SpotToken> {
        self.tokens.get(&index)
    }

    /// Get all tokens
    pub fn get_all_tokens(&self) -> Vec<&SpotToken> {
        self.tokens.values().collect()
    }

    /// Find token by symbol
    pub fn find_token_by_symbol(&self, symbol: &str) -> Option<&SpotToken> {
        self.tokens.values().find(|t| t.symbol == symbol)
    }

    // Balance operations - Now using UnifiedState

    /// Get spot balance for account and token
    ///
    /// Returns a SpotBalance struct for backward compatibility:
    /// - `total`: from UnifiedState.core_view
    /// - `reserved`: from local reserved map
    pub fn get_balance(&self, account: AccountAddress, token_index: TokenIndex) -> SpotBalance {
        let unified = self.unified_state.read().unwrap();
        let core_view = unified.get_core_view(account, token_index);
        let reserved = self.reserved
            .get(&(account, token_index))
            .copied()
            .unwrap_or_else(|| Decimal::from_raw(0, core_view.decimals()));

        SpotBalance {
            total: core_view,
            reserved,
        }
    }

    /// Get all balances for an account
    pub fn get_all_balances(&self, account: AccountAddress) -> Vec<(TokenIndex, SpotBalance)> {
        let unified = self.unified_state.read().unwrap();
        unified.get_all_balances(account)
            .iter()
            .map(|(token_index, unified_balance)| {
                let reserved = self.reserved
                    .get(&(account, *token_index))
                    .copied()
                    .unwrap_or_else(|| Decimal::from_raw(0, unified_balance.core_view.decimals()));
                (*token_index, SpotBalance {
                    total: unified_balance.core_view,
                    reserved,
                })
            })
            .collect()
    }

    /// Credit balance to account (adds to unified state core_view)
    pub fn credit_balance(&mut self, account: AccountAddress, token_index: TokenIndex, amount: Decimal) {
        let mut unified = self.unified_state.write().unwrap();
        unified.credit(account, token_index, amount);
    }

    /// Debit balance from account (removes from unified state core_view)
    pub fn debit_balance(&mut self, account: AccountAddress, token_index: TokenIndex, amount: Decimal) -> bool {
        let mut unified = self.unified_state.write().unwrap();
        unified.debit_core(account, token_index, amount)
    }

    /// Reserve balance for an order
    ///
    /// This increases the reserved amount, reducing available balance.
    /// The total in unified state is unchanged.
    pub fn reserve_balance(&mut self, account: AccountAddress, token_index: TokenIndex, amount: Decimal) -> bool {
        // Check available balance (core_view - reserved)
        let balance = self.get_balance(account, token_index);
        if balance.available() < amount {
            return false;
        }

        // Increase reserved amount
        let reserved = self.reserved
            .entry((account, token_index))
            .or_insert_with(|| Decimal::from_raw(0, amount.decimals()));
        *reserved = *reserved + amount;
        true
    }

    /// Release reserved balance
    pub fn release_balance(&mut self, account: AccountAddress, token_index: TokenIndex, amount: Decimal) {
        if let Some(reserved) = self.reserved.get_mut(&(account, token_index)) {
            *reserved = *reserved - amount.min(*reserved);
        }
    }

    // Market operations

    /// Get spot market
    pub fn get_market(&self, market_id: SpotMarketId) -> Option<&SpotMarket> {
        self.markets.get(&market_id)
    }

    /// Get mutable spot market
    pub fn get_market_mut(&mut self, market_id: SpotMarketId) -> Option<&mut SpotMarket> {
        self.markets.get_mut(&market_id)
    }

    /// Get all spot markets
    pub fn get_all_markets(&self) -> Vec<&SpotMarket> {
        self.markets.values().collect()
    }

    /// Find market by symbol
    pub fn find_market_by_symbol(&self, symbol: &str) -> Option<&SpotMarket> {
        self.markets.values().find(|m| m.config.symbol == symbol)
    }

    /// Get orderbook
    pub fn get_orderbook(&self, market_id: SpotMarketId) -> Option<&OrderBook> {
        self.orderbooks.get(&market_id)
    }

    /// Get mutable orderbook
    pub fn get_orderbook_mut(&mut self, market_id: SpotMarketId) -> Option<&mut OrderBook> {
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
    pub fn get_order(&self, market_id: SpotMarketId, order_id: OrderId) -> Option<&Order> {
        self.orders.get(&market_id)?.get(&order_id)
    }

    /// Remove an order
    pub fn remove_order(&mut self, market_id: SpotMarketId, order_id: OrderId) -> Option<Order> {
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
    pub fn get_orders_by_account(&self, address: AccountAddress, market_id: SpotMarketId) -> Vec<Order> {
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

    // === Persistence helper methods ===

    /// Get all reserved balances (for persistence)
    pub fn get_all_reserved(&self) -> &HashMap<(AccountAddress, TokenIndex), Decimal> {
        &self.reserved
    }

    /// Get all orders for a market (for persistence)
    pub fn get_all_orders(&self, market_id: SpotMarketId) -> Vec<&Order> {
        self.orders
            .get(&market_id)
            .map(|orders| orders.values().collect())
            .unwrap_or_default()
    }

    /// Get next token index without incrementing (for persistence)
    pub fn next_token_index(&self) -> TokenIndex {
        self.next_token_index
    }

    /// Get next market ID without incrementing (for persistence)
    pub fn next_market_id(&self) -> SpotMarketId {
        self.next_market_id
    }

    /// Get next order ID without incrementing (for persistence)
    pub fn peek_next_order_id(&self) -> OrderId {
        self.next_order_id
    }

    /// Set next token index (for state restore)
    pub fn set_next_token_index(&mut self, index: TokenIndex) {
        self.next_token_index = index;
    }

    /// Set next market ID (for state restore)
    pub fn set_next_market_id(&mut self, id: SpotMarketId) {
        self.next_market_id = id;
    }

    /// Set next order ID (for state restore)
    pub fn set_next_order_id(&mut self, id: OrderId) {
        self.next_order_id = id;
    }

    /// Set reserved balance (for state restore)
    pub fn set_reserved(&mut self, account: AccountAddress, token: TokenIndex, amount: Decimal) {
        if amount.is_zero() {
            self.reserved.remove(&(account, token));
        } else {
            self.reserved.insert((account, token), amount);
        }
    }

    /// Restore a token directly (for state restore)
    pub fn restore_token(&mut self, token: SpotToken) {
        self.tokens.insert(token.index, token);
    }

    /// Restore a market directly (for state restore)
    pub fn restore_market(&mut self, config: SpotMarketConfig) {
        let market_id = config.id;
        let market = SpotMarket::new(config);
        self.markets.insert(market_id, market);
        self.orderbooks.entry(market_id).or_insert_with(OrderBook::new);
        self.orders.entry(market_id).or_insert_with(HashMap::new);
    }

    /// Restore an order directly (for state restore)
    pub fn restore_order(&mut self, market_id: SpotMarketId, order: Order) {
        let order_id = order.id;
        let owner = order.owner;

        // Add to market orders
        self.orders
            .entry(market_id)
            .or_insert_with(HashMap::new)
            .insert(order_id, order.clone());

        // Add to orderbook
        if let Some(book) = self.orderbooks.get_mut(&market_id) {
            book.restore_order(order);
        }

        // Add to account orders index
        self.account_orders
            .entry(owner)
            .or_default()
            .entry(market_id)
            .or_default()
            .push(order_id);
    }

    // === Snapshot/Restore for Re-org Handling ===

    /// Create a snapshot of the current spot engine state
    ///
    /// This captures all state needed to restore during a re-org.
    /// Note: Does NOT capture unified_state - that's handled separately.
    pub fn snapshot(&self) -> SpotEngineStateSnapshot {
        SpotEngineStateSnapshot {
            tokens: self.tokens.clone(),
            markets: self.markets.clone(),
            reserved: self.reserved.clone(),
            orders: self.orders.clone(),
            account_orders: self.account_orders.clone(),
            next_token_index: self.next_token_index,
            next_order_id: self.next_order_id,
            next_market_id: self.next_market_id,
        }
    }

    /// Restore state from a snapshot
    ///
    /// Note: This does NOT restore unified_state - that must be restored separately.
    /// The orderbooks are rebuilt from the orders.
    pub fn restore(&mut self, snapshot: SpotEngineStateSnapshot) {
        self.tokens = snapshot.tokens;
        self.markets = snapshot.markets;
        self.reserved = snapshot.reserved;
        self.orders = snapshot.orders;
        self.account_orders = snapshot.account_orders;
        self.next_token_index = snapshot.next_token_index;
        self.next_order_id = snapshot.next_order_id;
        self.next_market_id = snapshot.next_market_id;

        // Rebuild orderbooks from orders
        self.orderbooks.clear();
        for (market_id, _) in &self.markets {
            self.orderbooks.insert(*market_id, OrderBook::new());
        }
        for (market_id, market_orders) in &self.orders {
            if let Some(book) = self.orderbooks.get_mut(market_id) {
                for order in market_orders.values() {
                    book.insert(order.clone());
                }
            }
        }
    }
}

/// Snapshot of SpotEngineState for re-org handling
#[derive(Debug, Clone)]
pub struct SpotEngineStateSnapshot {
    pub tokens: HashMap<TokenIndex, SpotToken>,
    pub markets: HashMap<SpotMarketId, SpotMarket>,
    pub reserved: HashMap<(AccountAddress, TokenIndex), Decimal>,
    pub orders: HashMap<SpotMarketId, HashMap<OrderId, Order>>,
    pub account_orders: HashMap<AccountAddress, HashMap<SpotMarketId, Vec<OrderId>>>,
    pub next_token_index: TokenIndex,
    pub next_order_id: OrderId,
    pub next_market_id: SpotMarketId,
}

impl Default for SpotEngineState {
    fn default() -> Self {
        Self::new()
    }
}

/// Spot trading engine
pub struct SpotEngine {
    pub state: SpotEngineState,
}

impl SpotEngine {
    /// Create new spot engine
    pub fn new() -> Self {
        Self {
            state: SpotEngineState::new(),
        }
    }

    /// Create new spot engine with shared unified state
    pub fn with_unified_state(unified_state: SharedUnifiedState) -> Self {
        Self {
            state: SpotEngineState::with_unified_state(unified_state),
        }
    }

    /// Get shared unified state reference
    pub fn unified_state(&self) -> &SharedUnifiedState {
        self.state.unified_state()
    }

    /// Deploy a new spot token
    pub fn deploy_token(
        &mut self,
        params: SpotTokenDeployParams,
        deployer: AccountAddress,
        timestamp: Timestamp,
    ) -> Result<SpotToken> {
        self.state.deploy_token(params, deployer, timestamp)
    }

    /// Place a spot order
    pub fn place_order(
        &mut self,
        account: AccountAddress,
        market_id: SpotMarketId,
        request: OrderRequest,
        timestamp: Timestamp,
    ) -> Result<(Order, Vec<SpotFill>)> {
        // Get market
        let market = self.state.get_market(market_id)
            .ok_or(Error::MarketNotFound(market_id))?
            .clone();

        if !market.config.is_active {
            return Err(Error::MarketPaused(market.config.symbol.clone()));
        }

        // Validate order
        self.validate_order(&request, &market.config)?;

        // Check balance
        // For buy: need quote token (USDC) to pay
        // For sell: need base token to sell
        let (reserve_token, reserve_amount) = match request.side {
            OrderSide::Buy => {
                // Reserve USDC for purchase
                let quote_amount = request.size * request.price;
                (market.config.quote_token, quote_amount.to_decimals(Decimal::USDC_DECIMALS))
            }
            OrderSide::Sell => {
                // Reserve base token for sale
                (market.config.base_token, request.size)
            }
        };

        // Check and reserve balance
        let balance = self.state.get_balance(account, reserve_token);
        if balance.available() < reserve_amount {
            return Err(Error::InsufficientBalance {
                required: reserve_amount.to_string_trimmed(),
                available: balance.available().to_string_trimmed(),
            });
        }

        if !self.state.reserve_balance(account, reserve_token, reserve_amount) {
            return Err(Error::InsufficientBalance {
                required: reserve_amount.to_string_trimmed(),
                available: balance.available().to_string_trimmed(),
            });
        }

        // Create order
        let order_id = self.state.next_order_id();
        let order = Order::new(order_id, account, request.clone(), timestamp);

        // Process matching
        let mut fills = Vec::new();
        let mut remaining_order = order.clone();
        // Track maker orders that need state updates after matching
        let mut filled_maker_ids: Vec<OrderId> = Vec::new();
        let mut partially_filled_makers: Vec<(OrderId, Decimal)> = Vec::new();

        {
            let orderbook = self.state.get_orderbook_mut(market_id)
                .ok_or(Error::MarketNotFound(market_id))?;

            // Match against opposite side
            let opposite_side = match request.side {
                OrderSide::Buy => OrderSide::Sell,
                OrderSide::Sell => OrderSide::Buy,
            };

            // Simple matching loop
            while !remaining_order.is_filled() {
                // Get best order from opposite side
                let maker_order = match opposite_side {
                    OrderSide::Sell => orderbook.best_ask(),
                    OrderSide::Buy => orderbook.best_bid(),
                };

                let Some(maker_order) = maker_order else {
                    break;
                };

                let match_price = maker_order.price;

                // Check if prices cross
                let prices_cross = match request.side {
                    OrderSide::Buy => remaining_order.price >= match_price,
                    OrderSide::Sell => remaining_order.price <= match_price,
                };

                if !prices_cross {
                    break;
                }

                // Clone maker order for use (we'll modify the book below)
                let maker_order = maker_order.clone();

                // Calculate fill size
                let fill_size = remaining_order.remaining_size.min(maker_order.remaining_size);
                let fill_price = maker_order.price; // Price at maker's level

                // Calculate quote amount and fees
                let quote_amount = fill_size * fill_price;
                let maker_fee = market.config.maker_fee(quote_amount);
                let taker_fee = market.config.taker_fee(quote_amount);

                // Create fill
                let fill = SpotFill {
                    market_id,
                    maker: maker_order.owner,
                    taker: account,
                    maker_order_id: maker_order.id,
                    taker_order_id: order_id,
                    price: fill_price,
                    base_size: fill_size,
                    quote_size: quote_amount,
                    maker_fee,
                    taker_fee,
                    timestamp,
                    is_taker_buy: matches!(request.side, OrderSide::Buy),
                };

                fills.push(fill.clone());

                // Update remaining order
                remaining_order.remaining_size = remaining_order.remaining_size - fill_size;
                if remaining_order.is_filled() {
                    remaining_order.status = OrderStatus::Filled;
                } else {
                    remaining_order.status = OrderStatus::PartiallyFilled;
                }

                // Remove maker order from book if filled
                if maker_order.remaining_size <= fill_size {
                    orderbook.remove(maker_order.id);
                    filled_maker_ids.push(maker_order.id);
                } else {
                    // Update maker order size in book
                    // Note: In a real implementation, we'd update the order in place
                    // For simplicity, we remove and re-add
                    let mut updated_maker = maker_order.clone();
                    updated_maker.remaining_size = updated_maker.remaining_size - fill_size;
                    orderbook.remove(maker_order.id);
                    if !updated_maker.is_filled() {
                        orderbook.insert(updated_maker.clone());
                        partially_filled_makers.push((maker_order.id, updated_maker.remaining_size));
                    } else {
                        filled_maker_ids.push(maker_order.id);
                    }
                }
            }
        }

        // Sync the orders/account_orders maps with orderbook changes
        for maker_id in &filled_maker_ids {
            self.state.remove_order(market_id, *maker_id);
        }
        for (maker_id, new_remaining) in &partially_filled_makers {
            if let Some(order_entry) = self.state.orders.get_mut(&market_id)
                .and_then(|m| m.get_mut(maker_id))
            {
                order_entry.remaining_size = *new_remaining;
                order_entry.status = OrderStatus::PartiallyFilled;
            }
        }

        // Apply fills to balances
        for fill in &fills {
            self.apply_fill(fill, &market.config)?;
        }

        // Calculate how much of the reserve was consumed by fills
        let filled_reserve = match request.side {
            OrderSide::Buy => {
                // For buy: reserved USDC, filled portion is the quote size
                fills.iter()
                    .map(|f| f.quote_size.to_decimals(reserve_amount.decimals()))
                    .fold(Decimal::from_raw(0, reserve_amount.decimals()), |a, b| a + b)
            }
            OrderSide::Sell => {
                // For sell: reserved base token, filled portion is the base size
                fills.iter()
                    .map(|f| f.base_size.to_decimals(reserve_amount.decimals()))
                    .fold(Decimal::from_raw(0, reserve_amount.decimals()), |a, b| a + b)
            }
        };

        // Calculate remaining reserve (what hasn't been consumed by fills)
        let remaining_reserve = if filled_reserve.raw() >= reserve_amount.raw() {
            Decimal::from_raw(0, reserve_amount.decimals())
        } else {
            reserve_amount - filled_reserve
        };

        // Determine what to do with remaining reserve based on order status
        if !remaining_order.is_filled() && remaining_order.should_rest() {
            // Order will rest - keep the remaining reserve for the resting order
            self.state.add_order(remaining_order.clone());
            // Reserved balance stays as-is to back the resting order
        } else if !remaining_order.is_filled() {
            // Order won't rest (IOC/FOK) - release remaining reserve
            if remaining_reserve.raw() > 0 {
                self.state.release_balance(account, reserve_token, remaining_reserve);
            }
        }
        // If order is fully filled, the fills already consumed the reserve via apply_fill

        Ok((remaining_order, fills))
    }

    /// Cancel a spot order
    pub fn cancel_order(
        &mut self,
        account: AccountAddress,
        market_id: SpotMarketId,
        order_id: OrderId,
    ) -> Result<Order> {
        let order = self.state.get_order(market_id, order_id)
            .ok_or(Error::OrderNotFound(order_id))?
            .clone();

        if order.owner != account {
            return Err(Error::OrderNotOwned);
        }

        // Get market to determine which token was reserved
        let market = self.state.get_market(market_id)
            .ok_or(Error::MarketNotFound(market_id))?
            .clone();

        // Release reserved balance
        let (reserve_token, reserve_amount) = match order.side {
            OrderSide::Buy => {
                let quote_amount = order.remaining_size * order.price;
                (market.config.quote_token, quote_amount.to_decimals(Decimal::USDC_DECIMALS))
            }
            OrderSide::Sell => {
                (market.config.base_token, order.remaining_size)
            }
        };

        self.state.release_balance(account, reserve_token, reserve_amount);
        self.state.remove_order(market_id, order_id);

        let mut canceled = order;
        canceled.cancel();
        Ok(canceled)
    }

    /// Cancel all orders for account in a market
    pub fn cancel_all_orders(
        &mut self,
        account: AccountAddress,
        market_id: SpotMarketId,
    ) -> Result<Vec<Order>> {
        let orders = self.state.get_orders_by_account(account, market_id);
        let mut canceled = Vec::new();

        for order in orders {
            match self.cancel_order(account, market_id, order.id) {
                Ok(c) => canceled.push(c),
                Err(_) => continue, // Skip failed cancellations
            }
        }

        Ok(canceled)
    }

    /// Transfer tokens between Core and EVM views
    ///
    /// This is the Phase 2A view transfer operation - NOT a bridge!
    /// It adjusts views in the unified state without changing totals.
    ///
    /// IMPORTANT: When transferring to EVM, we check AVAILABLE balance
    /// (core_view - reserved), not just core_view. This prevents users
    /// from transferring funds that are reserved for open orders.
    pub fn view_transfer(
        &mut self,
        account: AccountAddress,
        token_index: TokenIndex,
        amount: Decimal,
        to_evm: bool,
    ) -> Result<()> {
        if to_evm {
            // CRITICAL: Check available balance before transferring to EVM
            // Available = core_view - reserved (for open orders)
            let balance = self.state.get_balance(account, token_index);
            let available = balance.available();

            if available < amount {
                return Err(Error::InsufficientBalance {
                    required: amount.to_string_trimmed(),
                    available: available.to_string_trimmed(),
                });
            }

            // Now safe to transfer - reserved funds are protected
            let mut unified = self.state.unified_state.write().unwrap();
            unified.transfer_to_evm_view(account, token_index, amount)
                .map_err(|e| Error::Internal(e.to_string()))
        } else {
            // Transferring from EVM to Core - no reserved balance check needed
            // (EVM view doesn't have reserved funds concept)
            let mut unified = self.state.unified_state.write().unwrap();
            unified.transfer_to_core_view(account, token_index, amount)
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    /// Transfer tokens between EVM and Core (system address bridge)
    /// NOTE: This is the legacy bridge_transfer method kept for backward compatibility.
    /// For new code, use view_transfer() instead.
    pub fn bridge_transfer(
        &mut self,
        token_index: TokenIndex,
        from: AccountAddress,
        to: AccountAddress,
        amount: Decimal,
        is_to_core: bool,
    ) -> Result<()> {
        let token = self.state.get_token(token_index)
            .ok_or(Error::Internal(format!("Token {} not found", token_index)))?
            .clone();

        // Verify transfer involves system address
        let system_address = token.system_address;
        if is_to_core {
            // EVM -> Core: from should be system address
            if from != system_address {
                return Err(invalid_input!("Bridge transfer must come from system address"));
            }
            self.state.credit_balance(to, token_index, amount);
        } else {
            // Core -> EVM: to should be system address
            if to != system_address {
                return Err(invalid_input!("Bridge transfer must go to system address"));
            }
            if !self.state.debit_balance(from, token_index, amount) {
                return Err(Error::InsufficientBalance {
                    required: amount.to_string_trimmed(),
                    available: self.state.get_balance(from, token_index).available().to_string_trimmed(),
                });
            }
        }

        Ok(())
    }

    // Internal methods

    fn validate_order(&self, request: &OrderRequest, config: &SpotMarketConfig) -> Result<()> {
        if !config.validate_price(request.price) {
            return Err(Error::InvalidPrice {
                price: request.price.to_string_trimmed(),
                tick_size: config.tick_size.to_string_trimmed(),
            });
        }

        if !config.validate_size(request.size) {
            return Err(Error::InvalidSize {
                size: request.size.to_string_trimmed(),
                lot_size: config.lot_size.to_string_trimmed(),
            });
        }

        // Normalize size to the market's decimal scale for correct comparison.
        // Order sizes arrive in PRICE_DECIMALS (8) but market config uses
        // the token's wei_decimals (typically 18). The derived Ord on Decimal
        // compares raw values without normalizing, so we must align scales.
        let size_normalized = request.size.to_decimals(config.lot_size.decimals());

        if size_normalized.raw() < config.min_order_size.raw() {
            return Err(Error::SizeTooSmall {
                size: request.size.to_string_trimmed(),
                min_size: config.min_order_size.to_string_trimmed(),
            });
        }

        if size_normalized.raw() > config.max_order_size.raw() {
            return Err(Error::SizeTooLarge {
                size: request.size.to_string_trimmed(),
                max_size: config.max_order_size.to_string_trimmed(),
            });
        }

        Ok(())
    }

    fn apply_fill(&mut self, fill: &SpotFill, config: &SpotMarketConfig) -> Result<()> {
        // For buyer (taker if is_taker_buy, maker otherwise):
        // - Receives base token
        // - Pays quote token + fee
        // For seller:
        // - Receives quote token - fee
        // - Gives base token

        let (buyer, seller) = if fill.is_taker_buy {
            (fill.taker, fill.maker)
        } else {
            (fill.maker, fill.taker)
        };

        let (buyer_fee, seller_fee) = if fill.is_taker_buy {
            (fill.taker_fee, fill.maker_fee)
        } else {
            (fill.maker_fee, fill.taker_fee)
        };

        // Convert all quote amounts to USDC decimals (6)
        let quote_usdc = fill.quote_size.to_decimals(Decimal::USDC_DECIMALS);
        let buyer_fee_usdc = buyer_fee.to_decimals(Decimal::USDC_DECIMALS);
        let seller_fee_usdc = seller_fee.to_decimals(Decimal::USDC_DECIMALS);
        let quote_with_fee = Decimal::from_raw(quote_usdc.raw() + buyer_fee_usdc.raw(), Decimal::USDC_DECIMALS);

        // Get base token decimals from existing balance or fill
        let base_decimals = fill.base_size.decimals();

        // Release reserved balances (they were reserved when order was placed)
        // Buyer had USDC reserved, seller had base token reserved
        self.state.release_balance(buyer, config.quote_token, quote_with_fee);
        self.state.release_balance(seller, config.base_token, fill.base_size);

        // Apply balance changes using unified state
        // Buyer: -USDC (quote + fee), +base
        if !self.state.debit_balance(buyer, config.quote_token, quote_with_fee) {
            return Err(Error::InsufficientBalance {
                required: quote_with_fee.to_string_trimmed(),
                available: "0".to_string(),
            });
        }
        self.state.credit_balance(buyer, config.base_token, fill.base_size);

        // Seller: +USDC (quote - fee), -base
        // Note: seller_fee can be negative (rebate), but we use raw arithmetic to handle it
        let seller_receives = Decimal::from_raw(
            quote_usdc.raw() - seller_fee_usdc.raw(),
            Decimal::USDC_DECIMALS
        );

        if !self.state.debit_balance(seller, config.base_token, fill.base_size) {
            return Err(Error::InsufficientBalance {
                required: fill.base_size.to_string_trimmed(),
                available: "0".to_string(),
            });
        }
        self.state.credit_balance(seller, config.quote_token, seller_receives);

        // Update market state
        if let Some(market) = self.state.get_market_mut(fill.market_id) {
            market.state.last_price = Some(fill.price);
            market.state.last_trade_time = fill.timestamp;
            // Use raw arithmetic to avoid decimal mismatch issues
            let vol_add = fill.quote_size.to_decimals(Decimal::USDC_DECIMALS);
            market.state.volume_24h = Decimal::from_raw(
                market.state.volume_24h.to_decimals(Decimal::USDC_DECIMALS).raw() + vol_add.raw(),
                Decimal::USDC_DECIMALS
            );
            let base_vol_add = fill.base_size.to_decimals(base_decimals);
            market.state.base_volume_24h = Decimal::from_raw(
                market.state.base_volume_24h.to_decimals(base_decimals).raw() + base_vol_add.raw(),
                base_decimals
            );
            market.state.trades_24h += 1;
        }

        Ok(())
    }
}

impl Default for SpotEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use hypercore_primitives::OrderType;

    fn setup_engine() -> SpotEngine {
        SpotEngine::new()
    }

    #[test]
    fn test_deploy_token() {
        let mut engine = setup_engine();
        let deployer = Address::repeat_byte(1);

        let params = SpotTokenDeployParams {
            name: "Test Token".to_string(),
            symbol: "TEST".to_string(),
            wei_decimals: 18,
            sz_decimals: 8,
            max_supply: Decimal::from_raw(1_000_000_000_000_000_000_000_000i128, 18), // 1M tokens
            genesis_allocations: vec![],
            erc20_address: None,
        };

        let result = engine.deploy_token(params, deployer, 1000);
        assert!(result.is_ok());

        let token = result.unwrap();
        assert_eq!(token.index, 1);
        assert_eq!(token.symbol, "TEST");

        // Check deployer received all tokens (in core_view)
        let balance = engine.state.get_balance(deployer, 1);
        assert_eq!(balance.total.raw(), 1_000_000_000_000_000_000_000_000i128);

        // Verify unified state has the balance
        let unified = engine.state.unified_state.read().unwrap();
        let unified_balance = unified.get_balance(deployer, 1).unwrap();
        assert_eq!(unified_balance.core_view.raw(), 1_000_000_000_000_000_000_000_000i128);
        assert_eq!(unified_balance.evm_view.raw(), 0);

        // Check market was created
        let markets = engine.state.get_all_markets();
        assert_eq!(markets.len(), 1);
        assert_eq!(markets[0].config.symbol, "TEST-USDC");
    }

    #[test]
    fn test_spot_order_buy() {
        let mut engine = setup_engine();
        let alice = Address::repeat_byte(1);
        let bob = Address::repeat_byte(2);

        // Deploy token with simpler decimals (6, like USDC)
        let params = SpotTokenDeployParams {
            name: "Test Token".to_string(),
            symbol: "TEST".to_string(),
            wei_decimals: 8, // Use 8 decimals for simplicity
            sz_decimals: 2,  // 2 sz decimals means lot size is 10^6
            max_supply: Decimal::from_raw(1_000_000_00000000i128, 8), // 1M tokens
            genesis_allocations: vec![
                (bob, Decimal::from_raw(500_000_00000000i128, 8)), // 500K to Bob
            ],
            erc20_address: None,
        };
        let _token = engine.deploy_token(params, alice, 1000).unwrap();

        // Give Alice some USDC to buy with
        engine.state.credit_balance(alice, 0, Decimal::from_raw(10_000_000_000i128, 6)); // $10,000

        let market = engine.state.find_market_by_symbol("TEST-USDC").unwrap();
        let market_id = market.id();

        // Bob places sell order: 100 tokens at $1 each
        let sell_request = OrderRequest {
            market_id,
            side: OrderSide::Sell,
            price: Decimal::price("1.00"),  // $1 per token
            size: Decimal::from_raw(100_00000000i128, 8), // 100 tokens (8 decimals)
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
        };

        let (bob_order, _) = engine.place_order(bob, market_id, sell_request, 2000).unwrap();
        assert!(!bob_order.is_filled()); // Resting order

        // Alice places buy order that matches: 50 tokens at $1 each
        let buy_request = OrderRequest {
            market_id,
            side: OrderSide::Buy,
            price: Decimal::price("1.00"),
            size: Decimal::from_raw(50_00000000i128, 8), // 50 tokens (8 decimals)
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
        };

        let (alice_order, fills) = engine.place_order(alice, market_id, buy_request, 3000).unwrap();

        assert!(alice_order.is_filled());
        assert_eq!(fills.len(), 1);
        assert!(fills[0].is_taker_buy);
    }

    #[test]
    fn test_view_transfer() {
        let mut engine = setup_engine();
        let user = Address::repeat_byte(1);

        // Credit some USDC
        engine.state.credit_balance(user, 0, Decimal::from_raw(1000_000_000, 6)); // 1000 USDC

        // Check initial state - all in core view
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(user, 0).unwrap();
            assert_eq!(balance.total.raw(), 1000_000_000);
            assert_eq!(balance.core_view.raw(), 1000_000_000);
            assert_eq!(balance.evm_view.raw(), 0);
        }

        // Transfer 400 USDC to EVM view
        let result = engine.view_transfer(user, 0, Decimal::from_raw(400_000_000, 6), true);
        assert!(result.is_ok());

        // Check updated state
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(user, 0).unwrap();
            assert_eq!(balance.total.raw(), 1000_000_000); // Unchanged!
            assert_eq!(balance.core_view.raw(), 600_000_000);
            assert_eq!(balance.evm_view.raw(), 400_000_000);
            assert!(balance.is_valid());
        }

        // Transfer 200 USDC back to Core view
        let result = engine.view_transfer(user, 0, Decimal::from_raw(200_000_000, 6), false);
        assert!(result.is_ok());

        // Check final state
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(user, 0).unwrap();
            assert_eq!(balance.total.raw(), 1000_000_000); // Still unchanged!
            assert_eq!(balance.core_view.raw(), 800_000_000);
            assert_eq!(balance.evm_view.raw(), 200_000_000);
            assert!(balance.is_valid());
        }
    }

    #[test]
    fn test_view_transfer_insufficient_balance() {
        let mut engine = setup_engine();
        let user = Address::repeat_byte(1);

        // Credit some USDC (all in core view)
        engine.state.credit_balance(user, 0, Decimal::from_raw(1000_000_000, 6));

        // Try to transfer more than available from EVM view (which is 0)
        let result = engine.view_transfer(user, 0, Decimal::from_raw(100_000_000, 6), false);
        assert!(result.is_err());

        // Try to transfer more than available from Core view
        let result = engine.view_transfer(user, 0, Decimal::from_raw(2000_000_000, 6), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_system_address() {
        let token = SpotToken::new(
            1,
            "Test".to_string(),
            "TEST".to_string(),
            18,
            8,
            Decimal::from_raw(1000, 18),
            Address::ZERO,
        );

        // System address should be deterministic
        let addr1 = SpotToken::derive_system_address(1);
        let addr2 = token.system_address;
        assert_eq!(addr1, addr2);
    }

    #[test]
    fn test_view_transfer_respects_reserved_balance() {
        // This test verifies the critical fix: view_transfer must check AVAILABLE
        // balance (core_view - reserved), not just core_view.
        let mut engine = setup_engine();
        let alice = Address::repeat_byte(1);

        // Give Alice 1000 USDC in core view
        engine.state.credit_balance(alice, 0, Decimal::from_raw(1000_000_000, 6));

        // Manually reserve 500 USDC (simulates having open orders)
        // In real scenarios, place_order would reserve this
        let reserved_amount = Decimal::from_raw(500_000_000, 6);
        let reserve_success = engine.state.reserve_balance(alice, 0, reserved_amount);
        assert!(reserve_success, "Should be able to reserve balance");

        // Now Alice has:
        // - core_view: 1000 USDC
        // - reserved: 500 USDC
        // - available: 500 USDC
        let balance = engine.state.get_balance(alice, 0);
        assert_eq!(balance.total.raw(), 1000_000_000, "Core view should be 1000 USDC");
        assert_eq!(balance.reserved.raw(), 500_000_000, "500 USDC should be reserved");
        let available = balance.available();
        assert_eq!(available.raw(), 500_000_000, "Available should be 500 USDC");

        // Try to transfer 800 USDC to EVM - should FAIL because only 500 is available
        let transfer_result = engine.view_transfer(
            alice,
            0,
            Decimal::from_raw(800_000_000, 6),
            true
        );
        assert!(transfer_result.is_err(), "Transfer should fail - reserved balance protection");

        // Transfer 400 USDC to EVM - should SUCCEED because it's within available
        let transfer_result = engine.view_transfer(
            alice,
            0,
            Decimal::from_raw(400_000_000, 6),
            true
        );
        assert!(transfer_result.is_ok(), "Transfer should succeed - within available balance");

        // Verify state after successful transfer
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(alice, 0).unwrap();
            // Total should remain at 1000 USDC
            assert_eq!(balance.total.raw(), 1000_000_000);
            // Core view reduced by 400
            assert_eq!(balance.core_view.raw(), 600_000_000);
            // EVM view increased by 400
            assert_eq!(balance.evm_view.raw(), 400_000_000);
            assert!(balance.is_valid(), "Invariant should hold");
        }

        // After transfer, Alice's SpotBalance should still show reserved
        let spot_balance = engine.state.get_balance(alice, 0);
        assert_eq!(spot_balance.reserved.raw(), 500_000_000, "Reserved should still be 500");
        // Available is now core_view (600) - reserved (500) = 100
        assert_eq!(spot_balance.available().raw(), 100_000_000, "Available should be 100");

        // Try to transfer 200 more - should FAIL because only 100 is available
        let transfer_result = engine.view_transfer(
            alice,
            0,
            Decimal::from_raw(200_000_000, 6),
            true
        );
        assert!(transfer_result.is_err(), "Transfer should fail - only 100 available");

        // Transfer 100 - should SUCCEED (exact available amount)
        let transfer_result = engine.view_transfer(
            alice,
            0,
            Decimal::from_raw(100_000_000, 6),
            true
        );
        assert!(transfer_result.is_ok(), "Transfer should succeed - exact available");

        // Final state check
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(alice, 0).unwrap();
            assert_eq!(balance.total.raw(), 1000_000_000, "Total unchanged");
            assert_eq!(balance.core_view.raw(), 500_000_000, "Core view = 500");
            assert_eq!(balance.evm_view.raw(), 500_000_000, "EVM view = 500");
        }
    }

    #[test]
    fn test_view_transfer_evm_to_core_no_reserve_check() {
        // Transferring from EVM to Core should NOT check reserved balances
        // because EVM view doesn't have the reserved funds concept
        let mut engine = setup_engine();
        let user = Address::repeat_byte(1);

        // Credit 1000 USDC to core view
        engine.state.credit_balance(user, 0, Decimal::from_raw(1000_000_000, 6));

        // Transfer 500 to EVM
        engine.view_transfer(user, 0, Decimal::from_raw(500_000_000, 6), true).unwrap();

        // Verify EVM view has 500
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(user, 0).unwrap();
            assert_eq!(balance.evm_view.raw(), 500_000_000);
            assert_eq!(balance.core_view.raw(), 500_000_000);
        }

        // Transfer back 300 from EVM to Core - should work without reserve check
        let result = engine.view_transfer(user, 0, Decimal::from_raw(300_000_000, 6), false);
        assert!(result.is_ok(), "EVM to Core transfer should succeed");

        // Verify final state
        {
            let unified = engine.state.unified_state.read().unwrap();
            let balance = unified.get_balance(user, 0).unwrap();
            assert_eq!(balance.total.raw(), 1000_000_000);
            assert_eq!(balance.core_view.raw(), 800_000_000);
            assert_eq!(balance.evm_view.raw(), 200_000_000);
        }
    }

    #[test]
    fn test_resting_order_reserves_balance() {
        // Test that a resting (unmatched) order maintains its reserved balance
        let mut engine = setup_engine();
        let alice = Address::repeat_byte(1);
        let bob = Address::repeat_byte(2);

        // Deploy a token
        let params = SpotTokenDeployParams {
            name: "Test Token".to_string(),
            symbol: "TEST".to_string(),
            wei_decimals: 8,
            sz_decimals: 2,
            max_supply: Decimal::from_raw(1_000_000_00000000i128, 8),
            genesis_allocations: vec![
                (bob, Decimal::from_raw(500_000_00000000i128, 8)), // Bob has tokens to sell
            ],
            erc20_address: None,
        };
        let _token = engine.deploy_token(params, alice, 1000).unwrap();

        // Give Alice 1000 USDC
        engine.state.credit_balance(alice, 0, Decimal::from_raw(1000_000_000, 6));

        let market = engine.state.find_market_by_symbol("TEST-USDC").unwrap();
        let market_id = market.id();

        // Check Alice's balance before order
        let before = engine.state.get_balance(alice, 0);
        assert_eq!(before.total.raw(), 1000_000_000, "Alice should have 1000 USDC");
        assert_eq!(before.reserved.raw(), 0, "No reserved balance yet");

        // Alice places a buy order at a low price that won't match
        // Buy 1000 TEST at $0.05 = 50 USDC to reserve
        let buy_request = OrderRequest {
            market_id,
            side: OrderSide::Buy,
            price: Decimal::price("0.05"),
            size: Decimal::from_raw(1000_00000000i128, 8), // 1000 tokens
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
        };

        let result = engine.place_order(alice, market_id, buy_request, 2000);
        assert!(result.is_ok(), "Order should be placed");
        let (order, fills) = result.unwrap();
        assert!(fills.is_empty(), "Order should not match (no sells at this price)");
        assert!(!order.is_filled(), "Order should not be filled");

        // CRITICAL: Verify reserved balance is maintained for resting order
        let after = engine.state.get_balance(alice, 0);
        assert_eq!(after.total.raw(), 1000_000_000, "Total should be unchanged");
        // Reserved should be approximately 1000 * 0.05 = 50 USDC (50_000_000 raw)
        assert!(after.reserved.raw() > 0, "Reserved balance should be non-zero for resting order");
        assert!(after.reserved.raw() >= 49_000_000 && after.reserved.raw() <= 51_000_000,
                "Reserved should be ~50 USDC, got: {}", after.reserved.raw());

        // Available should be total - reserved
        let expected_available = after.total.raw() - after.reserved.raw();
        assert_eq!(after.available().raw(), expected_available,
                   "Available should be total minus reserved");

        // Cancel the order - should release the reserved balance
        let cancel_result = engine.cancel_order(alice, market_id, order.id);
        assert!(cancel_result.is_ok(), "Order should be cancelled");

        // After cancel, reserved should be 0
        let after_cancel = engine.state.get_balance(alice, 0);
        assert_eq!(after_cancel.reserved.raw(), 0, "Reserved should be 0 after cancel");
        assert_eq!(after_cancel.available().raw(), 1000_000_000, "Full balance should be available");
    }
}
