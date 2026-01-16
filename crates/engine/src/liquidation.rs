//! Liquidation engine for handling underwater positions

use hypercore_primitives::{AccountAddress, Decimal, Market, Position, Timestamp};

/// Liquidation engine
pub struct LiquidationEngine {
    /// Partial liquidation ratio (default 25%)
    pub partial_ratio: Decimal,
    /// Liquidation spread (penalty applied to liquidation price)
    pub liquidation_spread: Decimal,
}

impl LiquidationEngine {
    /// Create a new liquidation engine
    pub fn new(partial_ratio: Decimal) -> Self {
        Self {
            partial_ratio,
            liquidation_spread: Decimal::rate("0.005"), // 0.5%
        }
    }

    /// Process a liquidation for an underwater position
    ///
    /// Returns (liquidation_size, liquidation_price) if liquidation is needed
    pub fn process_liquidation(
        &self,
        account: AccountAddress,
        position: &Position,
        market: &Market,
        timestamp: Timestamp,
    ) -> Option<(Decimal, Decimal)> {
        if position.is_empty() {
            return None;
        }

        // Calculate liquidation size (partial liquidation)
        let liq_size = self.calculate_liquidation_size(position);

        // Calculate liquidation price with spread
        let liq_price = self.calculate_liquidation_price(position, market);

        Some((liq_size, liq_price))
    }

    /// Calculate the size to liquidate (partial liquidation)
    pub fn calculate_liquidation_size(&self, position: &Position) -> Decimal {
        let abs_size = position.abs_size();
        let partial_size = abs_size * self.partial_ratio.to_decimals(abs_size.decimals());

        // Ensure minimum liquidation of the smaller of 25% or full position
        partial_size.min(abs_size)
    }

    /// Calculate liquidation price with spread penalty
    pub fn calculate_liquidation_price(&self, position: &Position, market: &Market) -> Decimal {
        let mark = market.state.mark_price;
        let spread_factor = self.liquidation_spread.to_decimals(mark.decimals());

        if position.is_long() {
            // Long liquidation: sell below mark price
            let one = Decimal::from_int(1, mark.decimals());
            mark * (one - spread_factor)
        } else {
            // Short liquidation: buy above mark price
            let one = Decimal::from_int(1, mark.decimals());
            mark * (one + spread_factor)
        }
    }

    /// Calculate bankruptcy price (where equity = 0)
    pub fn calculate_bankruptcy_price(&self, position: &Position, balance: Decimal) -> Option<Decimal> {
        if position.is_empty() {
            return None;
        }

        let entry_price = position.entry_price()?;
        let balance_norm = balance.to_decimals(entry_price.decimals());

        if position.is_long() {
            // Long: bankruptcy when mark_price = entry_price - balance/size
            let per_unit = balance_norm / position.abs_size();
            Some(entry_price - per_unit)
        } else {
            // Short: bankruptcy when mark_price = entry_price + balance/size
            let per_unit = balance_norm / position.abs_size();
            Some(entry_price + per_unit)
        }
    }

    /// Calculate insurance fund contribution from liquidation
    pub fn calculate_insurance_contribution(
        &self,
        liq_size: Decimal,
        liq_price: Decimal,
        fee_rate: Decimal,
    ) -> Decimal {
        let notional = liq_size * liq_price;
        notional * fee_rate.to_decimals(notional.decimals())
    }

    /// Check if position should trigger ADL (auto-deleverage)
    ///
    /// ADL is triggered when insurance fund cannot cover the bankruptcy
    pub fn should_trigger_adl(
        &self,
        bankruptcy_loss: Decimal,
        insurance_fund_balance: Decimal,
    ) -> bool {
        bankruptcy_loss > insurance_fund_balance
    }

    /// Calculate ADL ranking score for a position
    ///
    /// Higher score = more likely to be deleveraged
    /// Score = profit_ratio * leverage
    pub fn calculate_adl_score(
        &self,
        position: &Position,
        market: &Market,
        leverage: u8,
    ) -> Decimal {
        let pnl = position.unrealized_pnl(market.state.mark_price);
        let notional = position.notional_value(market.state.mark_price);

        if notional.is_zero() {
            return Decimal::from_raw(0, Decimal::RATE_DECIMALS);
        }

        // Profit ratio = unrealized_pnl / position_value
        let profit_ratio = pnl.to_decimals(Decimal::RATE_DECIMALS)
            / notional.to_decimals(Decimal::RATE_DECIMALS);

        // ADL score = profit_ratio * leverage
        profit_ratio.mul_int(leverage as i64)
    }
}

