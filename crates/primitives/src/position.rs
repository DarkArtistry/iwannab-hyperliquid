//! Position types and calculations

use crate::{AccountAddress, Decimal, SignedAmount, Timestamp};
use serde::{Deserialize, Serialize};

/// Position in a perpetual market
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Position {
    /// Position size (positive = long, negative = short)
    pub size: Decimal,
    /// Cumulative entry notional (for entry price calculation)
    pub entry_notional: Decimal,
    /// Realized PnL (settled)
    pub realized_pnl: Decimal,
    /// Last funding index (for lazy funding settlement)
    pub last_funding_index: Decimal,
}

impl Position {
    /// Create a new empty position
    pub fn new() -> Self {
        Self {
            size: Decimal::from_raw(0, Decimal::SIZE_DECIMALS),
            entry_notional: Decimal::from_raw(0, Decimal::SIZE_DECIMALS),
            realized_pnl: Decimal::from_raw(0, Decimal::USDC_DECIMALS),
            last_funding_index: Decimal::from_raw(0, Decimal::RATE_DECIMALS),
        }
    }

    /// Check if position is empty (no open position)
    pub fn is_empty(&self) -> bool {
        self.size.is_zero()
    }

    /// Check if position is long
    pub fn is_long(&self) -> bool {
        self.size.is_positive()
    }

    /// Check if position is short
    pub fn is_short(&self) -> bool {
        self.size.is_negative()
    }

    /// Get absolute size
    pub fn abs_size(&self) -> Decimal {
        self.size.abs()
    }

    /// Calculate average entry price
    pub fn entry_price(&self) -> Option<Decimal> {
        if self.size.is_zero() {
            return None;
        }
        Some(self.entry_notional / self.abs_size())
    }

    /// Calculate unrealized PnL at a given mark price
    pub fn unrealized_pnl(&self, mark_price: Decimal) -> Decimal {
        if self.size.is_zero() {
            return Decimal::from_raw(0, Decimal::USDC_DECIMALS);
        }

        let current_notional = self.abs_size() * mark_price;

        if self.is_long() {
            // Long: profit when price goes up
            current_notional - self.entry_notional
        } else {
            // Short: profit when price goes down
            self.entry_notional - current_notional
        }
    }

    /// Calculate position notional value
    pub fn notional_value(&self, mark_price: Decimal) -> Decimal {
        self.abs_size() * mark_price
    }

    /// Calculate liquidation price
    /// Returns None if position is empty or math would overflow
    pub fn liquidation_price(&self, balance: Decimal, maintenance_margin_rate: Decimal) -> Option<Decimal> {
        if self.size.is_zero() {
            return None;
        }

        // Liquidation occurs when:
        // equity = maintenance_margin
        // balance + unrealized_pnl = notional * maintenance_margin_rate
        //
        // For long:
        // balance + size * (liq_price - entry_price) = size * liq_price * mm_rate
        // balance + size * liq_price - entry_notional = size * liq_price * mm_rate
        // size * liq_price * (1 - mm_rate) = entry_notional - balance
        // liq_price = (entry_notional - balance) / (size * (1 - mm_rate))
        //
        // For short:
        // balance + size * (entry_price - liq_price) = |size| * liq_price * mm_rate
        // balance + entry_notional - |size| * liq_price = |size| * liq_price * mm_rate
        // entry_notional + balance = |size| * liq_price * (1 + mm_rate)
        // liq_price = (entry_notional + balance) / (|size| * (1 + mm_rate))

        let one = Decimal::from_int(1, maintenance_margin_rate.decimals());

        if self.is_long() {
            let factor = one - maintenance_margin_rate;
            if factor.is_zero() {
                return None;
            }
            let numerator = self.entry_notional - balance.to_decimals(self.entry_notional.decimals());
            if numerator.is_negative() {
                // Over-collateralized, no liquidation price
                return Some(Decimal::from_raw(0, Decimal::PRICE_DECIMALS));
            }
            Some(numerator / (self.size * factor))
        } else {
            let factor = one + maintenance_margin_rate;
            let numerator = self.entry_notional + balance.to_decimals(self.entry_notional.decimals());
            Some(numerator / (self.abs_size() * factor))
        }
    }

    /// Calculate realized PnL from a fill WITHOUT modifying state
    ///
    /// Returns the PnL that would be realized if this fill were applied.
    /// Returns 0 if the fill would increase the position (no realization).
    pub fn calculate_fill_pnl(&self, fill_size: Decimal, fill_price: Decimal, is_buy: bool) -> Decimal {
        if self.size.is_zero() {
            // Opening new position - no realized PnL
            return Decimal::from_raw(0, Decimal::USDC_DECIMALS);
        }

        let fill_side_size = if is_buy { fill_size } else { -fill_size };
        let new_size = self.size + fill_side_size;
        let fill_notional = fill_size * fill_price;

        // Check if we're increasing, reducing, or flipping
        let same_sign = (self.size.is_positive() && new_size.is_positive())
            || (self.size.is_negative() && new_size.is_negative())
            || new_size.is_zero();

        if same_sign {
            // Check if increasing or reducing
            if self.size.raw().signum() == fill_side_size.raw().signum() {
                // Increasing position - no realized PnL
                Decimal::from_raw(0, Decimal::USDC_DECIMALS)
            } else {
                // Reducing position - calculate PnL on closed portion
                let close_ratio = fill_size / self.abs_size();
                let closed_notional = self.entry_notional * close_ratio;

                let pnl = if self.is_long() {
                    fill_notional - closed_notional
                } else {
                    closed_notional - fill_notional
                };

                pnl.to_decimals(Decimal::USDC_DECIMALS)
            }
        } else {
            // Flipping position - realize PnL on closing old position
            self.unrealized_pnl(fill_price).to_decimals(Decimal::USDC_DECIMALS)
        }
    }

