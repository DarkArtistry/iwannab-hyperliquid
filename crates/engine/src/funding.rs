//! Funding rate calculation and settlement

use hypercore_primitives::{Decimal, Market, MarketState, Timestamp};

/// Funding engine for rate calculation and settlement
pub struct FundingEngine {
    /// Funding interval in seconds
    pub funding_interval_secs: u64,
    /// Maximum funding rate per interval
    pub max_funding_rate: Decimal,
    /// EWMA decay factor for premium calculation
    pub ewma_decay: Decimal,
}

impl FundingEngine {
    /// Create a new funding engine
    pub fn new(funding_interval_secs: u64, max_funding_rate: Decimal) -> Self {
        Self {
            funding_interval_secs,
            max_funding_rate,
            // 5 minute EWMA
            ewma_decay: Decimal::rate("0.9"),
        }
    }

    /// Calculate funding rate based on market state
    ///
    /// Funding rate = clamp(premium_index, -max_rate, +max_rate)
    /// Premium index = (mark - index) / index
    pub fn calculate_funding_rate(
        &self,
        market_state: &MarketState,
        index_price: Decimal,
    ) -> Decimal {
        if index_price.is_zero() {
            return Decimal::from_raw(0, Decimal::RATE_DECIMALS);
        }

        // Calculate premium: (mark - index) / index
        let mark = market_state.mark_price.to_decimals(Decimal::RATE_DECIMALS);
        let index = index_price.to_decimals(Decimal::RATE_DECIMALS);

        let premium = (mark - index) / index;

        // Clamp to max funding rate
        self.clamp_rate(premium)
    }

    /// Calculate premium index using mid price
    pub fn calculate_premium_index(
        &self,
        best_bid: Option<Decimal>,
        best_ask: Option<Decimal>,
        index_price: Decimal,
    ) -> Decimal {
        let mid_price = match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => (bid + ask).div_int(2),
            _ => return Decimal::from_raw(0, Decimal::RATE_DECIMALS),
        };

        if index_price.is_zero() {
            return Decimal::from_raw(0, Decimal::RATE_DECIMALS);
        }

        let mid = mid_price.to_decimals(Decimal::RATE_DECIMALS);
        let index = index_price.to_decimals(Decimal::RATE_DECIMALS);

        (mid - index) / index
    }

    /// Clamp funding rate to allowed range
    fn clamp_rate(&self, rate: Decimal) -> Decimal {
        let max = self.max_funding_rate;
        let min = -max;

        if rate > max {
            max
        } else if rate < min {
            min
        } else {
            rate
        }
    }

    /// Settle funding for a market
    pub fn settle_funding(
        &self,
        market: &mut Market,
        funding_rate: Decimal,
        current_time: Timestamp,
    ) {
        // Update funding accumulator
        let funding_delta = funding_rate * market.state.mark_price.to_decimals(funding_rate.decimals());
        market.state.funding_accumulator = market.state.funding_accumulator + funding_delta;

        // Store current funding rate
        market.state.funding_rate = funding_rate;

        // Schedule next funding
        market.state.next_funding_time = current_time + (self.funding_interval_secs * 1000);
    }

    /// Calculate funding payment for a position
    ///
    /// Returns the payment amount (positive = position holder pays)
    pub fn calculate_funding_payment(
        &self,
        position_size: Decimal,
        last_funding_index: Decimal,
        current_funding_accumulator: Decimal,
    ) -> Decimal {
        let funding_delta = current_funding_accumulator - last_funding_index;
        position_size * funding_delta
    }

    /// Check if funding should be settled
    pub fn should_settle(&self, market_state: &MarketState, current_time: Timestamp) -> bool {
        current_time >= market_state.next_funding_time
    }

    /// Get time until next funding
    pub fn time_until_funding(&self, market_state: &MarketState, current_time: Timestamp) -> u64 {
        if current_time >= market_state.next_funding_time {
            0
        } else {
            market_state.next_funding_time - current_time
        }
    }
}

impl Default for FundingEngine {
    fn default() -> Self {
        Self::new(
            8 * 60 * 60, // 8 hours
            Decimal::rate("0.0005"), // 0.05%
        )
    }
}

/// Funding rate history entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FundingRateEntry {
    pub timestamp: Timestamp,
    pub funding_rate: Decimal,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub premium_index: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypercore_primitives::MarketConfig;

    fn setup() -> (FundingEngine, Market) {
        let engine = FundingEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );
        (engine, market)
    }

    #[test]
    fn test_funding_rate_neutral() {
        let (engine, market) = setup();

        // Mark = Index -> 0% funding
        let rate = engine.calculate_funding_rate(&market.state, Decimal::price("65000"));
        assert!(rate.is_zero());
    }

    #[test]
    fn test_funding_rate_positive() {
        let (engine, mut market) = setup();

        // Mark > Index -> positive funding (longs pay shorts)
        market.state.mark_price = Decimal::price("65325"); // 0.5% above index

        let rate = engine.calculate_funding_rate(&market.state, Decimal::price("65000"));
        assert!(rate.is_positive());
        // Should be clamped to max 0.05%
        assert!(rate <= engine.max_funding_rate);
    }

    #[test]
    fn test_funding_rate_negative() {
        let (engine, mut market) = setup();

        // Mark < Index -> negative funding (shorts pay longs)
        market.state.mark_price = Decimal::price("64675"); // 0.5% below index

        let rate = engine.calculate_funding_rate(&market.state, Decimal::price("65000"));
        assert!(rate.is_negative());
        assert!(rate >= -engine.max_funding_rate);
    }

    #[test]
    fn test_funding_rate_clamping() {
        let (engine, mut market) = setup();

        // Extreme premium should be clamped
        market.state.mark_price = Decimal::price("70000"); // ~7.7% above index

        let rate = engine.calculate_funding_rate(&market.state, Decimal::price("65000"));
        assert_eq!(rate, engine.max_funding_rate);
    }

    #[test]
    fn test_funding_settlement() {
        let (engine, mut market) = setup();

        let initial_accumulator = market.state.funding_accumulator;
        let rate = Decimal::rate("0.0001"); // 0.01%
        let timestamp = 1000u64;

        engine.settle_funding(&mut market, rate, timestamp);

        assert!(market.state.funding_accumulator > initial_accumulator);
        assert_eq!(market.state.funding_rate, rate);
        assert_eq!(
            market.state.next_funding_time,
            timestamp + (engine.funding_interval_secs * 1000)
        );
    }

    #[test]
    fn test_funding_payment_calculation() {
        let engine = FundingEngine::default();

        // Long 1 BTC, funding delta = 6.5 (65000 * 0.0001)
        let position_size = Decimal::size("1.0");
        let last_index = Decimal::rate("0");
        let current_accumulator = Decimal::rate("6.5");

        let payment = engine.calculate_funding_payment(
            position_size,
            last_index,
            current_accumulator,
        );

        // Long pays 6.5 USDC
        assert_eq!(payment.to_string_trimmed(), "6.5");
    }

    #[test]
    fn test_should_settle() {
        let engine = FundingEngine::default();
        // Create market with a future funding time
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            1000000, // Future funding time in ms
        );

        // Before funding time
        assert!(!engine.should_settle(&market.state, 0));
        assert!(!engine.should_settle(&market.state, 999999));

        // At funding time
        assert!(engine.should_settle(&market.state, market.state.next_funding_time));

        // After funding time
        assert!(engine.should_settle(&market.state, market.state.next_funding_time + 1000));
    }
}
