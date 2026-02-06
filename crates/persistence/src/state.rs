//! State Serialization and Snapshot Types
//!
//! This module defines the types used for serializing and deserializing
//! the complete application state for persistence.

use hypercore_primitives::{
    AccountAddress, Decimal, MarketId, Order, OrderId, Position, TokenIndex,
};
use serde::{Deserialize, Serialize};

/// Schema version for the persistence format
/// Increment when making breaking changes to the schema
///
/// Version history:
/// - v1: Initial schema with core/perp/spot/evm/chain fields
/// - v2: Unified persistence - perp.* is now the canonical source for all trading
///        state (positions, orders, leverage, markets, scalars). core.* only holds
///        balances and metadata. compute_app_hash() reads from perp_engine directly.
pub const SCHEMA_VERSION: u32 = 2;

/// Complete persisted state that can be serialized/deserialized
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    /// Schema version for migration support
    pub schema_version: u32,

    /// Current block height
    pub height: u64,

    /// Current timestamp
    pub timestamp: u64,

    /// Current app hash
    pub app_hash: [u8; 32],

    /// Core trading state
    pub core: CoreState,

    /// Spot trading state
    pub spot: SpotState,

    /// EVM state
    pub evm: EvmStateData,

    /// Chain metadata
    pub chain: ChainState,

    /// Perpetual engine state (canonical source of truth for all trading state)
    ///
    /// Since schema v2, this is the canonical source for positions, orders, leverage,
    /// markets, and scalars. compute_app_hash() reads directly from the perp_engine.
    /// On restore, this data is loaded into the perp_engine via restore_perp_engine_state().
    /// Falls back to core.* fields for backward compatibility with v1 snapshots.
    #[serde(default)]
    pub perp: PerpState,
}

impl PersistedState {
    /// Create a new empty persisted state
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            height: 0,
            timestamp: 0,
            app_hash: [0u8; 32],
            core: CoreState::default(),
            spot: SpotState::default(),
            evm: EvmStateData::default(),
            chain: ChainState::default(),
            perp: PerpState::default(),
        }
    }

    /// Validate the state invariants
    pub fn validate(&self) -> Result<(), String> {
        // Check schema version
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Schema version mismatch: expected {}, found {}",
                SCHEMA_VERSION, self.schema_version
            ));
        }

        // Validate unified balance invariants
        for balance in &self.core.balances {
            if !balance.balance.is_valid() {
                return Err(format!(
                    "Invalid balance for {:?} token {}: total != core_view + evm_view",
                    balance.account, balance.token
                ));
            }
        }

        Ok(())
    }
}

impl Default for PersistedState {
    fn default() -> Self {
        Self::new()
    }
}

/// Core trading state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoreState {
    /// Unified balances
    pub balances: Vec<BalanceEntry>,

    /// Account states (equity, margin, etc.)
    pub accounts: Vec<AccountEntry>,

    /// Perpetual positions
    pub positions: Vec<PositionEntry>,

    /// Leverage settings per account per market
    pub leverage: Vec<LeverageEntry>,

    /// Market definitions
    pub markets: Vec<MarketEntry>,

    /// Open orders
    pub orders: Vec<OrderEntry>,

    /// Insurance fund balance
    pub insurance_fund: String, // Decimal as string for JSON compatibility

    /// Next order ID
    pub next_order_id: OrderId,
}

/// Spot trading state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpotState {
    /// Spot token definitions
    pub tokens: Vec<SpotTokenEntry>,

    /// Spot market definitions
    pub markets: Vec<SpotMarketEntry>,

    /// Reserved balances for spot orders
    pub reserved: Vec<ReservedEntry>,

    /// Spot orders (separate from perp orders)
    pub orders: Vec<OrderEntry>,

    /// Next token index
    pub next_token_index: TokenIndex,

    /// Next spot market ID
    pub next_market_id: u32,

    /// Next order ID for spot
    pub next_order_id: OrderId,
}

/// EVM state data
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmStateData {
    /// EVM account metadata (nonce, code_hash)
    pub accounts: Vec<EvmAccountEntry>,

    /// Contract storage
    pub storage: Vec<EvmStorageEntry>,

    /// Contract code
    pub code: Vec<EvmCodeEntry>,

    /// EVM block hashes
    pub block_hashes: Vec<EvmBlockHashEntry>,
}

/// Chain/consensus metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainState {
    /// Transaction nonces per account
    pub nonces: Vec<NonceEntry>,

    /// Recent block metadata
    pub blocks: Vec<BlockMetaEntry>,

    /// CLOID to order ID mappings
    pub cloid_index: Vec<CloidEntry>,
}

/// Perpetual engine internal state
///
/// Stored separately from CoreState because the shared EngineState (which CoreState
/// represents) has different contents than the perp_engine's internal EngineState
/// during normal CometBFT operation. The shared EngineState has empty positions/orders/
/// leverage, while the perp_engine tracks them internally.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerpState {
    /// Positions tracked by the perp_engine
    pub positions: Vec<PositionEntry>,

    /// Leverage settings tracked by the perp_engine
    pub leverage: Vec<LeverageEntry>,

    /// Market state from the perp_engine (mark_price, funding, OI)
    pub markets: Vec<MarketEntry>,

    /// Open orders in the perp_engine's orderbooks
    pub orders: Vec<OrderEntry>,

    /// Next order ID from the perp_engine
    pub next_order_id: OrderId,

    /// Insurance fund balance from the perp_engine
    #[serde(default)]
    pub insurance_fund: String,
}

