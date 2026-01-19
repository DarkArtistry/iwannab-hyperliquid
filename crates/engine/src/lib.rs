//! HyperCore Matching Engine
//!
//! This crate implements the core trading engine including:
//! - Order book management
//! - Deterministic order matching
//! - Risk and margin calculations
//! - Funding rate computation
//! - Liquidation logic
//! - HIP-1 style spot token trading

pub mod funding;
pub mod liquidation;
pub mod matching;
pub mod orderbook;
pub mod risk;
pub mod spot_engine;
pub mod state;

pub use funding::FundingEngine;
pub use liquidation::LiquidationEngine;
pub use matching::MatchingEngine;
pub use orderbook::OrderBook;
pub use risk::RiskEngine;
pub use spot_engine::{SpotEngine, SpotEngineState};
pub use state::{BlockMetadata, EngineState};

use hypercore_primitives::{
    AccountAddress, Decimal, Error, Fill, Market, MarketConfig, MarketId, Order, OrderId,
    OrderRequest, OrderSide, Position, Result, Timestamp,
};
use std::collections::HashMap;

/// Engine configuration
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Maximum number of markets
    pub max_markets: usize,
    /// Funding interval in seconds (default: 8 hours)
    pub funding_interval_secs: u64,
    /// Maximum funding rate per interval (default: 0.05%)
    pub max_funding_rate: Decimal,
    /// Liquidation partial size (default: 25%)
    pub liquidation_partial_ratio: Decimal,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_markets: 256,
            funding_interval_secs: 8 * 60 * 60, // 8 hours
            max_funding_rate: Decimal::rate("0.0005"), // 0.05%
            liquidation_partial_ratio: Decimal::rate("0.25"), // 25%
        }
    }
}

/// Main engine orchestrating all components
pub struct Engine {
    pub config: EngineConfig,
    pub state: EngineState,
    pub matching: MatchingEngine,
    pub risk: RiskEngine,
    pub funding: FundingEngine,
    pub liquidation: LiquidationEngine,
}

impl Engine {
    /// Create a new engine instance
    pub fn new(config: EngineConfig) -> Self {
        Self {
            state: EngineState::new(),
            matching: MatchingEngine::new(),
            risk: RiskEngine::new(),
            funding: FundingEngine::new(config.funding_interval_secs, config.max_funding_rate),
            liquidation: LiquidationEngine::new(config.liquidation_partial_ratio),
            config,
        }
    }

    /// Initialize a market
    pub fn add_market(&mut self, config: MarketConfig, initial_price: Decimal, timestamp: Timestamp) {
        let market = Market::new(config.clone(), initial_price, timestamp + self.config.funding_interval_secs * 1000);
        self.state.add_market(market);
        self.matching.add_orderbook(config.id);
    }

    /// Process an order
    pub fn place_order(
        &mut self,
        account: AccountAddress,
        request: OrderRequest,
        timestamp: Timestamp,
    ) -> Result<(Order, Vec<Fill>)> {
        let market_id = request.market_id;

        // Clone market data upfront to avoid borrow conflicts
        let market = self.state.get_market(market_id)
            .ok_or(Error::MarketNotFound(market_id))?
            .clone();

        // Validate market is active
        if !market.state.is_tradeable() {
            return Err(Error::MarketPaused(market.config.symbol.clone()));
        }

        // Validate order parameters
        self.validate_order(&request, &market.config)?;

        // Create account if needed, then get a clone of its state for validation
        self.state.get_or_create_account_mut(account);
        let account_state = self.state.get_account(account).unwrap().clone();

        // Create position if needed, then get a clone for validation
        {
            let _ = self.state.get_or_create_position(account, market_id);
        }
        let position = self.state.get_position(account, market_id)
            .cloned()
            .unwrap_or_else(Position::new);

        // Validate reduce-only
        if request.reduce_only {
            self.validate_reduce_only(&request, &position)?;
        }

        // Check margin (unless reduce-only)
        if !request.reduce_only {
            let leverage = self.state.get_leverage(account, market_id);
            self.risk.check_order_margin(
                &request,
                &account_state,
                &position,
                &market,
                leverage,
            )?;
        }

        // Create order
        let order_id = self.state.next_order_id();
        let order = Order::new(order_id, account, request.clone(), timestamp);

        // Collect positions needed for matching (to avoid borrow in closure)
        // In a real implementation, we'd need a more sophisticated approach
        let positions_snapshot: std::collections::HashMap<AccountAddress, Position> = self.state.accounts.keys()
            .filter_map(|addr| {
                self.state.get_position(*addr, market_id).cloned().map(|p| (*addr, p))
            })
            .collect();

        // Execute matching
        let orderbook = self.state.get_orderbook_mut(market_id)
            .ok_or(Error::MarketNotFound(market_id))?;
        let (updated_order, fills) = self.matching.process_order(
            order,
            orderbook,
            |maker_addr| positions_snapshot.get(&maker_addr).cloned(),
        )?;

        // Apply fills
        for fill in &fills {
            self.apply_fill(fill, &market.config)?;
        }

        // Add remaining to book if applicable
        if !updated_order.is_filled() && updated_order.should_rest() {
            self.state.add_order(updated_order.clone());
        }

        Ok((updated_order, fills))
    }

