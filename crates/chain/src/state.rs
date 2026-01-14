//! Application state management for HyperCore chain
//!
//! Phase 2B: This module now integrates with the unified state model from Phase 2A.
//! The AppState holds references to both perpetuals (EngineState) and spot (SpotEngine)
//! trading, all backed by a single UnifiedState for balance management.

use std::collections::HashMap;
use std::sync::Arc;

use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{
    AccountAddress, BlockHeight, Decimal, MarketId, OrderId,
    SharedUnifiedState, Timestamp, TokenIndex, new_shared_unified_state,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::tx::Transaction;

/// Shared Engine State wrapper (perpetuals)
pub type SharedEngineState = Arc<RwLock<EngineState>>;

/// Shared Spot Engine wrapper (HIP-1 spot trading)
pub type SharedSpotEngine = Arc<RwLock<SpotEngine>>;

/// Application state containing engine state and chain metadata
///
/// ## Phase 2B Architecture
///
/// The AppState now connects to the unified architecture:
/// - `unified_state`: The master balance sheet (shared with EVM)
/// - `engine`: Perpetuals trading engine
/// - `spot_engine`: Spot trading engine (HIP-1 tokens)
///
/// Both engines read from the same UnifiedState, ensuring balance consistency.
pub struct AppState {
    /// Reference to unified state (master balance sheet)
    /// This is the single source of truth for all user balances
    pub unified_state: SharedUnifiedState,
    /// Perpetuals engine state (orderbooks, positions, accounts)
    pub engine: SharedEngineState,
    /// Spot trading engine state
    pub spot_engine: Option<SharedSpotEngine>,
    /// Current block height
    pub height: BlockHeight,
    /// Last block timestamp
    pub timestamp: Timestamp,
    /// Last block app hash
    pub app_hash: [u8; 32],
    /// Nonce tracking per account
    nonces: HashMap<AccountAddress, u64>,
    /// Pending transactions in current block
    pending_txs: Vec<Transaction>,
    /// Committed block hashes
    block_hashes: HashMap<BlockHeight, [u8; 32]>,
}

impl AppState {
    /// Create new app state with default components
    pub fn new() -> Self {
        let unified_state = new_shared_unified_state();
        Self {
            unified_state: Arc::clone(&unified_state),
            engine: Arc::new(RwLock::new(EngineState::new())),
            spot_engine: None,
            height: 0,
            timestamp: 0,
            app_hash: [0u8; 32],
            nonces: HashMap::new(),
            pending_txs: Vec::new(),
            block_hashes: HashMap::new(),
        }
    }

    /// Create app state with shared components (for node integration)
    pub fn with_shared_state(
        unified_state: SharedUnifiedState,
        engine: SharedEngineState,
        spot_engine: Option<SharedSpotEngine>,
    ) -> Self {
        Self {
            unified_state,
            engine,
            spot_engine,
            height: 0,
            timestamp: 0,
            app_hash: [0u8; 32],
            nonces: HashMap::new(),
            pending_txs: Vec::new(),
            block_hashes: HashMap::new(),
        }
    }

    /// Get reference to unified state
    pub fn unified_state(&self) -> &SharedUnifiedState {
        &self.unified_state
    }

    /// Get reference to engine state
    pub fn engine(&self) -> &SharedEngineState {
        &self.engine
    }

    /// Get reference to spot engine
    pub fn spot_engine(&self) -> Option<&SharedSpotEngine> {
        self.spot_engine.as_ref()
    }

    /// Get current nonce for account
    pub fn get_nonce(&self, address: &AccountAddress) -> u64 {
        self.nonces.get(address).copied().unwrap_or(0)
    }

    /// Increment nonce for account
    pub fn increment_nonce(&mut self, address: AccountAddress) {
        let nonce = self.nonces.entry(address).or_insert(0);
        *nonce += 1;
    }

    /// Validate transaction nonce
    pub fn validate_nonce(&self, address: &AccountAddress, nonce: u64) -> bool {
        let expected = self.get_nonce(address);
        // Allow nonces within a window for pending txs
        nonce >= expected && nonce < expected + 100
    }

    /// Validate a transaction
    pub fn validate_tx(&self, tx: &Transaction) -> Result<(), StateError> {
        // Validate nonce
        let sender = tx.sender().map_err(|_| StateError::InvalidSignature)?;
        if !self.validate_nonce(&sender, tx.nonce) {
            return Err(StateError::InvalidNonce);
        }
        Ok(())
    }

    /// Add transaction to pending
    pub fn add_pending_tx(&mut self, tx: Transaction) {
        self.pending_txs.push(tx);
    }

    /// Get pending transactions
    pub fn pending_txs(&self) -> &[Transaction] {
        &self.pending_txs
    }

    /// Clear pending transactions
    pub fn clear_pending(&mut self) {
        self.pending_txs.clear();
    }

    /// Begin block processing
    pub fn begin_block(&mut self, height: BlockHeight, timestamp: Timestamp) {
        self.height = height;
        self.timestamp = timestamp;
        self.pending_txs.clear();
    }

    /// Compute app hash
    pub fn compute_app_hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.app_hash);

        // TODO: Include Merkle root of unified state for proper state commitment

        hasher.finalize().into()
    }

    /// End block processing and compute app hash
    pub fn end_block(&mut self) -> [u8; 32] {
        let app_hash = self.compute_app_hash();
        self.app_hash = app_hash;
        self.block_hashes.insert(self.height, app_hash);
        app_hash
    }

    /// Commit block
    pub fn commit(&mut self) {
        // Persist state if needed
        // For now, state is kept in memory
    }

    /// Get block hash at height
    pub fn get_block_hash(&self, height: BlockHeight) -> Option<[u8; 32]> {
        self.block_hashes.get(&height).copied()
    }

    // === Balance Operations via UnifiedState ===

    /// Credit balance to a user (deposit)
    pub async fn credit_balance(&self, user: AccountAddress, token: TokenIndex, amount: Decimal) {
        let mut state = self.unified_state.write().unwrap();
        state.credit(user, token, amount);
    }

    /// Get user's core view balance (available for trading)
    pub fn get_core_balance(&self, user: AccountAddress, token: TokenIndex) -> Decimal {
        let state = self.unified_state.read().unwrap();
        state.get_core_view(user, token)
    }

    /// Get user's total balance
    pub fn get_total_balance(&self, user: AccountAddress, token: TokenIndex) -> Decimal {
        let state = self.unified_state.read().unwrap();
        state.get_total(user, token)
    }

    /// Transfer from core view to EVM view
    pub fn transfer_to_evm(
        &self,
        user: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> Result<(), StateError> {
        let mut state = self.unified_state.write().unwrap();
        state
            .transfer_to_evm_view(user, token, amount)
            .map_err(|e| StateError::Internal(e.to_string()))
    }

    /// Transfer from EVM view to core view
    pub fn transfer_to_core(
        &self,
        user: AccountAddress,
        token: TokenIndex,
        amount: Decimal,
    ) -> Result<(), StateError> {
        let mut state = self.unified_state.write().unwrap();
        state
            .transfer_to_core_view(user, token, amount)
            .map_err(|e| StateError::Internal(e.to_string()))
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// State errors
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Invalid nonce")]
    InvalidNonce,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Market not found")]
    MarketNotFound,
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Block metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMeta {
    pub height: BlockHeight,
    pub timestamp: Timestamp,
    pub hash: [u8; 32],
    pub proposer: Option<AccountAddress>,
    pub tx_count: u32,
}

/// State diff for a single block
#[derive(Debug, Clone, Default)]
pub struct StateDiff {
    /// Account balance changes
    pub balance_changes: HashMap<AccountAddress, BalanceChange>,
    /// Position changes
    pub position_changes: Vec<PositionChange>,
    /// Order events
    pub order_events: Vec<OrderEvent>,
    /// Fill events
    pub fills: Vec<FillEvent>,
}

#[derive(Debug, Clone)]
pub struct BalanceChange {
    pub address: AccountAddress,
    pub old_balance: Decimal,
    pub new_balance: Decimal,
    pub reason: BalanceChangeReason,
}

#[derive(Debug, Clone)]
pub enum BalanceChangeReason {
    Deposit,
    Withdrawal,
    Trade,
    Funding,
    Liquidation,
    Transfer,
    Fee,
    ViewTransfer, // Added for Core <-> EVM view transfers
}

#[derive(Debug, Clone)]
pub struct PositionChange {
    pub address: AccountAddress,
    pub market_id: MarketId,
    pub old_size: Decimal,
    pub new_size: Decimal,
    pub old_entry_price: Decimal,
    pub new_entry_price: Decimal,
}

#[derive(Debug, Clone)]
pub enum OrderEvent {
    Placed {
        order_id: OrderId,
        owner: AccountAddress,
        market_id: MarketId,
    },
    Canceled {
        order_id: OrderId,
        owner: AccountAddress,
        market_id: MarketId,
    },
    Filled {
        order_id: OrderId,
        owner: AccountAddress,
        fill_size: Decimal,
        fill_price: Decimal,
    },
}

#[derive(Debug, Clone)]
pub struct FillEvent {
    pub maker_order_id: OrderId,
    pub taker_order_id: OrderId,
    pub market_id: MarketId,
    pub price: Decimal,
    pub size: Decimal,
    pub maker: AccountAddress,
    pub taker: AccountAddress,
    pub timestamp: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_management() {
        let mut state = AppState::new();
        let addr = AccountAddress::default();

        assert_eq!(state.get_nonce(&addr), 0);
        assert!(state.validate_nonce(&addr, 0));
        assert!(state.validate_nonce(&addr, 50));
        assert!(!state.validate_nonce(&addr, 100));

        state.increment_nonce(addr);
        assert_eq!(state.get_nonce(&addr), 1);
    }

    #[test]
    fn test_block_processing() {
        let mut state = AppState::new();

        state.begin_block(1, 1000);
        assert_eq!(state.height, 1);
        assert_eq!(state.timestamp, 1000);

        let app_hash = state.end_block();
        assert_ne!(app_hash, [0u8; 32]);

        assert_eq!(state.get_block_hash(1), Some(app_hash));
    }

    #[test]
    fn test_unified_state_integration() {
        let state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Credit balance via unified state
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }

        // Read balance
        let balance = state.get_core_balance(user, 0);
        assert_eq!(balance.raw(), 1000);
    }
}
