//! Application state management for HyperCore chain
//!
//! Phase 2B: This module now integrates with the unified state model from Phase 2A.
//! The AppState holds references to both perpetuals (Engine) and spot (SpotEngine)
//! trading, all backed by a single UnifiedState for balance management.

use std::collections::HashMap;
use std::sync::Arc;

use hypercore_engine::{Engine, EngineConfig, EngineState, SpotEngine};
use hypercore_primitives::{
    AccountAddress, BlockHeight, Decimal, MarketId, OrderId,
    SharedUnifiedState, Timestamp, TokenIndex, new_shared_unified_state,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::tx::Transaction;

/// Shared Engine State wrapper (perpetuals) - for read-only access
pub type SharedEngineState = Arc<RwLock<EngineState>>;

/// Shared Engine wrapper (perpetuals) - full orchestrator with matching
pub type SharedEngine = Arc<RwLock<Engine>>;

/// Shared Spot Engine wrapper (HIP-1 spot trading)
pub type SharedSpotEngine = Arc<RwLock<SpotEngine>>;

/// Application state containing engine state and chain metadata
///
/// ## Phase 2B Architecture
///
/// The AppState now connects to the unified architecture:
/// - `unified_state`: The master balance sheet (shared with EVM)
/// - `perp_engine`: Full perpetuals trading engine with matching
/// - `engine`: Legacy EngineState reference (for compatibility/reads)
/// - `spot_engine`: Spot trading engine (HIP-1 tokens)
///
/// Both engines read from the same UnifiedState, ensuring balance consistency.
pub struct AppState {
    /// Reference to unified state (master balance sheet)
    /// This is the single source of truth for all user balances
    pub unified_state: SharedUnifiedState,
    /// Full perpetuals engine with matching, risk, funding, liquidation
    pub perp_engine: Option<SharedEngine>,
    /// Legacy perpetuals engine state reference (for compatibility)
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
    /// Block metadata storage
    block_metadata: HashMap<BlockHeight, BlockMeta>,
    /// Events per block
    block_events: HashMap<BlockHeight, Vec<serde_json::Value>>,
    /// Client Order ID to Order ID mapping (for CancelByCloid)
    cloid_to_oid: HashMap<(AccountAddress, String), (MarketId, OrderId)>,
    /// Last timestamp nonce per account (for timestamp-based nonce validation)
    last_timestamp_nonces: HashMap<AccountAddress, u64>,
    /// Pending block events (accumulated during block execution)
    pending_block_events: Vec<serde_json::Value>,
    /// Trade ID counter for fill events
    next_trade_id: u64,
}

impl AppState {
    /// Create new app state with minimal components (for tests)
    ///
    /// Does not include full perp_engine to avoid blocking issues in async tests.
    /// Use `with_full_engine()` for production node integration.
    pub fn new() -> Self {
        let unified_state = new_shared_unified_state();
        Self {
            unified_state: Arc::clone(&unified_state),
            perp_engine: None, // Not initialized by default for test compatibility
            engine: Arc::new(RwLock::new(EngineState::new())),
            spot_engine: None,
            height: 0,
            timestamp: 0,
            app_hash: [0u8; 32],
            nonces: HashMap::new(),
            pending_txs: Vec::new(),
            block_hashes: HashMap::new(),
            block_metadata: HashMap::new(),
            block_events: HashMap::new(),
            cloid_to_oid: HashMap::new(),
            last_timestamp_nonces: HashMap::new(),
            pending_block_events: Vec::new(),
            next_trade_id: 1,
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
            perp_engine: None, // Will be set separately if needed
            engine,
            spot_engine,
            height: 0,
            timestamp: 0,
            app_hash: [0u8; 32],
            nonces: HashMap::new(),
            pending_txs: Vec::new(),
            block_hashes: HashMap::new(),
            block_metadata: HashMap::new(),
            block_events: HashMap::new(),
            cloid_to_oid: HashMap::new(),
            last_timestamp_nonces: HashMap::new(),
            pending_block_events: Vec::new(),
            next_trade_id: 1,
        }
    }

    /// Create app state with full engine integration
    pub fn with_full_engine(
        unified_state: SharedUnifiedState,
        perp_engine: SharedEngine,
        spot_engine: Option<SharedSpotEngine>,
    ) -> Self {
        // Create a compatible EngineState view for legacy code
        let engine = Arc::new(RwLock::new(EngineState::new()));
        Self {
            unified_state,
            perp_engine: Some(perp_engine),
            engine,
            spot_engine,
            height: 0,
            timestamp: 0,
            app_hash: [0u8; 32],
            nonces: HashMap::new(),
            pending_txs: Vec::new(),
            block_hashes: HashMap::new(),
            block_metadata: HashMap::new(),
            block_events: HashMap::new(),
            cloid_to_oid: HashMap::new(),
            last_timestamp_nonces: HashMap::new(),
            pending_block_events: Vec::new(),
            next_trade_id: 1,
        }
    }

    /// Get the full perpetuals engine
    pub fn perp_engine(&self) -> Option<&SharedEngine> {
        self.perp_engine.as_ref()
    }

    /// Register a client order ID mapping
    pub fn register_cloid(&mut self, owner: AccountAddress, cloid: String, market_id: MarketId, order_id: OrderId) {
        self.cloid_to_oid.insert((owner, cloid), (market_id, order_id));
    }

    /// Lookup order by client order ID
    pub fn get_order_by_cloid(&self, owner: AccountAddress, cloid: &str) -> Option<(MarketId, OrderId)> {
        self.cloid_to_oid.get(&(owner, cloid.to_string())).copied()
    }

    /// Remove client order ID mapping
    pub fn remove_cloid(&mut self, owner: AccountAddress, cloid: &str) {
        self.cloid_to_oid.remove(&(owner, cloid.to_string()));
    }

    /// Store block metadata
    pub fn store_block_metadata(&mut self, meta: BlockMeta) {
        self.block_metadata.insert(meta.height, meta);
    }

    /// Get block metadata
    pub fn get_block_metadata(&self, height: BlockHeight) -> Option<&BlockMeta> {
        self.block_metadata.get(&height)
    }

    /// Store events for a block
    pub fn store_block_events(&mut self, height: BlockHeight, events: Vec<serde_json::Value>) {
        self.block_events.insert(height, events);
    }

    /// Get block events
    pub fn get_block_events(&self, height: BlockHeight) -> Vec<serde_json::Value> {
        self.block_events.get(&height).cloned().unwrap_or_default()
    }

    /// Add a pending block event (accumulated during block execution)
    pub fn add_pending_block_event(&mut self, event: serde_json::Value) {
        self.pending_block_events.push(event);
    }

    /// Take all pending block events (drains the list)
    pub fn take_pending_block_events(&mut self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.pending_block_events)
    }

    /// Get the next trade ID and increment counter
    pub fn next_trade_id(&mut self) -> u64 {
        let id = self.next_trade_id;
        self.next_trade_id += 1;
        id
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
    ///
    /// Supports two nonce schemes:
    /// 1. **Timestamp-based nonces** (recommended): Use current time in milliseconds.
    ///    Must be within 1 hour of current time and greater than last used timestamp nonce.
    /// 2. **Sequential nonces**: Nonces 0, 1, 2, etc. Must be within window of expected nonce.
    ///
    /// The function auto-detects which scheme based on the nonce value:
    /// - If nonce > 1_000_000_000_000 (year 2001+), treat as timestamp
    /// - Otherwise, treat as sequential
    pub fn validate_nonce(&self, address: &AccountAddress, nonce: u64) -> bool {
        // Threshold: timestamps are > 1 trillion (milliseconds since 2001)
        const TIMESTAMP_THRESHOLD: u64 = 1_000_000_000_000;

        if nonce > TIMESTAMP_THRESHOLD {
            // Timestamp-based nonce validation
            let current_time_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            // Must be within 1 hour of current time (past or future)
            let one_hour_ms: u64 = 3_600_000;
            let is_recent = nonce > current_time_ms.saturating_sub(one_hour_ms)
                         && nonce < current_time_ms.saturating_add(one_hour_ms);

            if !is_recent {
                return false;
            }

            // Must be greater than last used timestamp nonce for this account
            let last_timestamp = self.last_timestamp_nonces.get(address).copied().unwrap_or(0);
            nonce > last_timestamp
        } else {
            // Sequential nonce validation
            let expected = self.get_nonce(address);
            // Allow nonces within a window for pending txs
            nonce >= expected && nonce < expected + 100
        }
    }

    /// Update last timestamp nonce for an account (call after successful tx with timestamp nonce)
    pub fn update_timestamp_nonce(&mut self, address: AccountAddress, nonce: u64) {
        const TIMESTAMP_THRESHOLD: u64 = 1_000_000_000_000;
        if nonce > TIMESTAMP_THRESHOLD {
            let entry = self.last_timestamp_nonces.entry(address).or_insert(0);
            if nonce > *entry {
                *entry = nonce;
            }
        }
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

    /// Compute app hash with full state commitment
    ///
    /// This computes a cryptographic commitment to the entire application state:
    /// - Block height and timestamp
    /// - Previous app hash (chain link)
    /// - Unified state Merkle root (account balances)
    /// - Nonce state
    pub fn compute_app_hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();

        // Chain link: height + timestamp + previous hash
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.app_hash);

        // Compute unified state root (simplified Merkle - in production use proper MPT)
        let unified_root = self.compute_unified_state_root();
        hasher.update(&unified_root);

        // Include nonce state
        let nonce_root = self.compute_nonce_root();
        hasher.update(&nonce_root);

        hasher.finalize().into()
    }

    /// Compute Merkle root of unified state
    fn compute_unified_state_root(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        let unified = self.unified_state.read().unwrap();
        let all_balances = unified.get_all_balances_global();

        let mut hasher = Keccak256::new();

        // Sort for deterministic ordering
        let mut entries: Vec<_> = all_balances.iter().collect();
        entries.sort_by_key(|((addr, token), _)| (*addr, *token));

        for ((addr, token), balance) in entries {
            hasher.update(addr.as_slice());
            hasher.update(&token.to_le_bytes());
            hasher.update(&balance.total.raw().to_le_bytes());
            hasher.update(&balance.core_view.raw().to_le_bytes());
            hasher.update(&balance.evm_view.raw().to_le_bytes());
        }

        hasher.finalize().into()
    }

    /// Compute root of nonce state
    fn compute_nonce_root(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();

        // Sort for deterministic ordering
        let mut entries: Vec<_> = self.nonces.iter().collect();
        entries.sort_by_key(|(addr, _)| *addr);

        for (addr, nonce) in entries {
            hasher.update(addr.as_slice());
            hasher.update(&nonce.to_le_bytes());
        }

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

    // === Persistence Helper Methods ===

    /// Get all nonces (for persistence)
    pub fn get_all_nonces(&self) -> &HashMap<AccountAddress, u64> {
        &self.nonces
    }

    /// Get all CLOID mappings (for persistence)
    pub fn get_all_cloid_mappings(&self) -> &HashMap<(AccountAddress, String), (MarketId, OrderId)> {
        &self.cloid_to_oid
    }

    /// Get all block metadata (for persistence)
    pub fn get_all_block_metadata(&self) -> &HashMap<BlockHeight, BlockMeta> {
        &self.block_metadata
    }

    /// Get all block hashes (for persistence)
    pub fn get_all_block_hashes(&self) -> &HashMap<BlockHeight, [u8; 32]> {
        &self.block_hashes
    }

    /// Restore nonces from persistence
    pub fn restore_nonces(&mut self, nonces: HashMap<AccountAddress, u64>) {
        self.nonces = nonces;
    }

    /// Restore CLOID mappings from persistence
    pub fn restore_cloid_mappings(&mut self, cloids: HashMap<(AccountAddress, String), (MarketId, OrderId)>) {
        self.cloid_to_oid = cloids;
    }

    /// Restore block hashes from persistence
    pub fn restore_block_hashes(&mut self, hashes: HashMap<BlockHeight, [u8; 32]>) {
        self.block_hashes = hashes;
    }

    /// Restore block metadata from persistence
    pub fn restore_block_metadata(&mut self, metadata: HashMap<BlockHeight, BlockMeta>) {
        self.block_metadata = metadata;
    }

    /// Restore height and timestamp from persistence
    pub fn restore_chain_state(&mut self, height: BlockHeight, timestamp: Timestamp, app_hash: [u8; 32]) {
        self.height = height;
        self.timestamp = timestamp;
        self.app_hash = app_hash;
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