// ==================== ENTRY TYPES ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceEntry {
    pub account: AccountAddress,
    pub token: TokenIndex,
    pub balance: UnifiedBalanceData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedBalanceData {
    pub total: String,      // Decimal as string
    pub core_view: String,  // Decimal as string
    pub evm_view: String,   // Decimal as string
    /// The decimal precision used for this balance (6 for USDC, 18 for other tokens)
    #[serde(default = "default_decimals")]
    pub decimals: u8,
}

fn default_decimals() -> u8 {
    6 // Default to USDC decimals for backwards compatibility
}

impl UnifiedBalanceData {
    /// Check if the balance invariant holds
    pub fn is_valid(&self) -> bool {
        // Parse decimals and check invariant using the stored decimal precision
        let decimals = self.decimals;
        let total = Decimal::from_str_exact(&self.total, decimals);
        let core = Decimal::from_str_exact(&self.core_view, decimals);
        let evm = Decimal::from_str_exact(&self.evm_view, decimals);

        match (total, core, evm) {
            (Some(t), Some(c), Some(e)) => t == c + e,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountEntry {
    pub account: AccountAddress,
    pub equity: String,
    pub margin_used: String,
    pub available_balance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionEntry {
    pub account: AccountAddress,
    pub market: MarketId,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeverageEntry {
    pub account: AccountAddress,
    pub market: MarketId,
    pub leverage: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEntry {
    pub id: MarketId,
    pub symbol: String,
    pub max_leverage: u8,
    pub tick_size: String,
    pub lot_size: String,
    /// Maker fee rate (string for JSON compatibility)
    /// CRITICAL: Must be persisted for consensus - different fee rates cause AppHash mismatch
    #[serde(default)]
    pub maker_fee_rate: String,
    /// Taker fee rate (string for JSON compatibility)
    /// CRITICAL: Must be persisted for consensus - different fee rates cause AppHash mismatch
    #[serde(default)]
    pub taker_fee_rate: String,
    /// Market runtime state (mark price, funding, open interest)
    /// These use #[serde(default)] for backwards compatibility with older snapshots
    #[serde(default)]
    pub mark_price: String,
    #[serde(default)]
    pub index_price: String,
    #[serde(default)]
    pub funding_rate: String,
    #[serde(default)]
    pub funding_accumulator: String,
    #[serde(default)]
    pub open_interest_long: String,
    #[serde(default)]
    pub open_interest_short: String,
    #[serde(default)]
    pub next_funding_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderEntry {
    pub market: MarketId,
    pub order_id: OrderId,
    pub order: Order,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotTokenEntry {
    pub index: TokenIndex,
    pub symbol: String,
    pub name: String,
    pub wei_decimals: u8,
    pub sz_decimals: u8,
    pub max_supply: String,
    pub deployer: AccountAddress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotMarketEntry {
    pub id: u32,
    pub symbol: String,
    pub base_token: TokenIndex,
    pub quote_token: TokenIndex,
    pub tick_size: String,
    pub lot_size: String,
    /// Maker fee rate (string for JSON compatibility)
    /// CRITICAL: Must be persisted for consensus - different fee rates cause AppHash mismatch
    #[serde(default)]
    pub maker_fee_rate: String,
    /// Taker fee rate (string for JSON compatibility)
    /// CRITICAL: Must be persisted for consensus - different fee rates cause AppHash mismatch
    #[serde(default)]
    pub taker_fee_rate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservedEntry {
    pub account: AccountAddress,
    pub token: TokenIndex,
    pub amount: String, // Decimal as string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmAccountEntry {
    pub address: [u8; 20],
    pub nonce: u64,
    pub code_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmStorageEntry {
    pub address: [u8; 20],
    pub slot: [u8; 32],
    pub value: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmCodeEntry {
    pub code_hash: [u8; 32],
    #[serde(with = "hex_bytes")]
    pub bytecode: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmBlockHashEntry {
    pub height: u64,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceEntry {
    pub account: AccountAddress,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetaEntry {
    pub height: u64,
    pub timestamp: u64,
    pub hash: [u8; 32],
    pub app_hash: [u8; 32],
    pub tx_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloidEntry {
    pub account: AccountAddress,
    pub cloid: String,
    pub market: MarketId,
    pub order_id: OrderId,
}

/// Point-in-time snapshot for consistent reads
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    pub height: u64,
    pub timestamp: u64,
    pub app_hash: [u8; 32],
}

// ==================== HEX SERIALIZATION HELPER ====================

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_version() {
        let state = PersistedState::new();
        assert_eq!(state.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn test_balance_validation() {
        let valid = UnifiedBalanceData {
            total: "100".to_string(),
            core_view: "60".to_string(),
            evm_view: "40".to_string(),
            decimals: 6,
        };
        assert!(valid.is_valid());

        let invalid = UnifiedBalanceData {
            total: "100".to_string(),
            core_view: "60".to_string(),
            evm_view: "50".to_string(), // 60 + 50 != 100
            decimals: 6,
        };
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_state_serialization() {
        let state = PersistedState::new();
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: PersistedState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.schema_version, state.schema_version);
    }
}
