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
    use hypercore_primitives::{MarketConfig, OrderSide, OrderType, TimeInForce};

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

    // =========================================================================
    // NEW COMPREHENSIVE TESTS - Edge Cases and Real-World Scenarios
    // =========================================================================

    #[test]
    fn test_equity_with_multiple_positions() {
        let (engine, _, account) = setup();

        // BTC market at 65000
        let btc_market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // ETH market at 3500
        let eth_market = Market::new(
            MarketConfig::eth_perp(),
            Decimal::price("3500"),
            1,
        );

        // Long 1 BTC from 64000 -> +1000 profit
        let mut btc_position = Position::new();
        btc_position.apply_fill(Decimal::size("1.0"), Decimal::price("64000"), true);

        // Short 10 ETH from 3400 -> -1000 loss (shorted at lower price)
        let mut eth_position = Position::new();
        eth_position.apply_fill(Decimal::size("10.0"), Decimal::price("3400"), false);
        // Short from 3400, mark at 3500 -> -1000 loss

        let positions = vec![
            (btc_position, &btc_market),
            (eth_position, &eth_market),
        ];

        let equity = engine.calculate_equity(&account, &positions);

        // 10000 + 1000 (BTC profit) - 1000 (ETH loss) = 10000
        assert_eq!(equity.to_string_trimmed(), "10000");
    }

    #[test]
    fn test_zero_balance_account() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        let account = AccountState {
            balance: 0,
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        // Empty position, zero balance
        let positions: Vec<(Position, &Market)> = vec![];
        let equity = engine.calculate_equity(&account, &positions);

        assert_eq!(equity.to_string_trimmed(), "0");
    }

    #[test]
    fn test_negative_equity_from_large_loss() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        // Long 1 BTC at 80000, mark at 65000 -> -15000 loss
        position.apply_fill(Decimal::size("1.0"), Decimal::price("80000"), true);

        let positions = vec![(position, &market)];
        let equity = engine.calculate_equity(&account, &positions);

        // 10000 - 15000 = -5000
        assert_eq!(equity.to_string_trimmed(), "-5000");
        assert!(equity.is_negative());
    }

    #[test]
    fn test_initial_margin_with_max_leverage() {
        let (engine, market, _) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // 50x leverage (max for BTC)
        let positions = vec![(position, &market, 50u8)];
        let margin = engine.calculate_initial_margin(&positions);

        // 65000 notional / 50 leverage = 1300 margin
        assert_eq!(margin.to_string_trimmed(), "1300");
    }

    #[test]
    fn test_initial_margin_with_min_leverage() {
        let (engine, market, _) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // 1x leverage (no leverage)
        let positions = vec![(position, &market, 1u8)];
        let margin = engine.calculate_initial_margin(&positions);

        // 65000 notional / 1 leverage = 65000 margin (full notional)
        assert_eq!(margin.to_string_trimmed(), "65000");
    }

    #[test]
    fn test_liquidatable_with_exact_maintenance_margin() {
        let (engine, market, mut account) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Maintenance margin = 65000 * 0.025 = 1625
        // Set balance exactly at maintenance margin
        account.balance = 1625_000000;

        // Equity equals maintenance -> liquidatable (equity <= maintenance)
        assert!(engine.is_liquidatable(&account, &position, &market));
    }

    #[test]
    fn test_liquidatable_slightly_above_maintenance() {
        let (engine, market, mut account) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Maintenance margin = 1625, set balance slightly above
        account.balance = 1626_000000;

        // Equity > maintenance -> not liquidatable
        assert!(!engine.is_liquidatable(&account, &position, &market));
    }

    #[test]
    fn test_empty_position_not_liquidatable() {
        let (engine, market, account) = setup();

        let empty_position = Position::new();

        // Empty position should never be liquidatable
        assert!(!engine.is_liquidatable(&account, &empty_position, &market));
    }

    #[test]
    fn test_free_collateral_with_no_position() {
        let (engine, market, account) = setup();

        let positions: Vec<(Position, &Market, u8)> = vec![];
        let free = engine.calculate_free_collateral(&account, &positions);

        // No positions = all balance is free
        assert_eq!(free.to_string_trimmed(), "10000");
    }

    #[test]
    fn test_free_collateral_fully_utilized() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Account with exactly $6500 (enough for 1 BTC at 10x)
        let account = AccountState {
            balance: 6500_000000,
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let positions = vec![(position, &market, 10u8)];
        let free = engine.calculate_free_collateral(&account, &positions);

        // Equity = 6500, margin = 6500, free = 0
        assert_eq!(free.to_string_trimmed(), "0");
    }

    #[test]
    fn test_free_collateral_negative() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Account with only $5000
        let account = AccountState {
            balance: 5000_000000,
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let positions = vec![(position, &market, 10u8)];
        let free = engine.calculate_free_collateral(&account, &positions);

        // Equity = 5000, margin = 6500, free = -1500
        assert_eq!(free.to_string_trimmed(), "-1500");
        assert!(free.is_negative());
    }

    #[test]
    fn test_margin_summary_comprehensive() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        // Long 0.5 BTC at 64000, mark at 65000 -> +500 profit
        position.apply_fill(Decimal::size("0.5"), Decimal::price("64000"), true);

        let positions = vec![(position.clone(), &market, 10u8)];
        let summary = engine.calculate_margin_summary(&account, &positions);

        // Account value = balance + unrealized PnL = 10000 + 500 = 10500
        let account_value = Decimal::from_raw(summary.account_value, Decimal::USDC_DECIMALS);
        assert_eq!(account_value.to_string_trimmed(), "10500");

        // Position value = 0.5 * 65000 = 32500
        let position_value = Decimal::from_raw(summary.total_position_value as i128, Decimal::USDC_DECIMALS);
        assert_eq!(position_value.to_string_trimmed(), "32500");

        // Initial margin = 32500 / 10 = 3250
        let init_margin = Decimal::from_raw(summary.total_initial_margin as i128, Decimal::USDC_DECIMALS);
        assert_eq!(init_margin.to_string_trimmed(), "3250");

        // Maintenance margin = 32500 * 0.025 = 812.5
        let maint_margin = Decimal::from_raw(summary.total_maintenance_margin as i128, Decimal::USDC_DECIMALS);
        assert!((maint_margin.to_f64() - 812.5).abs() < 1.0);
    }

    #[test]
    fn test_margin_ratio_healthy() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("0.1"), Decimal::price("65000"), true);

        let positions = vec![(position, &market)];
        let ratio = engine.margin_ratio(&account, &positions);

        assert!(ratio.is_some());
        // Equity = 10000, maintenance = 162.5, ratio = ~61.5x
        let r = ratio.unwrap();
        assert!(r.to_f64() > 50.0);
    }

    #[test]
    fn test_margin_ratio_near_liquidation() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Very low balance
        let account = AccountState {
            balance: 2000_000000,
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let positions = vec![(position, &market)];
        let ratio = engine.margin_ratio(&account, &positions);

        assert!(ratio.is_some());
        // Equity = 2000, maintenance = 1625, ratio = ~1.23x
        let r = ratio.unwrap();
        assert!(r.to_f64() > 1.0 && r.to_f64() < 2.0);
    }

    #[test]
    fn test_margin_ratio_no_position() {
        let (engine, _, account) = setup();

        let positions: Vec<(Position, &Market)> = vec![];
        let ratio = engine.margin_ratio(&account, &positions);

        // No positions = zero maintenance margin = None (division by zero)
        assert!(ratio.is_none());
    }

    #[test]
    fn test_check_order_margin_sufficient() {
        let (engine, market, account) = setup();

        let empty_position = Position::new();
        let request = OrderRequest {
            market_id: 0,
            size: Decimal::size("0.1"),
            price: Decimal::price("65000"),
            side: OrderSide::Buy,
            order_type: OrderType::Limit { tif: TimeInForce::Gtc },
            reduce_only: false,
            client_order_id: None,
        };

        // 10x leverage: notional 6500, margin needed = 650
        // Account has 10000, plenty of margin
        let result = engine.check_order_margin(&request, &account, &empty_position, &market, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_order_margin_insufficient() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Very low balance
        let account = AccountState {
            balance: 100_000000, // Only $100
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        let empty_position = Position::new();
        let request = OrderRequest {
            market_id: 0,
            size: Decimal::size("1.0"),
            price: Decimal::price("65000"),
            side: OrderSide::Buy,
            order_type: OrderType::Limit { tif: TimeInForce::Gtc },
            reduce_only: false,
            client_order_id: None,
        };

        // 10x leverage: notional 65000, margin needed = 6500
        // Account has only 100
        let result = engine.check_order_margin(&request, &account, &empty_position, &market, 10);
        assert!(result.is_err());

        match result {
            Err(Error::InsufficientMargin { required, available }) => {
                assert!(required.parse::<f64>().unwrap() > 0.0);
                assert!(available.parse::<f64>().unwrap() < required.parse::<f64>().unwrap());
            }
            _ => panic!("Expected InsufficientMargin error"),
        }
    }

    #[test]
    fn test_check_leverage_change_safe() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("0.1"), Decimal::price("65000"), true);

        // Current at 10x, change to 20x (requires less margin)
        let result = engine.check_leverage_change(&account, &position, &market, 20);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_leverage_change_would_liquidate() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Low balance account
        let account = AccountState {
            balance: 5000_000000, // $5000
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);
        // Notional = 65000

        // Trying to lower leverage from 20x to 5x would require more margin
        // At 5x: margin = 65000/5 = 13000 > 5000 balance
        let result = engine.check_leverage_change(&account, &position, &market, 5);
        assert!(result.is_err());

        match result {
            Err(Error::LeverageWouldCauseLiquidation) => {}
            _ => panic!("Expected LeverageWouldCauseLiquidation error"),
        }
    }

    #[test]
    fn test_short_position_pnl_calculation() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        // Short 1 BTC at 66000, mark at 65000 -> +1000 profit
        position.apply_fill(Decimal::size("1.0"), Decimal::price("66000"), false);

        let positions = vec![(position, &market)];
        let equity = engine.calculate_equity(&account, &positions);

        // 10000 + 1000 profit = 11000
        assert_eq!(equity.to_string_trimmed(), "11000");
    }

    #[test]
    fn test_short_position_loss() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        // Short 1 BTC at 64000, mark at 65000 -> -1000 loss
        position.apply_fill(Decimal::size("1.0"), Decimal::price("64000"), false);

        let positions = vec![(position, &market)];
        let equity = engine.calculate_equity(&account, &positions);

        // 10000 - 1000 loss = 9000
        assert_eq!(equity.to_string_trimmed(), "9000");
    }

    #[test]
    fn test_high_leverage_margin_requirements() {
        let (engine, market, _) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Compare margin requirements at different leverages
        let margin_1x = engine.calculate_initial_margin(&vec![(position.clone(), &market, 1u8)]);
        let margin_10x = engine.calculate_initial_margin(&vec![(position.clone(), &market, 10u8)]);
        let margin_50x = engine.calculate_initial_margin(&vec![(position.clone(), &market, 50u8)]);

        // Higher leverage = lower margin requirement
        assert!(margin_1x > margin_10x);
        assert!(margin_10x > margin_50x);

        // Verify ratios
        let ratio_10x = margin_1x.to_f64() / margin_10x.to_f64();
        let ratio_50x = margin_1x.to_f64() / margin_50x.to_f64();

        assert!((ratio_10x - 10.0).abs() < 0.1);
        assert!((ratio_50x - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_withdrawable_amount() {
        let (engine, market, account) = setup();

        let mut position = Position::new();
        position.apply_fill(Decimal::size("0.1"), Decimal::price("65000"), true);

        let positions = vec![(position, &market, 10u8)];
        let summary = engine.calculate_margin_summary(&account, &positions);

        let withdrawable = Decimal::from_raw(summary.withdrawable as i128, Decimal::USDC_DECIMALS);
        let free_collateral = Decimal::from_raw(summary.free_collateral, Decimal::USDC_DECIMALS);

        // Withdrawable should be positive and <= free collateral
        assert!(withdrawable.is_positive() || withdrawable.is_zero());
        assert!(withdrawable <= free_collateral);
    }

    #[test]
    fn test_withdrawable_with_negative_free_collateral() {
        let engine = RiskEngine::new();
        let market = Market::new(
            MarketConfig::btc_perp(),
            Decimal::price("65000"),
            0,
        );

        // Low balance leading to negative free collateral
        let account = AccountState {
            balance: 5000_000000, // $5000
            nonce: 0,
            last_timestamp_nonce: 0,
        };

        let mut position = Position::new();
        position.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);
        // Margin at 10x = 6500 > 5000 balance

        let positions = vec![(position, &market, 10u8)];
        let summary = engine.calculate_margin_summary(&account, &positions);

        let withdrawable = Decimal::from_raw(summary.withdrawable as i128, Decimal::USDC_DECIMALS);

        // Negative free collateral = zero withdrawable
        assert_eq!(withdrawable.to_string_trimmed(), "0");
    }
}