impl Default for LiquidationEngine {
    fn default() -> Self {
        Self::new(Decimal::rate("0.25"))
    }
}

/// Liquidation event data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiquidationEvent {
    pub account: AccountAddress,
    pub market_id: u8,
    pub timestamp: Timestamp,
    pub liquidated_size: Decimal,
    pub liquidation_price: Decimal,
    pub bankruptcy_price: Option<Decimal>,
    pub insurance_fund_delta: Decimal,
    pub is_bankruptcy: bool,
}

/// ADL (Auto-Deleverage) event data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdlEvent {
    pub bankrupt_account: AccountAddress,
    pub counter_account: AccountAddress,
    pub market_id: u8,
    pub timestamp: Timestamp,
    pub size: Decimal,
    pub price: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypercore_primitives::MarketConfig;

    fn setup() -> (LiquidationEngine, Market, Position) {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );
        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);
        (engine, market, position)
    }

    #[test]
    fn test_liquidation_size() {
        let (engine, _, position) = setup();

        let liq_size = engine.calculate_liquidation_size(&position);

        // 25% of 1.0 = 0.25
        assert_eq!(liq_size.to_string_trimmed(), "0.25");
    }

    #[test]
    fn test_liquidation_price_long() {
        let (engine, market, position) = setup();

        let liq_price = engine.calculate_liquidation_price(&position, &market);

        // Long liquidation: 65000 * (1 - 0.005) = 64675
        assert_eq!(liq_price.to_string_trimmed(), "64675");
    }

    #[test]
    fn test_liquidation_price_short() {
        let (engine, market, _) = setup();

        let mut short_position = Position::new();
        short_position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), false);

        let liq_price = engine.calculate_liquidation_price(&short_position, &market);

        // Short liquidation: 65000 * (1 + 0.005) = 65325
        assert_eq!(liq_price.to_string_trimmed(), "65325");
    }

    #[test]
    fn test_bankruptcy_price_long() {
        let engine = LiquidationEngine::default();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let balance = Decimal::usdc("1000");
        let bankruptcy = engine.calculate_bankruptcy_price(&position, balance);

        // Long: bankruptcy at 65000 - 1000/1 = 64000
        assert_eq!(bankruptcy.unwrap().to_string_trimmed(), "64000");
    }

    #[test]
    fn test_bankruptcy_price_short() {
        let engine = LiquidationEngine::default();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), false);

        let balance = Decimal::usdc("1000");
        let bankruptcy = engine.calculate_bankruptcy_price(&position, balance);

        // Short: bankruptcy at 65000 + 1000/1 = 66000
        assert_eq!(bankruptcy.unwrap().to_string_trimmed(), "66000");
    }

    #[test]
    fn test_insurance_contribution() {
        let engine = LiquidationEngine::default();

        let liq_size = Decimal::size("1.0");
        let liq_price = Decimal::price("64675");
        let fee_rate = Decimal::rate("0.005"); // 0.5%

        let contribution = engine.calculate_insurance_contribution(liq_size, liq_price, fee_rate);

        // 64675 * 0.005 = 323.375
        assert!(contribution.to_f64() > 323.0 && contribution.to_f64() < 324.0);
    }

    #[test]
    fn test_adl_score() {
        let (engine, market, _) = setup();

        // Position with profit
        let mut profitable = Position::new();
        profitable.apply_fill(Decimal::size("1.0"), Decimal::price("60000"), true);
        // At mark 65000: +5000 profit

        let score = engine.calculate_adl_score(&profitable, &market, 10);
        assert!(score.is_positive());

        // Position with loss
        let mut unprofitable = Position::new();
        unprofitable.apply_fill(Decimal::size("1.0"), Decimal::price("70000"), true);
        // At mark 65000: -5000 loss

        let score2 = engine.calculate_adl_score(&unprofitable, &market, 10);
        assert!(score2.is_negative());
    }

    // =========================================================================
    // NEW COMPREHENSIVE TESTS - Edge Cases and Real-World Scenarios
    // =========================================================================

    #[test]
    fn test_empty_position_returns_none() {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );
        let empty_position = Position::new();
        let account = AccountAddress::default();

        // Empty position should return None for liquidation
        let result = engine.process_liquidation(account, &empty_position, &market, 0);
        assert!(result.is_none());

        // Empty position should return None for bankruptcy price
        let bankruptcy = engine.calculate_bankruptcy_price(&empty_position, Decimal::usdc("1000"));
        assert!(bankruptcy.is_none());
    }

    #[test]
    fn test_small_position_liquidation_size() {
        let engine = LiquidationEngine::default();

        // Very small position (0.001 BTC)
        let mut small_position = Position::new();
        small_position.apply_fill(Decimal::size("0.001"), Decimal::price("65000"), true);

        let liq_size = engine.calculate_liquidation_size(&small_position);

        // 25% of 0.001 = 0.00025, but should be capped at full position
        assert!(liq_size <= small_position.abs_size());
        assert!(liq_size.is_positive());
    }

    #[test]
    fn test_large_position_partial_liquidation() {
        let engine = LiquidationEngine::default();

        // Large position (100 BTC)
        let mut large_position = Position::new();
        large_position.apply_fill(Decimal::size("100.0"), Decimal::price("65000"), true);

        let liq_size = engine.calculate_liquidation_size(&large_position);

        // 25% of 100 = 25 BTC
        assert_eq!(liq_size.to_string_trimmed(), "25");
    }

    #[test]
    fn test_custom_partial_ratio() {
        // Custom 50% partial liquidation
        let engine = LiquidationEngine::new(Decimal::rate("0.5"));

        let mut position = Position::new();
        position.apply_fill(Decimal::size("10.0"), Decimal::price("65000"), true);

        let liq_size = engine.calculate_liquidation_size(&position);

        // 50% of 10 = 5 BTC
        assert_eq!(liq_size.to_string_trimmed(), "5");
    }

    #[test]
    fn test_bankruptcy_price_with_large_balance() {
        let engine = LiquidationEngine::default();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Large balance relative to position
        let balance = Decimal::usdc("50000");
        let bankruptcy = engine.calculate_bankruptcy_price(&position, balance);

        // Long: bankruptcy at 65000 - 50000/1 = 15000
        assert_eq!(bankruptcy.unwrap().to_string_trimmed(), "15000");
    }

    #[test]
    fn test_bankruptcy_price_with_tiny_balance() {
        let engine = LiquidationEngine::default();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Tiny balance - near liquidation
        let balance = Decimal::usdc("100");
        let bankruptcy = engine.calculate_bankruptcy_price(&position, balance);

        // Long: bankruptcy at 65000 - 100/1 = 64900
        assert_eq!(bankruptcy.unwrap().to_string_trimmed(), "64900");
    }

    #[test]
    fn test_adl_trigger_threshold() {
        let engine = LiquidationEngine::default();

        // Insurance fund can cover the loss
        assert!(!engine.should_trigger_adl(
            Decimal::usdc("1000"),  // bankruptcy loss
            Decimal::usdc("5000"),  // insurance fund
        ));

        // Insurance fund cannot cover the loss
        assert!(engine.should_trigger_adl(
            Decimal::usdc("10000"), // bankruptcy loss
            Decimal::usdc("5000"),  // insurance fund
        ));

        // Exact boundary - loss equals insurance
        assert!(!engine.should_trigger_adl(
            Decimal::usdc("5000"),  // bankruptcy loss
            Decimal::usdc("5000"),  // insurance fund
        ));
    }

    #[test]
    fn test_adl_score_with_different_leverages() {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Profitable position
        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("60000"), true);
        // At mark 65000: +5000 profit

        // Higher leverage = higher ADL score (more likely to be deleveraged)
        let score_5x = engine.calculate_adl_score(&position, &market, 5);
        let score_20x = engine.calculate_adl_score(&position, &market, 20);

        assert!(score_20x > score_5x);
    }

    #[test]
    fn test_adl_score_with_zero_notional() {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("0"), // Zero mark price
            0,
        );

        let empty_position = Position::new();

        // Should return zero ADL score for empty position
        let score = engine.calculate_adl_score(&empty_position, &market, 10);
        assert!(score.is_zero());
    }

    #[test]
    fn test_liquidation_spread_long_vs_short() {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Long position
        let mut long_position = Position::new();
        long_position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Short position
        let mut short_position = Position::new();
        short_position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), false);

        let long_liq_price = engine.calculate_liquidation_price(&long_position, &market);
        let short_liq_price = engine.calculate_liquidation_price(&short_position, &market);

        // Long liquidates below mark, short liquidates above mark
        assert!(long_liq_price < Decimal::price("65000"));
        assert!(short_liq_price > Decimal::price("65000"));

        // Spread should be symmetric (0.5% each side)
        let long_spread = (Decimal::price("65000").to_f64() - long_liq_price.to_f64()) / 65000.0;
        let short_spread = (short_liq_price.to_f64() - Decimal::price("65000").to_f64()) / 65000.0;

        // Both spreads should be approximately 0.5%
        assert!((long_spread - 0.005).abs() < 0.0001);
        assert!((short_spread - 0.005).abs() < 0.0001);
    }

    #[test]
    fn test_process_liquidation_returns_values() {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );
        let account = AccountAddress::default();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("2.0"), Decimal::price("65000"), true);

        let result = engine.process_liquidation(account, &position, &market, 1234567890);

        assert!(result.is_some());
        let (liq_size, liq_price) = result.unwrap();

        // Should return 25% of position at liquidation price
        assert_eq!(liq_size.to_string_trimmed(), "0.5"); // 25% of 2.0
        assert_eq!(liq_price.to_string_trimmed(), "64675"); // 65000 * 0.995
    }

    #[test]
    fn test_insurance_contribution_with_varying_fee_rates() {
        let engine = LiquidationEngine::default();

        let liq_size = Decimal::size("1.0");
        let liq_price = Decimal::price("65000");

        // 0.1% fee
        let fee_01 = Decimal::rate("0.001");
        let contrib_01 = engine.calculate_insurance_contribution(liq_size, liq_price, fee_01);

        // 1% fee
        let fee_1 = Decimal::rate("0.01");
        let contrib_1 = engine.calculate_insurance_contribution(liq_size, liq_price, fee_1);

        // 1% fee should be 10x the 0.1% fee
        let ratio = contrib_1.to_f64() / contrib_01.to_f64();
        assert!((ratio - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_multiple_partial_liquidations_scenario() {
        let engine = LiquidationEngine::default();

        // Start with 10 BTC position
        let mut position = Position::new();
        position.apply_fill(Decimal::size("10.0"), Decimal::price("65000"), true);

        // First liquidation: 25% of 10 = 2.5 BTC
        let liq1 = engine.calculate_liquidation_size(&position);
        assert_eq!(liq1.to_string_trimmed(), "2.5");

        // Simulate reducing position after liquidation
        position.apply_fill(Decimal::size("2.5"), Decimal::price("64675"), false);

        // Second liquidation: 25% of 7.5 = 1.875 BTC
        let liq2 = engine.calculate_liquidation_size(&position);
        assert_eq!(liq2.to_string_trimmed(), "1.875");

        // Third liquidation
        position.apply_fill(Decimal::size("1.875"), Decimal::price("64675"), false);

        // 25% of 5.625 = 1.40625 BTC
        let liq3 = engine.calculate_liquidation_size(&position);
        assert!((liq3.to_f64() - 1.40625).abs() < 0.001);
    }

    #[test]
    fn test_high_leverage_bankruptcy_scenario() {
        let engine = LiquidationEngine::default();

        // 50x leverage position: 1 BTC at $65k = $65k notional
        // Initial margin at 50x = $1,300
        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let balance = Decimal::usdc("1300");
        let bankruptcy = engine.calculate_bankruptcy_price(&position, balance);

        // Bankruptcy at 65000 - 1300/1 = 63700
        // At this price, equity = 1300 - (65000 - 63700) = 0
        assert_eq!(bankruptcy.unwrap().to_string_trimmed(), "63700");
    }

    #[test]
    fn test_adl_score_ranking() {
        let engine = LiquidationEngine::default();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Three positions with different entry prices
        let mut pos_best = Position::new();
        pos_best.apply_fill(Decimal::size("1.0"), Decimal::price("55000"), true);
        // PnL: +10000 (best profit)

        let mut pos_mid = Position::new();
        pos_mid.apply_fill(Decimal::size("1.0"), Decimal::price("60000"), true);
        // PnL: +5000

        let mut pos_worst = Position::new();
        pos_worst.apply_fill(Decimal::size("1.0"), Decimal::price("70000"), true);
        // PnL: -5000 (loss)

        let score_best = engine.calculate_adl_score(&pos_best, &market, 10);
        let score_mid = engine.calculate_adl_score(&pos_mid, &market, 10);
        let score_worst = engine.calculate_adl_score(&pos_worst, &market, 10);

        // Higher profit ratio = higher ADL score
        assert!(score_best > score_mid);
        assert!(score_mid > score_worst);
    }
}
