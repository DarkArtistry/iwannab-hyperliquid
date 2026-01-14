//! HIP-1 Style Spot Token Primitives
//!
//! Native spot tokens with built-in orderbooks, inspired by Hyperliquid's HIP-1 standard.
//!
//! Key concepts:
//! - Each token gets a unique TokenIndex (0-255)
//! - Tokens can be linked to ERC20 contracts on HyperEVM
//! - System address bridging allows transfers between EVM and Core
//! - Built-in spot orderbooks for token/USDC pairs

use crate::{AccountAddress, Decimal, Timestamp};
use alloy_primitives::Address;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

/// Token index (0-255), similar to HIP-1's tokenIndex
/// Used to identify tokens and derive system addresses
pub type TokenIndex = u8;

/// Spot market identifier
/// Uses range 128-255 to distinguish from perp markets (0-127)
pub type SpotMarketId = u8;

/// Token type - native or linked to ERC20
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenType {
    /// Native token (like USDC)
    Native,
    /// Linked to an ERC20 contract on HyperEVM
    Erc20(Address),
}

impl Default for TokenType {
    fn default() -> Self {
        TokenType::Native
    }
}

/// Spot token definition (HIP-1 style)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotToken {
    /// Unique token index (0-255)
    pub index: TokenIndex,
    /// Token name (e.g., "Ethereum")
    pub name: String,
    /// Token symbol (e.g., "ETH") - max 6 chars like HIP-1
    pub symbol: String,
    /// Decimals for minimal unit conversion (weiDecimals in HIP-1)
    pub wei_decimals: u8,
    /// Minimum tradable decimals on orderbook (szDecimals in HIP-1)
    pub sz_decimals: u8,
    /// Maximum supply (can be reduced via burns)
    pub max_supply: Decimal,
    /// Current circulating supply
    pub circulating_supply: Decimal,
    /// Token type (native or ERC20-linked)
    pub token_type: TokenType,
    /// System address for EVM bridging (derived from index)
    pub system_address: Address,
    /// Deployer address (receives trading fees)
    pub deployer: AccountAddress,
    /// Deployer fee share (0-100%)
    pub deployer_fee_share: Decimal,
    /// Creation timestamp
    pub created_at: Timestamp,
}

impl SpotToken {
    /// Create a new spot token with HIP-1 style parameters
    pub fn new(
        index: TokenIndex,
        name: String,
        symbol: String,
        wei_decimals: u8,
        sz_decimals: u8,
        max_supply: Decimal,
        deployer: AccountAddress,
    ) -> Self {
        assert!(sz_decimals + 5 <= wei_decimals, "szDecimals + 5 must be <= weiDecimals");
        assert!(symbol.len() <= 6, "symbol must be <= 6 characters");

        Self {
            index,
            name,
            symbol,
            wei_decimals,
            sz_decimals,
            max_supply,
            circulating_supply: max_supply, // Initially all supply to deployer
            token_type: TokenType::Native,
            system_address: Self::derive_system_address(index),
            deployer,
            deployer_fee_share: Decimal::rate("1.0"), // 100% to deployer by default
            created_at: 0,
        }
    }

    /// Create USDC token (index 0, special native quote token)
    pub fn usdc() -> Self {
        Self {
            index: 0,
            name: "USD Coin".to_string(),
            symbol: "USDC".to_string(),
            wei_decimals: 6,
            sz_decimals: 0, // No decimals for USDC spot
            max_supply: Decimal::from_raw(u64::MAX as i128, 6), // Effectively unlimited
            circulating_supply: Decimal::from_raw(0, 6),
            token_type: TokenType::Native,
            system_address: Self::derive_system_address(0),
            deployer: Address::ZERO,
            deployer_fee_share: Decimal::from_raw(0, 10),
            created_at: 0,
        }
    }

    /// Derive system address from token index (HIP-1 style)
    /// The system address is used for EVM ↔ Core transfers
    pub fn derive_system_address(index: TokenIndex) -> Address {
        // Use keccak256("HyperCore.SpotToken.SystemAddress" + index) truncated to 20 bytes
        let mut hasher = Keccak256::new();
        hasher.update(b"HyperCore.SpotToken.SystemAddress");
        hasher.update([index]);
        let hash = hasher.finalize();
        Address::from_slice(&hash[12..32])
    }

    /// Calculate lot size (minimum tradable unit)
    pub fn lot_size(&self) -> u128 {
        10u128.pow((self.wei_decimals - self.sz_decimals) as u32)
    }

    /// Link this token to an ERC20 contract
    pub fn link_erc20(&mut self, contract: Address) {
        self.token_type = TokenType::Erc20(contract);
    }

    /// Check if token is linked to ERC20
    pub fn is_erc20_linked(&self) -> bool {
        matches!(self.token_type, TokenType::Erc20(_))
    }

