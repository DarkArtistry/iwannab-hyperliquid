//! Risk and margin calculations

use hypercore_primitives::{
    AccountState, Decimal, Error, Market, MarginSummary, OrderRequest, Position, Result,
};

/// Risk engine for margin calculations and validation
pub struct RiskEngine {
    // Configuration could be added here
}

impl RiskEngine {
    /// Create a new risk engine
    pub fn new() -> Self {
        Self {}
    }

    /// Calculate account equity
    pub fn calculate_equity(
        &self,
        account: &AccountState,
        positions: &[(Position, &Market)],
    ) -> Decimal {
        let balance = Decimal::from_raw(account.balance, Decimal::USDC_DECIMALS);

        let total_unrealized_pnl: Decimal = positions
            .iter()
            .map(|(pos, market)| pos.unrealized_pnl(market.state.mark_price))
            .fold(Decimal::from_raw(0, Decimal::USDC_DECIMALS), |acc, pnl| {
                acc + pnl.to_decimals(Decimal::USDC_DECIMALS)
            });

        balance + total_unrealized_pnl
    }

    /// Calculate total initial margin required
    pub fn calculate_initial_margin(
        &self,
        positions: &[(Position, &Market, u8)], // (position, market, leverage)
    ) -> Decimal {
        positions
            .iter()
            .map(|(pos, market, leverage)| {
                market
                    .config
                    .initial_margin(pos.size, market.state.mark_price, *leverage)
            })
            .fold(Decimal::from_raw(0, Decimal::USDC_DECIMALS), |acc, m| {
                acc + m.to_decimals(Decimal::USDC_DECIMALS)
            })
    }

    /// Calculate total maintenance margin required
    pub fn calculate_maintenance_margin(
        &self,
        positions: &[(Position, &Market)],
    ) -> Decimal {
        positions
            .iter()
            .map(|(pos, market)| {
                market
                    .config
                    .maintenance_margin(pos.size, market.state.mark_price)
            })
            .fold(Decimal::from_raw(0, Decimal::USDC_DECIMALS), |acc, m| {
                acc + m.to_decimals(Decimal::USDC_DECIMALS)
            })
    }

    /// Calculate free collateral
    pub fn calculate_free_collateral(
        &self,
        account: &AccountState,
        positions: &[(Position, &Market, u8)],
    ) -> Decimal {
        let positions_for_equity: Vec<_> = positions
            .iter()
            .map(|(p, m, _)| (p.clone(), *m))
            .collect();

        let equity = self.calculate_equity(account, &positions_for_equity);
        let initial_margin = self.calculate_initial_margin(positions);

        equity - initial_margin.to_decimals(equity.decimals())
    }

    /// Calculate full margin summary
    pub fn calculate_margin_summary(
        &self,
        account: &AccountState,
        positions: &[(Position, &Market, u8)],
    ) -> MarginSummary {
        let positions_for_equity: Vec<_> = positions
            .iter()
            .map(|(p, m, _)| (p.clone(), *m))
            .collect();

        let equity = self.calculate_equity(account, &positions_for_equity);
        let initial_margin = self.calculate_initial_margin(positions);
        let maintenance_margin = self.calculate_maintenance_margin(&positions_for_equity);

        let total_position_value: Decimal = positions
            .iter()
            .map(|(pos, market, _)| pos.notional_value(market.state.mark_price))
            .fold(Decimal::from_raw(0, Decimal::USDC_DECIMALS), |acc, v| {
                acc + v.to_decimals(Decimal::USDC_DECIMALS)
            });

        let free_collateral = equity - initial_margin.to_decimals(equity.decimals());

        let withdrawable = if free_collateral.is_positive() {
            let balance = Decimal::from_raw(account.balance, Decimal::USDC_DECIMALS);
            free_collateral.min(balance.max(Decimal::from_raw(0, Decimal::USDC_DECIMALS)))
        } else {
            Decimal::from_raw(0, Decimal::USDC_DECIMALS)
        };

        MarginSummary {
            account_value: equity.raw(),
            total_position_value: total_position_value.raw() as u128,
            total_initial_margin: initial_margin.raw() as u128,
            total_maintenance_margin: maintenance_margin.raw() as u128,
            free_collateral: free_collateral.raw(),
            withdrawable: withdrawable.raw() as u128,
        }
    }

    /// Check if an account is liquidatable
    pub fn is_liquidatable(
        &self,
        account: &AccountState,
        position: &Position,
        market: &Market,
    ) -> bool {
        if position.is_empty() {
            return false;
        }

        let positions = vec![(position.clone(), market)];
        let equity = self.calculate_equity(account, &positions);
        let maintenance_margin = self.calculate_maintenance_margin(&positions);

        equity <= maintenance_margin.to_decimals(equity.decimals())
    }

