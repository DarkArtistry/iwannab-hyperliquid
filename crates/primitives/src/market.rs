//! Market configuration and state

use crate::{Decimal, Timestamp};
use serde::{Deserialize, Serialize};

/// Market identifier (0-255)
pub type MarketId = u8;

/// Market status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketStatus {
    /// Market is active and accepting orders
    Active,
    /// Market is paused (no new orders, existing orders can be canceled)
    Paused,
    /// Market is settling (positions being closed at settlement price)
    Settling,
    /// Market is settled (no activity allowed)
    Settled,
}

impl Default for MarketStatus {
    fn default() -> Self {
        MarketStatus::Active
    }
}

/// Market configuration (static parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    /// Market ID
    pub id: MarketId,
    /// Symbol (e.g., "BTC-PERP")
    pub symbol: String,
    /// Base asset name (e.g., "BTC")
    pub base_asset: String,
    /// Decimals for size (e.g., 3 for 0.001 precision)
    pub size_decimals: u8,
    /// Decimals for price
    pub price_decimals: u8,
    /// Minimum price increment
    pub tick_size: Decimal,
    /// Minimum size increment
    pub lot_size: Decimal,
    /// Minimum order size
    pub min_order_size: Decimal,
    /// Maximum order size
    pub max_order_size: Decimal,
    /// Maximum position size (notional)
    pub max_position_notional: Decimal,
    /// Maximum leverage allowed
    pub max_leverage: u8,
    /// Maintenance margin rate (e.g., 0.025 = 2.5%)
    pub maintenance_margin_rate: Decimal,
    /// Initial margin buffer factor
    pub initial_margin_factor: Decimal,
    /// Maker fee rate (can be negative for rebate)
    pub maker_fee_rate: Decimal,
    /// Taker fee rate
    pub taker_fee_rate: Decimal,
    /// Liquidation fee rate (to insurance fund)
    pub liquidation_fee_rate: Decimal,
    /// Maximum open orders per user
    pub max_open_orders: u32,
}

impl MarketConfig {
    /// Create a new market config with basic parameters
    pub fn new(id: MarketId, symbol: String, max_leverage: u8) -> Self {
        Self {
            id,
            symbol: symbol.clone(),
            base_asset: symbol.split('-').next().unwrap_or(&symbol).to_string(),
            size_decimals: 3,
            price_decimals: 2,
            tick_size: Decimal::price("0.01"),
            lot_size: Decimal::size("0.001"),
            min_order_size: Decimal::size("0.001"),
            max_order_size: Decimal::size("10000"),
            max_position_notional: Decimal::usdc("10000000"),
            max_leverage,
            maintenance_margin_rate: Decimal::rate("0.025"),
            initial_margin_factor: Decimal::rate("1.0"),
            maker_fee_rate: Decimal::rate("0.0002"),
            taker_fee_rate: Decimal::rate("0.0005"),
            liquidation_fee_rate: Decimal::rate("0.005"),
            max_open_orders: 200,
        }
    }

    /// Create BTC-PERP configuration
    pub fn btc_perp() -> Self {
        Self {
            id: 0,
            symbol: "BTC-PERP".to_string(),
            base_asset: "BTC".to_string(),
            size_decimals: 3,
            price_decimals: 1,
            tick_size: Decimal::price("0.1"),
            lot_size: Decimal::size("0.001"),
            min_order_size: Decimal::size("0.001"),
            max_order_size: Decimal::size("1000"),
            max_position_notional: Decimal::usdc("10000000"), // $10M
            max_leverage: 50,
            maintenance_margin_rate: Decimal::rate("0.025"), // 2.5%
            initial_margin_factor: Decimal::rate("1.0"),
            maker_fee_rate: Decimal::rate("0.0002"),  // 2 bps
            taker_fee_rate: Decimal::rate("0.0005"),  // 5 bps
            liquidation_fee_rate: Decimal::rate("0.005"), // 50 bps
            max_open_orders: 200,
        }
    }

    /// Create ETH-PERP configuration
    pub fn eth_perp() -> Self {
        Self {
            id: 1,
            symbol: "ETH-PERP".to_string(),
            base_asset: "ETH".to_string(),
            size_decimals: 2,
            price_decimals: 2,
            tick_size: Decimal::price("0.01"),
            lot_size: Decimal::size("0.01"),
            min_order_size: Decimal::size("0.01"),
            max_order_size: Decimal::size("10000"),
            max_position_notional: Decimal::usdc("10000000"), // $10M
            max_leverage: 50,
            maintenance_margin_rate: Decimal::rate("0.025"),
            initial_margin_factor: Decimal::rate("1.0"),
            maker_fee_rate: Decimal::rate("0.0002"),
            taker_fee_rate: Decimal::rate("0.0005"),
            liquidation_fee_rate: Decimal::rate("0.005"),
            max_open_orders: 200,
        }
    }

    /// Validate a price against tick size
    pub fn validate_price(&self, price: Decimal) -> bool {
        let tick_raw = self.tick_size.raw();
        price.raw() % tick_raw == 0
    }

    /// Validate a size against lot size
    pub fn validate_size(&self, size: Decimal) -> bool {
        let lot_raw = self.lot_size.raw();
        size.raw() % lot_raw == 0 && size >= self.min_order_size
    }

    /// Calculate initial margin required for a position
    pub fn initial_margin(&self, size: Decimal, price: Decimal, leverage: u8) -> Decimal {
        let notional = size.abs() * price;
        notional / Decimal::from_int(leverage as i64, notional.decimals())
    }

    /// Calculate maintenance margin required
    pub fn maintenance_margin(&self, size: Decimal, price: Decimal) -> Decimal {
        let notional = size.abs() * price;
        notional * self.maintenance_margin_rate.to_decimals(notional.decimals())
    }