    /// Update position after a fill
    pub fn apply_fill(&mut self, fill_size: Decimal, fill_price: Decimal, is_buy: bool) {
        let fill_side_size = if is_buy {
            fill_size
        } else {
            -fill_size
        };

        let new_size = self.size + fill_side_size;
        let fill_notional = fill_size * fill_price;

        // Check if we're increasing, reducing, or flipping
        let same_sign = (self.size.is_positive() && new_size.is_positive())
            || (self.size.is_negative() && new_size.is_negative())
            || self.size.is_zero()
            || new_size.is_zero();

        if same_sign {
            if self.size.is_zero() || self.size.raw().signum() == fill_side_size.raw().signum() {
                // Increasing position
                self.entry_notional = self.entry_notional + fill_notional;
            } else {
                // Reducing position
                let close_ratio = fill_size / self.abs_size();
                let closed_notional = self.entry_notional * close_ratio;

                let pnl = if self.is_long() {
                    fill_notional - closed_notional
                } else {
                    closed_notional - fill_notional
                };

                self.realized_pnl = self.realized_pnl + pnl.to_decimals(self.realized_pnl.decimals());
                self.entry_notional = self.entry_notional - closed_notional;
            }
        } else {
            // Flipping position: close old, open new
            // First, realize PnL on closing old position
            let close_pnl = self.unrealized_pnl(fill_price);
            self.realized_pnl = self.realized_pnl + close_pnl.to_decimals(self.realized_pnl.decimals());

            // Then open new position with remaining size
            let remaining_size = (fill_size - self.abs_size()).abs();
            self.entry_notional = remaining_size * fill_price;
        }

        self.size = new_size;

        // Clean up if position is closed
        if self.size.is_zero() {
            self.entry_notional = Decimal::from_raw(0, self.entry_notional.decimals());
        }
    }

    /// Settle funding payment
    pub fn settle_funding(&mut self, funding_accumulator: Decimal) -> Decimal {
        let funding_delta = funding_accumulator - self.last_funding_index;
        let payment = self.size * funding_delta;

        self.last_funding_index = funding_accumulator;

        // Return the payment amount (positive = payment owed by position holder)
        payment.to_decimals(Decimal::USDC_DECIMALS)
    }

    /// Create a position for testing purposes
    ///
    /// # Arguments
    /// * `is_long` - True for long position, false for short
    /// * `size` - Position size as a string (e.g., "0.5")
    /// * `entry_price` - Entry price as a string (e.g., "65000")
    /// * `realized_pnl` - Realized PnL as a string (e.g., "100")
    /// * `last_funding_index` - Last funding index as an integer
    #[cfg(any(test, feature = "testing"))]
    pub fn new_for_test(
        is_long: bool,
        size: &str,
        entry_price: &str,
        realized_pnl: &str,
        last_funding_index: i128,
    ) -> Self {
        let abs_size = Decimal::size(size);
        let price = Decimal::price(entry_price);
        let entry_notional = abs_size * price;

        Self {
            size: if is_long { abs_size } else { -abs_size },
            entry_notional,
            realized_pnl: Decimal::usdc(realized_pnl),
            last_funding_index: Decimal::from_raw(last_funding_index, Decimal::RATE_DECIMALS),
        }
    }
}

