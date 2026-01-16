//! State Extraction - Converts runtime state to PersistedState
//!
//! This module provides functions to extract state from various runtime
//! components and convert them to serializable PersistedState format.

use crate::state::{
    AccountEntry, BalanceEntry, BlockMetaEntry, ChainState, CloidEntry, CoreState,
    EvmAccountEntry, EvmBlockHashEntry, EvmCodeEntry, EvmStateData, EvmStorageEntry,
    LeverageEntry, MarketEntry, NonceEntry, OrderEntry, PersistedState, PositionEntry,
    ReservedEntry, SpotMarketEntry, SpotState, SpotTokenEntry, UnifiedBalanceData,
    SCHEMA_VERSION,
};
use hypercore_primitives::{
    AccountAddress, Decimal, MarketId, Order, OrderId, Position, TokenIndex,
    UnifiedBalance, UnifiedState,
};
use std::collections::HashMap;

/// Extract state from UnifiedState to BalanceEntry vector
pub fn extract_unified_balances(state: &UnifiedState) -> Vec<BalanceEntry> {
    state
        .get_all_balances_global()
        .iter()
        .map(|((account, token), balance)| BalanceEntry {
            account: *account,
            token: *token,
            balance: UnifiedBalanceData {
                total: balance.total.to_string_trimmed(),
                core_view: balance.core_view.to_string_trimmed(),
                evm_view: balance.evm_view.to_string_trimmed(),
            },
        })
        .collect()
}

/// Builder for constructing PersistedState from various sources
#[derive(Default)]
pub struct StateExtractor {
    state: PersistedState,
}

impl StateExtractor {
    /// Create a new state extractor
    pub fn new() -> Self {
        Self {
            state: PersistedState::new(),
        }
    }

    /// Set the block height
    pub fn with_height(mut self, height: u64) -> Self {
        self.state.height = height;
        self
    }

    /// Set the timestamp
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.state.timestamp = timestamp;
        self
    }

    /// Set the app hash
    pub fn with_app_hash(mut self, hash: [u8; 32]) -> Self {
        self.state.app_hash = hash;
        self
    }

    /// Add unified balances from UnifiedState
    pub fn with_unified_balances(mut self, state: &UnifiedState) -> Self {
        self.state.core.balances = extract_unified_balances(state);
        self
    }

    /// Add balances directly
    pub fn with_balances(mut self, balances: Vec<BalanceEntry>) -> Self {
        self.state.core.balances = balances;
        self
    }

    /// Add positions
    pub fn with_positions(mut self, positions: Vec<PositionEntry>) -> Self {
        self.state.core.positions = positions;
        self
    }

    /// Add leverage settings
    pub fn with_leverage(mut self, leverage: Vec<LeverageEntry>) -> Self {
        self.state.core.leverage = leverage;
        self
    }

    /// Add markets
    pub fn with_markets(mut self, markets: Vec<MarketEntry>) -> Self {
        self.state.core.markets = markets;
        self
    }

    /// Add orders
    pub fn with_orders(mut self, orders: Vec<OrderEntry>) -> Self {
        self.state.core.orders = orders;
        self
    }

    /// Set next order ID
    pub fn with_next_order_id(mut self, id: OrderId) -> Self {
        self.state.core.next_order_id = id;
        self
    }

    /// Set insurance fund
    pub fn with_insurance_fund(mut self, amount: &str) -> Self {
        self.state.core.insurance_fund = amount.to_string();
        self
    }

    /// Add spot tokens
    pub fn with_spot_tokens(mut self, tokens: Vec<SpotTokenEntry>) -> Self {
        self.state.spot.tokens = tokens;
        self
    }

    /// Add spot markets
    pub fn with_spot_markets(mut self, markets: Vec<SpotMarketEntry>) -> Self {
        self.state.spot.markets = markets;
        self
    }

    /// Add spot reserved balances
    pub fn with_spot_reserved(mut self, reserved: Vec<ReservedEntry>) -> Self {
        self.state.spot.reserved = reserved;
        self
    }

    /// Add spot orders
    pub fn with_spot_orders(mut self, orders: Vec<OrderEntry>) -> Self {
        self.state.spot.orders = orders;
        self
    }

    /// Set spot next token index
    pub fn with_next_token_index(mut self, index: TokenIndex) -> Self {
        self.state.spot.next_token_index = index;
        self
    }

    /// Set spot next market ID
    pub fn with_next_spot_market_id(mut self, id: u32) -> Self {
        self.state.spot.next_market_id = id;
        self
    }

    /// Set spot next order ID
    pub fn with_spot_next_order_id(mut self, id: OrderId) -> Self {
        self.state.spot.next_order_id = id;
        self
    }

    /// Add EVM accounts
    pub fn with_evm_accounts(mut self, accounts: Vec<EvmAccountEntry>) -> Self {
        self.state.evm.accounts = accounts;
        self
    }

    /// Add EVM storage
    pub fn with_evm_storage(mut self, storage: Vec<EvmStorageEntry>) -> Self {
        self.state.evm.storage = storage;
        self
    }

    /// Add EVM code
    pub fn with_evm_code(mut self, code: Vec<EvmCodeEntry>) -> Self {
        self.state.evm.code = code;
        self
    }

    /// Add EVM block hashes
    pub fn with_evm_block_hashes(mut self, hashes: Vec<EvmBlockHashEntry>) -> Self {
        self.state.evm.block_hashes = hashes;
        self
    }

    /// Add nonces
    pub fn with_nonces(mut self, nonces: Vec<NonceEntry>) -> Self {
        self.state.chain.nonces = nonces;
        self
    }

    /// Add block metadata
    pub fn with_blocks(mut self, blocks: Vec<BlockMetaEntry>) -> Self {
        self.state.chain.blocks = blocks;
        self
    }

    /// Add CLOID index
    pub fn with_cloid_index(mut self, cloids: Vec<CloidEntry>) -> Self {
        self.state.chain.cloid_index = cloids;
        self
    }

    /// Build the final PersistedState
    pub fn build(self) -> PersistedState {
        self.state
    }
}