    /// Check if an order can be placed with available margin
    pub fn check_order_margin(
        &self,
        request: &OrderRequest,
        account: &AccountState,
        position: &Position,
        market: &Market,
        leverage: u8,
    ) -> Result<()> {
        // Calculate margin required for this order
        let order_notional = request.size * request.price;
        let order_margin = order_notional / Decimal::from_int(leverage as i64, order_notional.decimals());

        // Get current margin usage
        let current_positions = vec![(position.clone(), market, leverage)];
        let free_collateral = self.calculate_free_collateral(account, &current_positions);

        if order_margin.to_decimals(free_collateral.decimals()) > free_collateral {
            return Err(Error::InsufficientMargin {
                required: order_margin.to_string_trimmed(),
                available: free_collateral.to_string_trimmed(),
            });
        }

        // Check max position limit
        let new_position_notional = position.notional_value(market.state.mark_price) + order_notional;
        if new_position_notional > market.config.max_position_notional {
            return Err(Error::MaxPositionExceeded {
                max_notional: market.config.max_position_notional.to_string_trimmed(),
            });
        }

        Ok(())
    }

    /// Check if leverage change would cause liquidation
    pub fn check_leverage_change(
        &self,
        account: &AccountState,
        position: &Position,
        market: &Market,
        new_leverage: u8,
    ) -> Result<()> {
        // Calculate new initial margin requirement
        let new_initial_margin = market.config.initial_margin(
            position.size,
            market.state.mark_price,
            new_leverage,
        );

        // Get current equity
        let positions = vec![(position.clone(), market)];
        let equity = self.calculate_equity(account, &positions);

        if new_initial_margin.to_decimals(equity.decimals()) > equity {
            return Err(Error::LeverageWouldCauseLiquidation);
        }

        Ok(())
    }

    /// Calculate margin ratio (equity / maintenance margin)
    pub fn margin_ratio(
        &self,
        account: &AccountState,
        positions: &[(Position, &Market)],
    ) -> Option<Decimal> {
        let equity = self.calculate_equity(account, positions);
        let maintenance = self.calculate_maintenance_margin(positions);

        if maintenance.is_zero() {
            return None;
        }

        Some(equity / maintenance.to_decimals(equity.decimals()))
    }
}

impl Default for RiskEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypercore_primitives::MarketConfig;

    fn setup() -> (RiskEngine, Market, AccountState) {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );
        let account = AccountState {
            balance: 10000_000000, // $10,000
            nonce: 0,
            last_timestamp_nonce: 0,
        };
        (engine, market, account)
    }

    #[test]
    fn test_equity_no_position() {
        let (engine, market, account) = setup();
        let positions = vec![];

        let equity = engine.calculate_equity(&account, &positions);
        assert_eq!(equity.to_string_trimmed(), "10000");
    }

    #[test]
    fn test_equity_with_profit() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        // Long 1 BTC at 64000, mark at 65000 -> +1000 profit
        position.apply_fill(Decimal::size("1.0"), Decimal::price("64000"), true);

        let positions = vec![(position, &market)];
        let equity = engine.calculate_equity(&account, &positions);

        // 10000 + 1000 unrealized = 11000
        assert_eq!(equity.to_string_trimmed(), "11000");
    }

    #[test]
    fn test_equity_with_loss() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        // Long 1 BTC at 66000, mark at 65000 -> -1000 loss
        position.apply_fill(Decimal::size("1.0"), Decimal::price("66000"), true);

        let positions = vec![(position, &market)];
        let equity = engine.calculate_equity(&account, &positions);

        // 10000 - 1000 unrealized = 9000
        assert_eq!(equity.to_string_trimmed(), "9000");
    }

    #[test]
    fn test_initial_margin() {
        let (engine, market, _) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let positions = vec![(position, &market, 10u8)]; // 10x leverage
        let margin = engine.calculate_initial_margin(&positions);

        // 65000 notional / 10 leverage = 6500 margin
        assert_eq!(margin.to_string_trimmed(), "6500");
    }

    #[test]
    fn test_maintenance_margin() {
        let (engine, market, _) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let positions = vec![(position, &market)];
        let margin = engine.calculate_maintenance_margin(&positions);

        // 65000 notional * 2.5% = 1625
        assert_eq!(margin.to_string_trimmed(), "1625");
    }

    #[test]
    fn test_liquidatable() {
        let (engine, market, mut account) = setup();

        let mut position = Position::new();
        // Large position relative to balance
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Account with only $2000 and 1 BTC at 65k
        // Maintenance = 65000 * 0.025 = 1625
        // If mark drops to 63000, PnL = -2000, equity = 0
        account.balance = 2000_000000;

        // At mark price 65000, equity = 2000, maintenance = 1625 -> not liquidatable
        assert!(!engine.is_liquidatable(&account, &position, &market));

        // If we simulate mark price drop (would need to update market)
        // This test just verifies the function works at current price
    }

    #[test]
    fn test_free_collateral() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("0.1"), Decimal::price("65000"), true);

        let positions = vec![(position, &market, 10u8)];
        let free = engine.calculate_free_collateral(&account, &positions);

        // Equity = 10000 (no PnL at entry price)
        // Initial margin = 6500 / 10 = 650
        // Free = 10000 - 650 = 9350
        assert_eq!(free.to_string_trimmed(), "9350");
    }
}