    /// Calculate maker fee
    pub fn maker_fee(&self, notional: Decimal) -> Decimal {
        notional * self.maker_fee_rate.to_decimals(notional.decimals())
    }

    /// Calculate taker fee
    pub fn taker_fee(&self, notional: Decimal) -> Decimal {
        notional * self.taker_fee_rate.to_decimals(notional.decimals())
    }
}

/// Market state (dynamic, changes with trading)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketState {
    /// Current mark price
    pub mark_price: Decimal,
    /// Current index price (from oracle)
    pub index_price: Decimal,
    /// Best bid price
    pub best_bid: Option<Decimal>,
    /// Best ask price
    pub best_ask: Option<Decimal>,
    /// Current funding rate
    pub funding_rate: Decimal,
    /// Current premium index
    pub premium: Decimal,
    /// Funding accumulator (for lazy settlement)
    pub funding_accumulator: Decimal,
    /// Next funding settlement time
    pub next_funding_time: Timestamp,
    /// Open interest (long side)
    pub open_interest_long: Decimal,
    /// Open interest (short side)
    pub open_interest_short: Decimal,
    /// 24h volume
    pub volume_24h: Decimal,
    /// Current status
    pub status: MarketStatus,
    /// Last trade timestamp
    pub last_trade_time: Timestamp,
}

impl MarketState {
    /// Create new market state
    pub fn new(initial_price: Decimal, funding_time: Timestamp) -> Self {
        Self {
            mark_price: initial_price,
            index_price: initial_price,
            best_bid: None,
            best_ask: None,
            funding_rate: Decimal::from_raw(0, Decimal::RATE_DECIMALS),
            premium: Decimal::from_raw(0, Decimal::RATE_DECIMALS),
            funding_accumulator: Decimal::from_raw(0, Decimal::RATE_DECIMALS),
            next_funding_time: funding_time,
            open_interest_long: Decimal::from_raw(0, Decimal::SIZE_DECIMALS),
            open_interest_short: Decimal::from_raw(0, Decimal::SIZE_DECIMALS),
            volume_24h: Decimal::from_raw(0, Decimal::USDC_DECIMALS),
            status: MarketStatus::Active,
            last_trade_time: 0,
        }
    }

    /// Calculate mid price
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => {
                let sum = bid + ask;
                Some(sum.div_int(2))
            }
            _ => None,
        }
    }

    /// Calculate spread
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Check if market is tradeable
    pub fn is_tradeable(&self) -> bool {
        matches!(self.status, MarketStatus::Active)
    }

    /// Update open interest after a trade
    pub fn update_open_interest(&mut self, size_delta_long: Decimal, size_delta_short: Decimal) {
        self.open_interest_long = self.open_interest_long + size_delta_long;
        self.open_interest_short = self.open_interest_short + size_delta_short;
    }
}

/// Complete market (config + state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub config: MarketConfig,
    pub state: MarketState,
}

impl Market {
    pub fn new(config: MarketConfig, initial_price: Decimal, funding_time: Timestamp) -> Self {
        Self {
            state: MarketState::new(initial_price, funding_time),
            config,
        }
    }

    pub fn id(&self) -> MarketId {
        self.config.id
    }

    pub fn symbol(&self) -> &str {
        &self.config.symbol
    }
}

/// Market metadata for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketMeta {
    pub name: String,
    #[serde(rename = "szDecimals")]
    pub size_decimals: u8,
    #[serde(rename = "maxLeverage")]
    pub max_leverage: u8,
    #[serde(rename = "tickSize")]
    pub tick_size: String,
    #[serde(rename = "lotSize")]
    pub lot_size: String,
    #[serde(rename = "makerFee")]
    pub maker_fee: String,
    #[serde(rename = "takerFee")]
    pub taker_fee: String,
}

impl From<&MarketConfig> for MarketMeta {
    fn from(config: &MarketConfig) -> Self {
        Self {
            name: config.symbol.clone(),
            size_decimals: config.size_decimals,
            max_leverage: config.max_leverage,
            tick_size: config.tick_size.to_string_trimmed(),
            lot_size: config.lot_size.to_string_trimmed(),
            maker_fee: config.maker_fee_rate.to_string_trimmed(),
            taker_fee: config.taker_fee_rate.to_string_trimmed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_config() {
        let btc = MarketConfig::btc_perp();
        assert_eq!(btc.id, 0);
        assert_eq!(btc.symbol, "BTC-PERP");
        assert_eq!(btc.max_leverage, 50);
    }

    #[test]
    fn test_price_validation() {
        let btc = MarketConfig::btc_perp();

        assert!(btc.validate_price(Decimal::price("65000.0")));
        assert!(btc.validate_price(Decimal::price("65000.1")));
        assert!(!btc.validate_price(Decimal::price("65000.05"))); // Not multiple of 0.1
    }

    #[test]
    fn test_size_validation() {
        let btc = MarketConfig::btc_perp();

        assert!(btc.validate_size(Decimal::size("1.0")));
        assert!(btc.validate_size(Decimal::size("0.001")));
        assert!(!btc.validate_size(Decimal::size("0.0005"))); // Not multiple of 0.001
    }

    #[test]
    fn test_margin_calculation() {
        let btc = MarketConfig::btc_perp();

        let size = Decimal::size("1.0");
        let price = Decimal::price("65000");

        // 10x leverage -> 6500 margin
        let margin = btc.initial_margin(size, price, 10);
        assert_eq!(margin.to_string_trimmed(), "6500");

        // Maintenance margin at 2.5% -> 1625
        let maint = btc.maintenance_margin(size, price);
        assert_eq!(maint.to_string_trimmed(), "1625");
    }
}
