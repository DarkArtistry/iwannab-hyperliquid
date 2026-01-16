//! Column Family Definitions for RocksDB
//!
//! Each column family stores a different type of data with its own
//! key-value schema. This allows for logical separation while
//! maintaining atomic operations across families.

/// Column families used in HyperCore persistence
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnFamily {
    /// Default column family (required by RocksDB)
    Default,

    // ==================== UNIFIED STATE ====================
    /// Unified balances: (account, token) -> UnifiedBalance
    /// Key: account_address (20 bytes) + token_index (4 bytes)
    /// Value: bincode(UnifiedBalance)
    Balances,

    // ==================== PERPETUALS ENGINE ====================
    /// Account states: account -> AccountState
    /// Key: account_address (20 bytes)
    /// Value: bincode(AccountState)
    Accounts,

    /// Perpetual positions: (account, market) -> Position
    /// Key: account_address (20 bytes) + market_id (8 bytes)
    /// Value: bincode(Position)
    Positions,

    /// Leverage settings: (account, market) -> leverage
    /// Key: account_address (20 bytes) + market_id (8 bytes)
    /// Value: u8
    Leverage,

    /// Market definitions: market_id -> Market
    /// Key: market_id (8 bytes)
    /// Value: JSON(Market)
    Markets,

    /// All orders (perp + spot): (market_id, order_id) -> Order
    /// Key: market_id (8 bytes) + order_id (8 bytes)
    /// Value: bincode(Order)
    Orders,

    /// Account order index: (account, market) -> Vec<OrderId>
    /// Key: account_address (20 bytes) + market_id (8 bytes)
    /// Value: bincode(Vec<OrderId>)
    AccountOrders,

    // ==================== SPOT ENGINE ====================
    /// Spot token definitions: token_index -> SpotToken
    /// Key: token_index (4 bytes)
    /// Value: JSON(SpotToken)
    SpotTokens,

    /// Spot market definitions: spot_market_id -> SpotMarket
    /// Key: spot_market_id (4 bytes)
    /// Value: JSON(SpotMarket)
    SpotMarkets,

    /// Reserved balances for spot orders: (account, token) -> Decimal
    /// Key: account_address (20 bytes) + token_index (4 bytes)
    /// Value: bincode(Decimal)
    SpotReserved,

    // ==================== EVM STATE ====================
    /// EVM account metadata: address -> EvmAccount
    /// Key: address (20 bytes)
    /// Value: bincode(EvmAccount)
    EvmAccounts,

    /// EVM contract storage: (address, slot) -> value
    /// Key: address (20 bytes) + slot (32 bytes)
    /// Value: U256 (32 bytes)
    EvmStorage,

    /// EVM contract code: code_hash -> bytecode
    /// Key: code_hash (32 bytes)
    /// Value: bytecode (variable)
    EvmCode,

    /// EVM block hashes: height -> hash
    /// Key: height (8 bytes)
    /// Value: hash (32 bytes)
    EvmBlockHashes,

    // ==================== CHAIN/CONSENSUS ====================
    /// Account nonces: account -> nonce
    /// Key: account_address (20 bytes)
    /// Value: u64 (8 bytes)
    Nonces,

    /// Block metadata: height -> BlockMeta
    /// Key: height (8 bytes)
    /// Value: JSON(BlockMeta)
    BlockMeta,

    /// Block hashes: height -> hash
    /// Key: height (8 bytes)
    /// Value: hash (32 bytes)
    BlockHashes,

    /// App state hashes: height -> app_hash
    /// Key: height (8 bytes)
    /// Value: app_hash (32 bytes)
    AppHashes,

    /// CLOID to Order ID mapping: (account, cloid) -> (market, order_id)
    /// Key: account_address (20 bytes) + cloid (variable, max 64 bytes)
    /// Value: market_id (8 bytes) + order_id (8 bytes)
    CloidIndex,

    // ==================== SYSTEM METADATA ====================
    /// System metadata (heights, counters, etc.)
    /// Key: string (e.g., "height", "next_order_id")
    /// Value: varies by key
    Metadata,

    // ==================== HISTORY (OPTIONAL/PRUNABLE) ====================
    /// Fill history: (account, fill_id) -> Fill
    /// Key: account_address (20 bytes) + timestamp (8 bytes) + fill_id (8 bytes)
    /// Value: bincode(Fill)
    Fills,

    /// Funding payments: (account, timestamp) -> FundingPayment
    /// Key: account_address (20 bytes) + timestamp (8 bytes)
    /// Value: bincode(FundingPayment)
    FundingPayments,

    /// Trade feed: (market, timestamp, trade_id) -> Fill
    /// Key: market_id (8 bytes) + timestamp (8 bytes) + trade_id (8 bytes)
    /// Value: bincode(Fill)
    Trades,
}

