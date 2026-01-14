//! HyperCore precompiles for EVM
//!
//! These precompiles provide read access to HyperCore exchange state from Solidity contracts.
//! Perpetuals precompiles: 0x0800 - 0x0805
//! Spot precompiles: 0x0806 - 0x0808

use std::sync::Arc;

use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{AccountAddress, Decimal, MarketId, SpotMarketId, TokenIndex};
use revm::primitives::{Address, Bytes, PrecompileError, PrecompileErrors, PrecompileOutput, PrecompileResult};
use tokio::sync::RwLock;

/// Precompile addresses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecompileAddress {
    // === Perpetuals Precompiles (0x0800 - 0x0805) ===
    /// Get position for address/market
    Position = 0x0800,
    /// Get account info (balance, equity, margin)
    Account = 0x0801,
    /// Get market info (price, funding rate)
    Market = 0x0802,
    /// Get order by ID
    Order = 0x0803,
    /// Get current funding rate
    Funding = 0x0804,
    /// Get orderbook snapshot
    OrderBook = 0x0805,

    // === Spot Precompiles (0x0806 - 0x0808) ===
    /// Get spot token balance for address/token
    SpotBalance = 0x0806,
    /// Get spot market info
    SpotMarket = 0x0807,
    /// Get spot orderbook snapshot
    SpotOrderBook = 0x0808,
}

impl PrecompileAddress {
    pub fn from_address(addr: Address) -> Option<Self> {
        let bytes = addr.as_slice();
        if bytes.len() < 2 {
            return None;
        }
        // Check if it's in the precompile range (first 18 bytes should be zero)
        if !bytes[..18].iter().all(|&b| b == 0) {
            return None;
        }

        let value = u16::from_be_bytes([bytes[18], bytes[19]]);
        match value {
            // Perpetuals
            0x0800 => Some(Self::Position),
            0x0801 => Some(Self::Account),
            0x0802 => Some(Self::Market),
            0x0803 => Some(Self::Order),
            0x0804 => Some(Self::Funding),
            0x0805 => Some(Self::OrderBook),
            // Spot
            0x0806 => Some(Self::SpotBalance),
            0x0807 => Some(Self::SpotMarket),
            0x0808 => Some(Self::SpotOrderBook),
            _ => None,
        }
    }

    /// Check if this is a spot precompile (requires SpotEngine)
    pub fn is_spot(&self) -> bool {
        matches!(self, Self::SpotBalance | Self::SpotMarket | Self::SpotOrderBook)
    }

    pub fn to_address(self) -> Address {
        let mut bytes = [0u8; 20];
        let value = self as u16;
        bytes[18] = (value >> 8) as u8;
        bytes[19] = value as u8;
        Address::from(bytes)
    }

    pub fn gas_cost(&self) -> u64 {
        match self {
            // Perpetuals
            Self::Position => 100,
            Self::Account => 150,
            Self::Market => 100,
            Self::Order => 100,
            Self::Funding => 50,
            Self::OrderBook => 500,
            // Spot
            Self::SpotBalance => 100,
            Self::SpotMarket => 100,
            Self::SpotOrderBook => 500,
        }
    }
}

/// HyperCore precompiles implementation
pub struct HyperCorePrecompiles {
    /// Reference to perpetuals engine state
    engine: Arc<RwLock<EngineState>>,
    /// Reference to spot engine (optional)
    spot_engine: Option<Arc<RwLock<SpotEngine>>>,
}

impl HyperCorePrecompiles {
    /// Create new precompiles with engine state reference
    pub fn new(engine: Arc<RwLock<EngineState>>) -> Self {
        Self {
            engine,
            spot_engine: None,
        }
    }

    /// Create new precompiles with both perpetuals and spot engines
    pub fn with_spot(engine: Arc<RwLock<EngineState>>, spot_engine: Arc<RwLock<SpotEngine>>) -> Self {
        Self {
            engine,
            spot_engine: Some(spot_engine),
        }
    }

    /// Set the spot engine reference
    pub fn set_spot_engine(&mut self, spot_engine: Arc<RwLock<SpotEngine>>) {
        self.spot_engine = Some(spot_engine);
    }

    /// Check if address is a HyperCore precompile
    pub fn is_precompile(addr: &Address) -> bool {
        PrecompileAddress::from_address(*addr).is_some()
    }