    /// Cancel an order
    pub fn cancel_order(
        &mut self,
        account: AccountAddress,
        market_id: MarketId,
        order_id: OrderId,
    ) -> Result<Order> {
        let order = self.state.get_order(market_id, order_id)
            .ok_or(Error::OrderNotFound(order_id))?
            .clone();

        if order.owner != account {
            return Err(Error::OrderNotOwned);
        }

        self.state.remove_order(market_id, order_id);
        self.matching.remove_from_book(market_id, &order)?;

        let mut canceled = order;
        canceled.cancel();
        Ok(canceled)
    }

    /// Cancel all orders for an account in a market
    pub fn cancel_all_orders(
        &mut self,
        account: AccountAddress,
        market_id: MarketId,
    ) -> Result<Vec<Order>> {
        let orders = self.state.get_orders_by_account(account, market_id);
        let mut canceled = Vec::new();

        for order in orders {
            self.state.remove_order(market_id, order.id);
            self.matching.remove_from_book(market_id, &order)?;
            let mut c = order;
            c.cancel();
            canceled.push(c);
        }

        Ok(canceled)
    }

    /// Update leverage for an account in a market
    pub fn update_leverage(
        &mut self,
        account: AccountAddress,
        market_id: MarketId,
        new_leverage: u8,
    ) -> Result<()> {
        let market = self.state.get_market(market_id)
            .ok_or(Error::MarketNotFound(market_id))?;

        if new_leverage < 1 || new_leverage > market.config.max_leverage {
            return Err(Error::InvalidLeverage {
                leverage: new_leverage,
                max_leverage: market.config.max_leverage,
            });
        }

        // Check if leverage change would cause liquidation
        let position = self.state.get_position(account, market_id);
        if let Some(pos) = position {
            if !pos.is_empty() {
                let account_state = self.state.get_account(account)
                    .ok_or(Error::Internal("Account not found".to_string()))?;

                self.risk.check_leverage_change(
                    &account_state,
                    &pos,
                    &market,
                    new_leverage,
                )?;
            }
        }

        self.state.set_leverage(account, market_id, new_leverage);
        Ok(())
    }

    /// Process funding for all markets
    pub fn process_funding(&mut self, timestamp: Timestamp) -> Vec<(MarketId, Decimal)> {
        let mut rates = Vec::new();

        for market_id in 0..self.config.max_markets as u8 {
            if let Some(market) = self.state.get_market_mut(market_id) {
                if timestamp >= market.state.next_funding_time {
                    let rate = self.funding.calculate_funding_rate(
                        &market.state,
                        market.state.index_price,
                    );

                    self.funding.settle_funding(market, rate, timestamp);
                    rates.push((market_id, rate));
                }
            }
        }

        rates
    }

    /// Check and process liquidations
    pub fn process_liquidations(&mut self, timestamp: Timestamp) -> Vec<(AccountAddress, MarketId, Decimal)> {
        let mut liquidations = Vec::new();

        // Find accounts that need liquidation
        let underwater_accounts = self.find_underwater_accounts();

        for (account, market_id) in underwater_accounts {
            if let Some(market) = self.state.get_market(market_id) {
                if let Some(position) = self.state.get_position(account, market_id) {
                    let liq_result = self.liquidation.process_liquidation(
                        account,
                        &position,
                        &market,
                        timestamp,
                    );

                    if let Some((liq_size, liq_price)) = liq_result {
                        liquidations.push((account, market_id, liq_size));

                        // Apply liquidation (simplified - in production would go through orderbook)
                        self.apply_liquidation(account, market_id, liq_size, liq_price);
                    }
                }
            }
        }

        liquidations
    }

    // Internal methods