/// User position summary (for API responses)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSummary {
    pub market_id: u8,
    pub market_name: String,
    pub size: String,
    pub side: String,
    pub entry_price: String,
    pub mark_price: String,
    pub liquidation_price: Option<String>,
    pub unrealized_pnl: String,
    pub realized_pnl: String,
    pub leverage: u8,
    pub margin_used: String,
    pub roe: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_entry_price() {
        let mut pos = Position::new();

        // Open long at 65000
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let entry = pos.entry_price().unwrap();
        assert_eq!(entry.to_string_trimmed(), "65000");
    }

    #[test]
    fn test_position_unrealized_pnl() {
        let mut pos = Position::new();

        // Open long 1 BTC at 65000
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Mark price 66000 -> +1000 profit
        let pnl = pos.unrealized_pnl(Decimal::price("66000"));
        assert_eq!(pnl.to_string_trimmed(), "1000");

        // Mark price 64000 -> -1000 loss
        let pnl = pos.unrealized_pnl(Decimal::price("64000"));
        assert_eq!(pnl.to_string_trimmed(), "-1000");
    }

    #[test]
    fn test_position_reduce() {
        let mut pos = Position::new();

        // Open long 1 BTC at 65000
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Close 0.5 BTC at 66000 -> 500 profit
        pos.apply_fill(Decimal::size("0.5"), Decimal::price("66000"), false);

        assert_eq!(pos.size.to_string_trimmed(), "0.5");
        assert_eq!(pos.realized_pnl.to_string_trimmed(), "500");
    }

    #[test]
    fn test_position_flip() {
        let mut pos = Position::new();

        // Open long 1 BTC at 65000
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Sell 2 BTC at 66000 -> close long + open short
        pos.apply_fill(Decimal::size("2.0"), Decimal::price("66000"), false);

        assert!(pos.is_short());
        assert_eq!(pos.abs_size().to_string_trimmed(), "1");
        // Realized 1000 profit from closing long
        assert_eq!(pos.realized_pnl.to_string_trimmed(), "1000");
    }

    #[test]
    fn test_short_position() {
        let mut pos = Position::new();

        // Open short 1 BTC at 65000
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), false);

        assert!(pos.is_short());

        // Mark price 64000 -> +1000 profit (short wins when price drops)
        let pnl = pos.unrealized_pnl(Decimal::price("64000"));
        assert_eq!(pnl.to_string_trimmed(), "1000");

        // Mark price 66000 -> -1000 loss
        let pnl = pos.unrealized_pnl(Decimal::price("66000"));
        assert_eq!(pnl.to_string_trimmed(), "-1000");
    }

    #[test]
    fn test_calculate_fill_pnl_empty_position() {
        let pos = Position::new();

        // Opening a new position should have 0 realized PnL
        let pnl = pos.calculate_fill_pnl(Decimal::size("1.0"), Decimal::price("65000"), true);
        assert_eq!(pnl.raw(), 0, "Opening new position should realize 0 PnL");
    }

    #[test]
    fn test_calculate_fill_pnl_increase_long() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Adding to long position should have 0 realized PnL
        let pnl = pos.calculate_fill_pnl(Decimal::size("1.0"), Decimal::price("66000"), true);
        assert_eq!(pnl.raw(), 0, "Increasing position should realize 0 PnL");
    }

    #[test]
    fn test_calculate_fill_pnl_reduce_long_profit() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Closing half at 66000 -> $500 profit (0.5 * 1000)
        let pnl = pos.calculate_fill_pnl(Decimal::size("0.5"), Decimal::price("66000"), false);
        assert_eq!(pnl.to_string_trimmed(), "500", "Closing long at profit should realize positive PnL");
    }

    #[test]
    fn test_calculate_fill_pnl_reduce_long_loss() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Closing half at 64000 -> -$500 loss (0.5 * -1000)
        let pnl = pos.calculate_fill_pnl(Decimal::size("0.5"), Decimal::price("64000"), false);
        assert_eq!(pnl.to_string_trimmed(), "-500", "Closing long at loss should realize negative PnL");
    }

    #[test]
    fn test_calculate_fill_pnl_reduce_short_profit() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), false);

        // Closing half short at 64000 -> $500 profit (short wins when price drops)
        let pnl = pos.calculate_fill_pnl(Decimal::size("0.5"), Decimal::price("64000"), true);
        assert_eq!(pnl.to_string_trimmed(), "500", "Closing short at profit should realize positive PnL");
    }

    #[test]
    fn test_calculate_fill_pnl_reduce_short_loss() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), false);

        // Closing half short at 66000 -> -$500 loss (short loses when price rises)
        let pnl = pos.calculate_fill_pnl(Decimal::size("0.5"), Decimal::price("66000"), true);
        assert_eq!(pnl.to_string_trimmed(), "-500", "Closing short at loss should realize negative PnL");
    }

    #[test]
    fn test_calculate_fill_pnl_flip_position() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        // Selling 2 BTC at 66000 flips from long to short
        // Closing 1 BTC long at +$1000 profit
        let pnl = pos.calculate_fill_pnl(Decimal::size("2.0"), Decimal::price("66000"), false);
        assert_eq!(pnl.to_string_trimmed(), "1000", "Flipping position should realize PnL on closed portion");
    }

    #[test]
    fn test_calculate_fill_pnl_doesnt_modify_state() {
        let mut pos = Position::new();
        pos.apply_fill(Decimal::size("1.0"), Decimal::price("65000"), true);

        let size_before = pos.size;
        let entry_before = pos.entry_notional;
        let realized_before = pos.realized_pnl;

        // Calculate PnL without modifying state
        let _ = pos.calculate_fill_pnl(Decimal::size("0.5"), Decimal::price("66000"), false);

        // State should be unchanged
        assert_eq!(pos.size.raw(), size_before.raw(), "calculate_fill_pnl should not modify size");
        assert_eq!(pos.entry_notional.raw(), entry_before.raw(), "calculate_fill_pnl should not modify entry_notional");
        assert_eq!(pos.realized_pnl.raw(), realized_before.raw(), "calculate_fill_pnl should not modify realized_pnl");
    }
}