    /// Get ERC20 address if linked
    pub fn erc20_address(&self) -> Option<Address> {
        match &self.token_type {
            TokenType::Erc20(addr) => Some(*addr),
            TokenType::Native => None,
        }
    }
}

/// Spot balance for an account's token holdings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpotBalance {
    /// Total balance owned
    pub total: Decimal,
    /// Amount reserved in open orders
    pub reserved: Decimal,
}

impl SpotBalance {
    /// Create new balance
    pub fn new(total: Decimal) -> Self {
        Self {
            total,
            reserved: Decimal::from_raw(0, total.decimals()),
        }
    }

    /// Available balance (total - reserved)
    pub fn available(&self) -> Decimal {
        self.total - self.reserved
    }

    /// Reserve balance for an order
    pub fn reserve(&mut self, amount: Decimal) -> bool {
        if self.available() >= amount {
            self.reserved = self.reserved + amount;
            true
        } else {
            false
        }
    }

    /// Release reserved balance
    pub fn release(&mut self, amount: Decimal) {
        self.reserved = self.reserved - amount.min(self.reserved);
    }

    /// Add to total balance
    pub fn credit(&mut self, amount: Decimal) {
        self.total = self.total + amount;
    }

    /// Remove from total balance
    pub fn debit(&mut self, amount: Decimal) -> bool {
        if self.available() >= amount {
            self.total = self.total - amount;
            true
        } else {
            false
        }
    }
}

/// Spot market configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotMarketConfig {
    /// Market ID (128-255 range for spot)
    pub id: SpotMarketId,
    /// Market symbol (e.g., "ETH-USDC")
    pub symbol: String,
    /// Base token index
    pub base_token: TokenIndex,
    /// Quote token index (usually USDC = 0)
    pub quote_token: TokenIndex,
    /// Minimum price increment
    pub tick_size: Decimal,
    /// Minimum size increment
    pub lot_size: Decimal,
    /// Minimum order size
    pub min_order_size: Decimal,
    /// Maximum order size
    pub max_order_size: Decimal,
    /// Maker fee rate (can be negative for rebate)
    pub maker_fee_rate: Decimal,
    /// Taker fee rate
    pub taker_fee_rate: Decimal,
    /// Whether market is active
    pub is_active: bool,
}

impl SpotMarketConfig {
    /// Create a new spot market for token/USDC pair
    pub fn new_usdc_pair(base_token: &SpotToken, id: SpotMarketId) -> Self {
        let symbol = format!("{}-USDC", base_token.symbol);

        // Calculate lot size: the minimum tradable unit based on szDecimals
        let lot_size_raw = base_token.lot_size() as i128;

        // Max order size: 1 million times the lot size, or the max supply / 10
        // Using i128 to avoid overflow
        let max_order_raw = lot_size_raw.saturating_mul(1_000_000_000_000); // 10^12 lots

        Self {
            id,
            symbol,
            base_token: base_token.index,
            quote_token: 0, // USDC
            tick_size: Decimal::price("0.0001"), // 4 decimals
            lot_size: Decimal::from_raw(lot_size_raw, base_token.wei_decimals),
            min_order_size: Decimal::from_raw(lot_size_raw, base_token.wei_decimals),
            max_order_size: Decimal::from_raw(max_order_raw, base_token.wei_decimals),
            maker_fee_rate: Decimal::rate("-0.0002"), // -2 bps rebate
            taker_fee_rate: Decimal::rate("0.0005"),  // 5 bps
            is_active: true,
        }
    }

    /// Validate price against tick size
    pub fn validate_price(&self, price: Decimal) -> bool {
        let tick_raw = self.tick_size.raw();
        if tick_raw == 0 { return true; }
        price.raw() % tick_raw == 0
    }

    /// Validate size against lot size
    pub fn validate_size(&self, size: Decimal) -> bool {
        let lot_raw = self.lot_size.raw();
        if lot_raw == 0 { return true; }
        size.raw() % lot_raw == 0 && size >= self.min_order_size
    }

    /// Calculate maker fee (positive = pay, negative = rebate)
    pub fn maker_fee(&self, notional: Decimal) -> Decimal {
        notional * self.maker_fee_rate.to_decimals(notional.decimals())
    }

    /// Calculate taker fee
    pub fn taker_fee(&self, notional: Decimal) -> Decimal {
        notional * self.taker_fee_rate.to_decimals(notional.decimals())
    }
}

/// Spot market state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpotMarketState {
    /// Best bid price
    pub best_bid: Option<Decimal>,
    /// Best ask price
    pub best_ask: Option<Decimal>,
    /// Last trade price
    pub last_price: Option<Decimal>,
    /// 24h volume in quote (USDC)
    pub volume_24h: Decimal,
    /// 24h volume in base
    pub base_volume_24h: Decimal,
    /// Number of trades in 24h
    pub trades_24h: u64,
    /// Last trade timestamp
    pub last_trade_time: Timestamp,
}