    fn validate_order(&self, request: &OrderRequest, config: &MarketConfig) -> Result<()> {
        // Validate price
        if !config.validate_price(request.price) {
            return Err(Error::InvalidPrice {
                price: request.price.to_string_trimmed(),
                tick_size: config.tick_size.to_string_trimmed(),
            });
        }

        // Validate size
        if !config.validate_size(request.size) {
            return Err(Error::InvalidSize {
                size: request.size.to_string_trimmed(),
                lot_size: config.lot_size.to_string_trimmed(),
            });
        }

        if request.size < config.min_order_size {
            return Err(Error::SizeTooSmall {
                size: request.size.to_string_trimmed(),
                min_size: config.min_order_size.to_string_trimmed(),
            });
        }

        if request.size > config.max_order_size {
            return Err(Error::SizeTooLarge {
                size: request.size.to_string_trimmed(),
                max_size: config.max_order_size.to_string_trimmed(),
            });
        }

        Ok(())
    }

    fn validate_reduce_only(&self, request: &OrderRequest, position: &Position) -> Result<()> {
        // Reduce-only must reduce position
        let would_increase = match request.side {
            OrderSide::Buy => position.is_long() || position.is_empty(),
            OrderSide::Sell => position.is_short() || position.is_empty(),
        };

        if would_increase {
            return Err(Error::ReduceOnlyViolation);
        }

        Ok(())
    }

    fn apply_fill(&mut self, fill: &Fill, config: &MarketConfig) -> Result<()> {
        // Update maker position
        if let Some(maker_pos) = self.state.get_position_mut(fill.maker, fill.market_id) {
            let maker_is_buy = !fill.is_taker_buy;
            maker_pos.apply_fill(
                Decimal::from_raw(fill.size as i128, Decimal::SIZE_DECIMALS),
                Decimal::from_raw(fill.price as i128, Decimal::PRICE_DECIMALS),
                maker_is_buy,
            );
        }

        // Update taker position
        if let Some(taker_pos) = self.state.get_position_mut(fill.taker, fill.market_id) {
            taker_pos.apply_fill(
                Decimal::from_raw(fill.size as i128, Decimal::SIZE_DECIMALS),
                Decimal::from_raw(fill.price as i128, Decimal::PRICE_DECIMALS),
                fill.is_taker_buy,
            );
        }

        // Update balances (fees)
        if let Some(maker_state) = self.state.get_account_mut(fill.maker) {
            maker_state.balance -= fill.maker_fee;
        }
        if let Some(taker_state) = self.state.get_account_mut(fill.taker) {
            taker_state.balance -= fill.taker_fee as i128;
        }

        // Update market state
        if let Some(market) = self.state.get_market_mut(fill.market_id) {
            market.state.last_trade_time = fill.timestamp;
            // Update mark price (simplified - in production use EWMA)
            market.state.mark_price = Decimal::from_raw(fill.price as i128, Decimal::PRICE_DECIMALS);
        }

        Ok(())
    }

    fn find_underwater_accounts(&self) -> Vec<(AccountAddress, MarketId)> {
        let mut result = Vec::new();

        for (account, state) in self.state.accounts.iter() {
            for (market_id, position) in self.state.get_all_positions(*account) {
                if let Some(market) = self.state.get_market(market_id) {
                    if self.risk.is_liquidatable(state, &position, &market) {
                        result.push((*account, market_id));
                    }
                }
            }
        }

        // Sort for deterministic liquidation order
        // This ensures all validators process liquidations in the same order
        result.sort_by_key(|(addr, mid)| (*addr, *mid));
        result
    }

    fn apply_liquidation(
        &mut self,
        account: AccountAddress,
        market_id: MarketId,
        size: Decimal,
        price: Decimal,
    ) {
        // This is simplified - in production, liquidation would go through the orderbook
        if let Some(position) = self.state.get_position_mut(account, market_id) {
            let is_buy = position.is_short(); // Close position
            position.apply_fill(size, price, is_buy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use hypercore_primitives::OrderType;

    fn setup_engine() -> Engine {
        let mut engine = Engine::new(EngineConfig::default());
        engine.add_market(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
        engine
    }

    #[test]
    fn test_place_order() {
        let mut engine = setup_engine();
        let account = Address::repeat_byte(1);

        // Deposit some funds
        engine.state.deposit(account, 10000_000000); // $10,000

        let request = OrderRequest {
            market_id: 0,
            side: OrderSide::Buy,
            price: Decimal::price("65000"),
            size: Decimal::size("0.1"),
            order_type: OrderType::default(),
            reduce_only: false,
            client_order_id: None,
        };

        let result = engine.place_order(account, request, 1000);
        assert!(result.is_ok());

        let (order, fills) = result.unwrap();
        assert!(fills.is_empty()); // No matching orders
        assert_eq!(order.remaining_size, Decimal::size("0.1"));
    }
}