/// Helper to convert HashMap positions to PositionEntry vector
pub fn positions_to_entries(
    positions: &HashMap<AccountAddress, HashMap<MarketId, Position>>,
) -> Vec<PositionEntry> {
    let mut entries = Vec::new();
    for (account, market_positions) in positions {
        for (market, position) in market_positions {
            entries.push(PositionEntry {
                account: *account,
                market: *market,
                position: position.clone(),
            });
        }
    }
    entries
}

/// Helper to convert HashMap leverage to LeverageEntry vector
pub fn leverage_to_entries(
    leverage: &HashMap<AccountAddress, HashMap<MarketId, u8>>,
) -> Vec<LeverageEntry> {
    let mut entries = Vec::new();
    for (account, market_leverage) in leverage {
        for (market, lev) in market_leverage {
            entries.push(LeverageEntry {
                account: *account,
                market: *market,
                leverage: *lev,
            });
        }
    }
    entries
}

/// Helper to convert HashMap orders to OrderEntry vector
pub fn orders_to_entries(
    orders: &HashMap<MarketId, HashMap<OrderId, Order>>,
) -> Vec<OrderEntry> {
    let mut entries = Vec::new();
    for (market, market_orders) in orders {
        for (order_id, order) in market_orders {
            entries.push(OrderEntry {
                market: *market,
                order_id: *order_id,
                order: order.clone(),
            });
        }
    }
    entries
}

/// Helper to convert HashMap nonces to NonceEntry vector
pub fn nonces_to_entries(
    nonces: &HashMap<AccountAddress, u64>,
) -> Vec<NonceEntry> {
    nonces
        .iter()
        .map(|(account, nonce)| NonceEntry {
            account: *account,
            nonce: *nonce,
        })
        .collect()
}

/// Helper to convert CLOID index to CloidEntry vector
pub fn cloid_index_to_entries(
    cloids: &HashMap<(AccountAddress, String), (MarketId, OrderId)>,
) -> Vec<CloidEntry> {
    cloids
        .iter()
        .map(|((account, cloid), (market, order_id))| CloidEntry {
            account: *account,
            cloid: cloid.clone(),
            market: *market,
            order_id: *order_id,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_extractor() {
        let state = StateExtractor::new()
            .with_height(100)
            .with_timestamp(12345)
            .with_next_order_id(500)
            .build();

        assert_eq!(state.height, 100);
        assert_eq!(state.timestamp, 12345);
        assert_eq!(state.core.next_order_id, 500);
    }

    #[test]
    fn test_nonces_to_entries() {
        let mut nonces = HashMap::new();
        nonces.insert(AccountAddress::from([1u8; 20]), 10);
        nonces.insert(AccountAddress::from([2u8; 20]), 20);

        let entries = nonces_to_entries(&nonces);
        assert_eq!(entries.len(), 2);
    }
}