impl SpotMarketState {
    /// Calculate mid price
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask).div_int(2)),
            _ => self.last_price,
        }
    }

    /// Calculate spread
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }
}

/// Complete spot market (config + state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotMarket {
    pub config: SpotMarketConfig,
    pub state: SpotMarketState,
}

impl SpotMarket {
    pub fn new(config: SpotMarketConfig) -> Self {
        Self {
            config,
            state: SpotMarketState::default(),
        }
    }

    pub fn id(&self) -> SpotMarketId {
        self.config.id
    }

    pub fn symbol(&self) -> &str {
        &self.config.symbol
    }
}

/// Spot fill event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotFill {
    pub market_id: SpotMarketId,
    pub maker: AccountAddress,
    pub taker: AccountAddress,
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub price: Decimal,
    pub base_size: Decimal,
    pub quote_size: Decimal, // price * base_size
    pub maker_fee: Decimal,  // Can be negative (rebate)
    pub taker_fee: Decimal,
    pub timestamp: Timestamp,
    pub is_taker_buy: bool,
}

/// Token deploy action parameters (like HIP-1 Dutch auction winner)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotTokenDeployParams {
    /// Token name
    pub name: String,
    /// Token symbol (max 6 chars)
    pub symbol: String,
    /// Decimals for minimal unit (weiDecimals)
    pub wei_decimals: u8,
    /// Tradable decimals (szDecimals)
    pub sz_decimals: u8,
    /// Maximum/initial supply
    pub max_supply: Decimal,
    /// Initial allocations (addresses and amounts)
    pub genesis_allocations: Vec<(AccountAddress, Decimal)>,
    /// Optional: Link to existing ERC20 contract
    pub erc20_address: Option<Address>,
}

/// Token transfer between EVM and Core
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotTokenTransfer {
    /// Token index
    pub token_index: TokenIndex,
    /// Sender address
    pub from: AccountAddress,
    /// Recipient address
    pub to: AccountAddress,
    /// Amount to transfer
    pub amount: Decimal,
    /// Is this EVM -> Core (true) or Core -> EVM (false)
    pub is_to_core: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_address_derivation() {
        // System address should be deterministic
        let addr1 = SpotToken::derive_system_address(1);
        let addr2 = SpotToken::derive_system_address(1);
        assert_eq!(addr1, addr2);

        // Different indices should give different addresses
        let addr3 = SpotToken::derive_system_address(2);
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_spot_token_creation() {
        let token = SpotToken::new(
            1,
            "Test Token".to_string(),
            "TEST".to_string(),
            18,  // wei decimals
            8,   // sz decimals (lot size = 10^10)
            Decimal::from_raw(1_000_000_000_000_000_000_000_000i128, 18), // 1M tokens
            Address::ZERO,
        );

        assert_eq!(token.index, 1);
        assert_eq!(token.symbol, "TEST");
        assert_eq!(token.lot_size(), 10_000_000_000); // 10^10
    }

    #[test]
    fn test_spot_balance() {
        let mut balance = SpotBalance::new(Decimal::from_raw(1000, 6));

        // Reserve some
        assert!(balance.reserve(Decimal::from_raw(300, 6)));
        assert_eq!(balance.available().raw(), 700);
        assert_eq!(balance.reserved.raw(), 300);

        // Can't reserve more than available
        assert!(!balance.reserve(Decimal::from_raw(800, 6)));

        // Release
        balance.release(Decimal::from_raw(100, 6));
        assert_eq!(balance.reserved.raw(), 200);
        assert_eq!(balance.available().raw(), 800);
    }

    #[test]
    fn test_spot_market_config() {
        let token = SpotToken::new(
            1,
            "Ethereum".to_string(),
            "ETH".to_string(),
            18,
            8,
            Decimal::from_raw(100_000_000_000_000_000_000_000_000i128, 18),
            Address::ZERO,
        );

        let market = SpotMarketConfig::new_usdc_pair(&token, 128);

        assert_eq!(market.id, 128);
        assert_eq!(market.symbol, "ETH-USDC");
        assert_eq!(market.base_token, 1);
        assert_eq!(market.quote_token, 0);
    }

    #[test]
    fn test_usdc_token() {
        let usdc = SpotToken::usdc();
        assert_eq!(usdc.index, 0);
        assert_eq!(usdc.symbol, "USDC");
        assert_eq!(usdc.wei_decimals, 6);
    }

    #[test]
    #[should_panic(expected = "szDecimals + 5 must be <= weiDecimals")]
    fn test_invalid_decimals() {
        // This should panic: 15 + 5 = 20 > 18
        SpotToken::new(
            1,
            "Bad Token".to_string(),
            "BAD".to_string(),
            18,
            15, // Invalid: 15 + 5 = 20 > 18
            Decimal::from_raw(1000, 18),
            Address::ZERO,
        );
    }
}