    /// Execute precompile
    pub fn execute(&self, addr: Address, input: &Bytes, gas_limit: u64) -> PrecompileResult {
        let precompile = PrecompileAddress::from_address(addr)
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Unknown precompile")))?;

        let gas_cost = precompile.gas_cost();
        if gas_limit < gas_cost {
            return Err(PrecompileErrors::Error(PrecompileError::OutOfGas));
        }

        let output = match precompile {
            // Perpetuals precompiles
            PrecompileAddress::Position => self.get_position(input)?,
            PrecompileAddress::Account => self.get_account(input)?,
            PrecompileAddress::Market => self.get_market(input)?,
            PrecompileAddress::Order => self.get_order(input)?,
            PrecompileAddress::Funding => self.get_funding(input)?,
            PrecompileAddress::OrderBook => self.get_orderbook(input)?,
            // Spot precompiles
            PrecompileAddress::SpotBalance => self.get_spot_balance(input)?,
            PrecompileAddress::SpotMarket => self.get_spot_market(input)?,
            PrecompileAddress::SpotOrderBook => self.get_spot_orderbook(input)?,
        };

        Ok(PrecompileOutput::new(gas_cost, output))
    }

    /// Get position precompile
    /// Input: address (20 bytes) + marketId (1 byte)
    /// Output: ABI-encoded Position struct
    fn get_position(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.len() < 21 {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&input[..20]);
        let address = AccountAddress::from(addr_bytes);
        let market_id: MarketId = input[20];

        let engine = self.engine.blocking_read();
        let position = engine.get_position(address, market_id);

        // Encode position as 256 bytes of zeros if not found, or position data if found
        let mut output = vec![0u8; 256];
        if let Some(pos) = position {
            // Encode size as int256 at offset 0
            let size_bytes = pos.size.raw().to_be_bytes();
            output[16..32].copy_from_slice(&size_bytes);

            // Entry notional at offset 32
            let entry_bytes = pos.entry_notional.raw().to_be_bytes();
            output[48..64].copy_from_slice(&entry_bytes);

            // Realized PnL at offset 64
            let pnl_bytes = pos.realized_pnl.raw().to_be_bytes();
            output[80..96].copy_from_slice(&pnl_bytes);
        }

        Ok(Bytes::from(output))
    }

    /// Get account precompile
    /// Input: address (20 bytes)
    /// Output: ABI-encoded (balance, equity, marginUsed, withdrawable)
    fn get_account(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.len() < 20 {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&input[..20]);
        let address = AccountAddress::from(addr_bytes);

        let engine = self.engine.blocking_read();

        // Return zeros if account doesn't exist
        let mut output = vec![0u8; 128];

        if let Some(account) = engine.get_account(address) {
            // Balance at offset 0
            let balance_bytes = account.balance.to_be_bytes();
            output[16..32].copy_from_slice(&balance_bytes);

            // Equity, margin, withdrawable would need to be calculated
            // For now, just return balance for all
            output[48..64].copy_from_slice(&balance_bytes);
            output[80..96].copy_from_slice(&[0u8; 16]);
            output[112..128].copy_from_slice(&balance_bytes);
        }

        Ok(Bytes::from(output))
    }

