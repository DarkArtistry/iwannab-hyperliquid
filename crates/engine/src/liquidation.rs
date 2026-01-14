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
}