impl ColumnFamily {
    /// Get the string name for RocksDB
    pub fn name(&self) -> &'static str {
        match self {
            ColumnFamily::Default => "default",
            ColumnFamily::Balances => "balances",
            ColumnFamily::Accounts => "accounts",
            ColumnFamily::Positions => "positions",
            ColumnFamily::Leverage => "leverage",
            ColumnFamily::Markets => "markets",
            ColumnFamily::Orders => "orders",
            ColumnFamily::AccountOrders => "account_orders",
            ColumnFamily::SpotTokens => "spot_tokens",
            ColumnFamily::SpotMarkets => "spot_markets",
            ColumnFamily::SpotReserved => "spot_reserved",
            ColumnFamily::EvmAccounts => "evm_accounts",
            ColumnFamily::EvmStorage => "evm_storage",
            ColumnFamily::EvmCode => "evm_code",
            ColumnFamily::EvmBlockHashes => "evm_block_hashes",
            ColumnFamily::Nonces => "nonces",
            ColumnFamily::BlockMeta => "block_meta",
            ColumnFamily::BlockHashes => "block_hashes",
            ColumnFamily::AppHashes => "app_hashes",
            ColumnFamily::CloidIndex => "cloid_index",
            ColumnFamily::Metadata => "metadata",
            ColumnFamily::Fills => "fills",
            ColumnFamily::FundingPayments => "funding_payments",
            ColumnFamily::Trades => "trades",
        }
    }

    /// Get all column families (for database creation)
    pub fn all() -> &'static [ColumnFamily] {
        &[
            ColumnFamily::Default,
            ColumnFamily::Balances,
            ColumnFamily::Accounts,
            ColumnFamily::Positions,
            ColumnFamily::Leverage,
            ColumnFamily::Markets,
            ColumnFamily::Orders,
            ColumnFamily::AccountOrders,
            ColumnFamily::SpotTokens,
            ColumnFamily::SpotMarkets,
            ColumnFamily::SpotReserved,
            ColumnFamily::EvmAccounts,
            ColumnFamily::EvmStorage,
            ColumnFamily::EvmCode,
            ColumnFamily::EvmBlockHashes,
            ColumnFamily::Nonces,
            ColumnFamily::BlockMeta,
            ColumnFamily::BlockHashes,
            ColumnFamily::AppHashes,
            ColumnFamily::CloidIndex,
            ColumnFamily::Metadata,
            ColumnFamily::Fills,
            ColumnFamily::FundingPayments,
            ColumnFamily::Trades,
        ]
    }

    /// Get all column family names
    pub fn all_names() -> Vec<&'static str> {
        Self::all().iter().map(|cf| cf.name()).collect()
    }

    /// Whether this column family stores critical state (not prunable)
    pub fn is_critical(&self) -> bool {
        match self {
            // Core trading state - never prune
            ColumnFamily::Balances
            | ColumnFamily::Accounts
            | ColumnFamily::Positions
            | ColumnFamily::Leverage
            | ColumnFamily::Markets
            | ColumnFamily::Orders
            | ColumnFamily::AccountOrders
            | ColumnFamily::SpotTokens
            | ColumnFamily::SpotMarkets
            | ColumnFamily::SpotReserved
            | ColumnFamily::EvmAccounts
            | ColumnFamily::EvmStorage
            | ColumnFamily::EvmCode
            | ColumnFamily::Nonces
            | ColumnFamily::Metadata => true,

            // Block metadata - keep recent, can prune old
            ColumnFamily::BlockMeta
            | ColumnFamily::BlockHashes
            | ColumnFamily::AppHashes
            | ColumnFamily::EvmBlockHashes => true, // Keep for now

            // History - can be pruned
            ColumnFamily::Fills
            | ColumnFamily::FundingPayments
            | ColumnFamily::Trades
            | ColumnFamily::CloidIndex
            | ColumnFamily::Default => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_column_families() {
        let all = ColumnFamily::all();
        assert!(!all.is_empty());

        // Verify unique names
        let names: std::collections::HashSet<_> =
            all.iter().map(|cf| cf.name()).collect();
        assert_eq!(names.len(), all.len(), "Duplicate column family names");
    }

    #[test]
    fn test_critical_classification() {
        assert!(ColumnFamily::Balances.is_critical());
        assert!(ColumnFamily::Orders.is_critical());
        assert!(!ColumnFamily::Fills.is_critical());
        assert!(!ColumnFamily::Trades.is_critical());
    }
}