    /// Get market precompile
    /// Input: marketId (1 byte)
    /// Output: ABI-encoded (markPrice, indexPrice, fundingRate, openInterest)
    fn get_market(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.is_empty() {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let market_id: MarketId = input[0];
        let engine = self.engine.blocking_read();

        let mut output = vec![0u8; 128];

        if let Some(state) = engine.get_market_state(market_id) {
            // Mark price at offset 0
            let mark_bytes = state.mark_price.raw().to_be_bytes();
            output[16..32].copy_from_slice(&mark_bytes);

            // Index price at offset 32
            let index_bytes = state.index_price.raw().to_be_bytes();
            output[48..64].copy_from_slice(&index_bytes);

            // Funding rate at offset 64
            let funding_bytes = state.funding_rate.raw().to_be_bytes();
            output[80..96].copy_from_slice(&funding_bytes);

            // Open interest at offset 96
            let oi_bytes = state.open_interest_long.raw().to_be_bytes();
            output[112..128].copy_from_slice(&oi_bytes);
        }

        Ok(Bytes::from(output))
    }

    /// Get order precompile
    /// Input: marketId (1 byte) + orderId (8 bytes)
    /// Output: ABI-encoded Order struct
    fn get_order(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.len() < 9 {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let market_id: MarketId = input[0];
        let order_id = u64::from_be_bytes(input[1..9].try_into().unwrap());

        let engine = self.engine.blocking_read();

        let mut output = vec![0u8; 256];

        if let Some(order) = engine.get_order(market_id, order_id) {
            // Order ID at offset 24-32
            output[24..32].copy_from_slice(&order.id.to_be_bytes());

            // Owner at offset 44-64
            output[44..64].copy_from_slice(order.owner.as_slice());

            // Price at offset 64-96
            let price_bytes = order.price.raw().to_be_bytes();
            output[80..96].copy_from_slice(&price_bytes);

            // Original size at offset 96-128
            let size_bytes = order.original_size.raw().to_be_bytes();
            output[112..128].copy_from_slice(&size_bytes);

            // Remaining size at offset 128-160
            let remaining_bytes = order.remaining_size.raw().to_be_bytes();
            output[144..160].copy_from_slice(&remaining_bytes);
        }

        Ok(Bytes::from(output))
    }

    /// Get funding rate precompile
    /// Input: marketId (1 byte)
    /// Output: ABI-encoded (fundingRate, nextFundingTime)
    fn get_funding(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.is_empty() {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let market_id: MarketId = input[0];
        let engine = self.engine.blocking_read();

        let mut output = vec![0u8; 64];

        if let Some(state) = engine.get_market_state(market_id) {
            // Funding rate at offset 0
            let funding_bytes = state.funding_rate.raw().to_be_bytes();
            output[16..32].copy_from_slice(&funding_bytes);

            // Next funding time at offset 56-64
            output[56..64].copy_from_slice(&state.next_funding_time.to_be_bytes());
        }

        Ok(Bytes::from(output))
    }

    /// Get orderbook precompile
    /// Input: marketId (1 byte) + depth (1 byte)
    /// Output: ABI-encoded L2 snapshot
    fn get_orderbook(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.len() < 2 {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let market_id: MarketId = input[0];
        let depth = input[1].min(20) as usize;

        let engine = self.engine.blocking_read();

        let book = engine.get_orderbook(market_id)
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Orderbook not found")))?;

        let (bids, asks) = book.get_l2(depth);

        // Encode as simple arrays
        let mut output = Vec::with_capacity(64 + (bids.len() + asks.len()) * 64);

        // Bids count
        output.extend_from_slice(&[0u8; 28]);
        output.extend_from_slice(&(bids.len() as u32).to_be_bytes());

        // Asks count
        output.extend_from_slice(&[0u8; 28]);
        output.extend_from_slice(&(asks.len() as u32).to_be_bytes());

        // Bids data
        for level in &bids {
            let mut price_bytes = [0u8; 32];
            price_bytes[16..32].copy_from_slice(&level.price.raw().to_be_bytes());
            output.extend_from_slice(&price_bytes);

            let mut size_bytes = [0u8; 32];
            size_bytes[16..32].copy_from_slice(&level.size.raw().to_be_bytes());
            output.extend_from_slice(&size_bytes);
        }

        // Asks data
        for level in &asks {
            let mut price_bytes = [0u8; 32];
            price_bytes[16..32].copy_from_slice(&level.price.raw().to_be_bytes());
            output.extend_from_slice(&price_bytes);

            let mut size_bytes = [0u8; 32];
            size_bytes[16..32].copy_from_slice(&level.size.raw().to_be_bytes());
            output.extend_from_slice(&size_bytes);
        }

        Ok(Bytes::from(output))
    }

    // === Spot Precompiles ===

    /// Get spot balance precompile
    /// Input: address (20 bytes) + tokenIndex (1 byte)
    /// Output: ABI-encoded (total, reserved, available)
    fn get_spot_balance(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.len() < 21 {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let spot_engine = self.spot_engine.as_ref()
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Spot engine not available")))?;

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&input[..20]);
        let address = AccountAddress::from(addr_bytes);
        let token_index: TokenIndex = input[20];

        let engine = spot_engine.blocking_read();
        let balance = engine.state.get_balance(address, token_index);

        // Encode as 3 uint256 values (total, reserved, available)
        let mut output = vec![0u8; 96];

        // Total at offset 0
        let total_bytes = balance.total.raw().to_be_bytes();
        output[16..32].copy_from_slice(&total_bytes);

        // Reserved at offset 32
        let reserved_bytes = balance.reserved.raw().to_be_bytes();
        output[48..64].copy_from_slice(&reserved_bytes);

        // Available at offset 64
        let available_bytes = balance.available().raw().to_be_bytes();
        output[80..96].copy_from_slice(&available_bytes);

        Ok(Bytes::from(output))
    }

    /// Get spot market info precompile
    /// Input: marketId (1 byte)
    /// Output: ABI-encoded (baseToken, quoteToken, tickSize, lotSize, bestBid, bestAsk, lastPrice)
    fn get_spot_market(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.is_empty() {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let spot_engine = self.spot_engine.as_ref()
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Spot engine not available")))?;

        let market_id: SpotMarketId = input[0];
        let engine = spot_engine.blocking_read();

        let market = engine.state.get_market(market_id)
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Market not found")))?;

        // Encode market info (7 uint256 values = 224 bytes)
        let mut output = vec![0u8; 224];

        // Base token at offset 0 (as uint256)
        output[31] = market.config.base_token;

        // Quote token at offset 32
        output[63] = market.config.quote_token;

        // Tick size at offset 64
        let tick_bytes = market.config.tick_size.raw().to_be_bytes();
        output[80..96].copy_from_slice(&tick_bytes);

        // Lot size at offset 96
        let lot_bytes = market.config.lot_size.raw().to_be_bytes();
        output[112..128].copy_from_slice(&lot_bytes);

        // Best bid at offset 128
        if let Some(bid) = market.state.best_bid {
            let bid_bytes = bid.raw().to_be_bytes();
            output[144..160].copy_from_slice(&bid_bytes);
        }

        // Best ask at offset 160
        if let Some(ask) = market.state.best_ask {
            let ask_bytes = ask.raw().to_be_bytes();
            output[176..192].copy_from_slice(&ask_bytes);
        }

        // Last price at offset 192
        if let Some(last) = market.state.last_price {
            let last_bytes = last.raw().to_be_bytes();
            output[208..224].copy_from_slice(&last_bytes);
        }

        Ok(Bytes::from(output))
    }

    /// Get spot orderbook precompile
    /// Input: marketId (1 byte) + depth (1 byte)
    /// Output: ABI-encoded L2 snapshot
    fn get_spot_orderbook(&self, input: &Bytes) -> Result<Bytes, PrecompileErrors> {
        if input.len() < 2 {
            return Err(PrecompileErrors::Error(PrecompileError::other("Invalid input length")));
        }

        let spot_engine = self.spot_engine.as_ref()
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Spot engine not available")))?;

        let market_id: SpotMarketId = input[0];
        let depth = input[1].min(20) as usize;

        let engine = spot_engine.blocking_read();

        let book = engine.state.get_orderbook(market_id)
            .ok_or_else(|| PrecompileErrors::Error(PrecompileError::other("Orderbook not found")))?;

        let (bids, asks) = book.get_l2(depth);

        // Encode as simple arrays (same format as perp orderbook)
        let mut output = Vec::with_capacity(64 + (bids.len() + asks.len()) * 64);

        // Bids count
        output.extend_from_slice(&[0u8; 28]);
        output.extend_from_slice(&(bids.len() as u32).to_be_bytes());

        // Asks count
        output.extend_from_slice(&[0u8; 28]);
        output.extend_from_slice(&(asks.len() as u32).to_be_bytes());

        // Bids data
        for level in &bids {
            let mut price_bytes = [0u8; 32];
            price_bytes[16..32].copy_from_slice(&level.price.raw().to_be_bytes());
            output.extend_from_slice(&price_bytes);

            let mut size_bytes = [0u8; 32];
            size_bytes[16..32].copy_from_slice(&level.size.raw().to_be_bytes());
            output.extend_from_slice(&size_bytes);
        }

        // Asks data
        for level in &asks {
            let mut price_bytes = [0u8; 32];
            price_bytes[16..32].copy_from_slice(&level.price.raw().to_be_bytes());
            output.extend_from_slice(&price_bytes);

            let mut size_bytes = [0u8; 32];
            size_bytes[16..32].copy_from_slice(&level.size.raw().to_be_bytes());
            output.extend_from_slice(&size_bytes);
        }

        Ok(Bytes::from(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precompile_addresses() {
        assert_eq!(
            PrecompileAddress::Position.to_address(),
            Address::from([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x00])
        );

        assert_eq!(
            PrecompileAddress::from_address(Address::from([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x01
            ])),
            Some(PrecompileAddress::Account)
        );

        // Test spot precompile addresses
        assert_eq!(
            PrecompileAddress::SpotBalance.to_address(),
            Address::from([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x06])
        );

        assert_eq!(
            PrecompileAddress::from_address(Address::from([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x08
            ])),
            Some(PrecompileAddress::SpotOrderBook)
        );
    }

    #[test]
    fn test_precompile_gas_costs() {
        assert_eq!(PrecompileAddress::Position.gas_cost(), 100);
        assert_eq!(PrecompileAddress::OrderBook.gas_cost(), 500);
    }
}
