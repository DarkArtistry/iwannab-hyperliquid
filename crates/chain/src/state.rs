//! Application state management for HyperCore chain
//!
//! Phase 2B: This module now integrates with the unified state model from Phase 2A.
//! The AppState holds references to both perpetuals (Engine) and spot (SpotEngine)
//! trading, all backed by a single UnifiedState for balance management.
//!
//! ## Re-org Support (Phase 7C)
//!
//! This module provides coordinated state snapshots for handling blockchain reorganizations.
//! All three state layers (UnifiedState, EngineState, SpotEngineState) can be captured
//! atomically and restored together when reverting to a previous block.

use std::collections::HashMap;
use std::sync::Arc;

use hypercore_engine::{Engine, EngineConfig, EngineState, EngineStateSnapshot, SpotEngine, SpotEngineStateSnapshot};
use hypercore_primitives::{
    AccountAddress, BlockHeight, Decimal, MarketId, OrderId,
    SharedUnifiedState, Timestamp, TokenIndex, new_shared_unified_state,
    UnifiedStateSnapshot,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::merkle::{MerkleTree, MerkleProof, hash_entry};
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
    /// Cached unified state Merkle tree (rebuilt on end_block)
    unified_state_tree: Option<MerkleTree>,
    /// Cached nonce Merkle tree (rebuilt on end_block)
    nonce_tree: Option<MerkleTree>,
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
            unified_state_tree: None,
            nonce_tree: None,
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
            unified_state_tree: None,
            nonce_tree: None,
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
            unified_state_tree: None,
            nonce_tree: None,
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
    ///    Must be within 1 hour of reference time and greater than last used timestamp nonce.
    /// 2. **Sequential nonces**: Nonces 0, 1, 2, etc. Must be within window of expected nonce.
    ///
    /// The function auto-detects which scheme based on the nonce value:
    /// - If nonce > 1_000_000_000_000 (year 2001+), treat as timestamp
    /// - Otherwise, treat as sequential
    ///
    /// # Parameters
    /// - `address`: The account address
    /// - `nonce`: The nonce value from the transaction
    /// - `reference_time_ms`: The reference time in milliseconds for timestamp validation.
    ///   For consensus-critical validation (DeliverTx), this MUST be the block timestamp.
    ///   For mempool validation (CheckTx), this can be the node's current time.
    ///
    /// # Determinism
    /// This function is deterministic given the same reference_time_ms input.
    /// CRITICAL: Never use SystemTime::now() here - it would break consensus.
    pub fn validate_nonce(&self, address: &AccountAddress, nonce: u64, reference_time_ms: u64) -> bool {
        // Threshold: timestamps are > 1 trillion (milliseconds since 2001)
        const TIMESTAMP_THRESHOLD: u64 = 1_000_000_000_000;

        if nonce > TIMESTAMP_THRESHOLD {
            // Timestamp-based nonce validation
            // Use provided reference time instead of SystemTime::now() for determinism
            let current_time_ms = reference_time_ms;

            // Must be within 1 hour of reference time (past or future)
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
    ///
    /// # Parameters
    /// - `tx`: The transaction to validate
    /// - `reference_time_ms`: Reference time for timestamp-based nonce validation.
    ///   For DeliverTx (block execution), pass the block timestamp.
    ///   For CheckTx (mempool), pass the node's current time.
    pub fn validate_tx(&self, tx: &Transaction, reference_time_ms: u64) -> Result<(), StateError> {
        // Validate nonce
        let sender = tx.sender().map_err(|_| StateError::InvalidSignature)?;
        if !self.validate_nonce(&sender, tx.nonce, reference_time_ms) {
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
    /// This computes a cryptographic commitment to the entire application state.
    /// ALL consensus-critical state MUST be included for validators to detect divergence.
    ///
    /// ## Included State (Phase 3C-extended)
    ///
    /// | Component | Method | Description |
    /// |-----------|--------|-------------|
    /// | Chain metadata | height, timestamp, prev_hash | Block chain link |
    /// | Unified balances | compute_unified_state_root() | User balances (core+evm views) |
    /// | Nonces | compute_nonce_root() | Transaction replay protection |
    /// | Positions | compute_positions_root() | Open perpetual positions |
    /// | Orders | compute_orders_root() | Open orders |
    /// | Markets | compute_markets_root() | Market state (prices, funding, OI) |
    /// | Leverage | compute_leverage_root() | User leverage settings |
    /// | CLOIDs | compute_cloid_root() | Client order ID mappings |
    /// | Engine scalars | compute_engine_scalars_hash() | Insurance fund, next order ID |
    ///
    /// ## Determinism
    /// This function is fully deterministic. All inputs are sorted before hashing.
    /// No floating point, no SystemTime, no HashMap iteration (sorted).
    pub fn compute_app_hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();

        // === Chain link ===
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.app_hash); // Previous app hash

        // === Unified State (balances) ===
        let unified_root = self.compute_unified_state_root();
        hasher.update(&unified_root);

        // === Nonces (replay protection) ===
        let nonce_root = self.compute_nonce_root();
        hasher.update(&nonce_root);

        // === Engine State (Phase 3C-extended) ===
        // These were previously missing, causing potential undetected divergence

        // Positions
        let positions_root = self.compute_positions_root();
        hasher.update(&positions_root);

        // Open orders
        let orders_root = self.compute_orders_root();
        hasher.update(&orders_root);

        // Market state (prices, funding, OI)
        let markets_root = self.compute_markets_root();
        hasher.update(&markets_root);

        // Leverage settings
        let leverage_root = self.compute_leverage_root();
        hasher.update(&leverage_root);

        // Client order ID mappings
        let cloid_root = self.compute_cloid_root();
        hasher.update(&cloid_root);

        // Scalar values (insurance fund, next order ID)
        let scalars_hash = self.compute_engine_scalars_hash();
        hasher.update(&scalars_hash);

        // TODO: Add EVM state roots when EVM execution becomes consensus-critical
        // - EVM accounts root
        // - EVM storage root
        // - EVM code root

        hasher.finalize().into()
    }

    /// Build Merkle tree of unified state and return the root
    ///
    /// Phase 3C: Uses a proper binary Merkle tree instead of simple hashing.
    /// This enables proof generation for light client verification.
    fn build_unified_state_tree(&self) -> MerkleTree {
        let unified = self.unified_state.read().unwrap();
        let all_balances = unified.get_all_balances_global();

        // Sort for deterministic ordering
        let mut entries: Vec<_> = all_balances.iter().collect();
        entries.sort_by_key(|((addr, token), _)| (*addr, *token));

        // Build key-value pairs for Merkle tree
        // Key: address (20 bytes) || token (4 bytes) = 24 bytes
        // Value: total (16 bytes) || core_view (16 bytes) || evm_view (16 bytes) = 48 bytes
        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|((addr, token), balance)| {
                let mut key = Vec::with_capacity(24);
                key.extend_from_slice(addr.as_slice());
                key.extend_from_slice(&token.to_le_bytes());

                let mut value = Vec::with_capacity(48);
                value.extend_from_slice(&balance.total.raw().to_le_bytes());
                value.extend_from_slice(&balance.core_view.raw().to_le_bytes());
                value.extend_from_slice(&balance.evm_view.raw().to_le_bytes());

                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries)
    }

    /// Compute Merkle root of unified state
    fn compute_unified_state_root(&self) -> [u8; 32] {
        self.build_unified_state_tree().root()
    }

    /// Build Merkle tree of nonce state and return the root
    fn build_nonce_tree(&self) -> MerkleTree {
        // Sort for deterministic ordering
        let mut entries: Vec<_> = self.nonces.iter().collect();
        entries.sort_by_key(|(addr, _)| *addr);

        // Build key-value pairs for Merkle tree
        // Key: address (20 bytes)
        // Value: nonce (8 bytes)
        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|(addr, nonce)| {
                let key = addr.as_slice().to_vec();
                let value = nonce.to_le_bytes().to_vec();
                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries)
    }

    /// Compute root of nonce state
    fn compute_nonce_root(&self) -> [u8; 32] {
        self.build_nonce_tree().root()
    }

    // === Engine State Merkle Trees (Phase 3C-extended) ===
    //
    // These methods compute Merkle roots for consensus-critical EngineState
    // components that were previously missing from the AppHash commitment.

    /// Compute Merkle root of all positions
    ///
    /// Key: address (20 bytes) || market_id (1 byte) = 21 bytes
    /// Value: size (16 bytes) || entry_notional (16 bytes) || realized_pnl (16 bytes) || funding_index (16 bytes) = 64 bytes
    pub fn compute_positions_root(&self) -> [u8; 32] {
        // Use try_read() to handle both sync and async contexts
        // This can panic if a write lock is held, but should not happen at end_block
        let engine = self.engine.try_read()
            .expect("Engine lock should not be held during state commitment");
        let all_positions = engine.get_all_positions_global();

        // Flatten and sort for deterministic ordering
        let mut entries: Vec<_> = all_positions
            .iter()
            .flat_map(|(addr, positions)| {
                positions.iter().map(move |(market_id, pos)| (*addr, *market_id, pos))
            })
            .collect();
        entries.sort_by_key(|(addr, mid, _)| (*addr, *mid));

        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|(addr, market_id, pos)| {
                let mut key = Vec::with_capacity(21);
                key.extend_from_slice(addr.as_slice());
                key.push(*market_id);

                // Use entry_notional instead of entry_price (entry_price is computed)
                let mut value = Vec::with_capacity(64);
                value.extend_from_slice(&pos.size.raw().to_le_bytes());
                value.extend_from_slice(&pos.entry_notional.raw().to_le_bytes());
                value.extend_from_slice(&pos.realized_pnl.raw().to_le_bytes());
                value.extend_from_slice(&pos.last_funding_index.raw().to_le_bytes());

                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries).root()
    }

    /// Compute Merkle root of all open orders
    ///
    /// Key: market_id (1 byte) || order_id (8 bytes) = 9 bytes
    /// Value: owner (20) || side (1) || price (16) || size (16) || remaining (16) = 69 bytes
    pub fn compute_orders_root(&self) -> [u8; 32] {
        let engine = self.engine.try_read()
            .expect("Engine lock should not be held during state commitment");
        let all_orders = engine.get_all_orders_global();

        // Flatten all orders and sort for deterministic ordering
        let mut entries: Vec<_> = all_orders
            .iter()
            .flat_map(|(market_id, orders)| {
                orders.iter().map(move |(order_id, order)| (*market_id, *order_id, order))
            })
            .collect();

        // Sort by (market_id, order_id) for deterministic ordering
        entries.sort_by_key(|(mid, oid, _)| (*mid, *oid));

        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|(market_id, order_id, order)| {
                // Key: market_id (1 byte) || order_id (8 bytes)
                let mut key = Vec::with_capacity(9);
                key.push(*market_id);
                key.extend_from_slice(&order_id.to_le_bytes());

                // Value: owner (20) || side (1) || price (16) || original_size (16) || remaining_size (16)
                let mut value = Vec::with_capacity(69);
                value.extend_from_slice(order.owner.as_slice());
                value.push(match order.side {
                    hypercore_primitives::OrderSide::Buy => 0,
                    hypercore_primitives::OrderSide::Sell => 1,
                });
                value.extend_from_slice(&order.price.raw().to_le_bytes());
                value.extend_from_slice(&order.original_size.raw().to_le_bytes());
                value.extend_from_slice(&order.remaining_size.raw().to_le_bytes());

                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries).root()
    }

    /// Compute Merkle root of all markets
    ///
    /// Key: market_id (1 byte)
    /// Value: mark_price (16) || index_price (16) || funding_rate (16) || oi_long (16) || oi_short (16) = 80 bytes
    pub fn compute_markets_root(&self) -> [u8; 32] {
        let engine = self.engine.try_read()
            .expect("Engine lock should not be held during state commitment");
        let markets = engine.get_all_markets();

        // Sort by market ID for deterministic ordering
        let mut market_vec: Vec<_> = markets.iter().collect();
        market_vec.sort_by_key(|m| m.id());

        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = market_vec
            .iter()
            .map(|market| {
                let key = vec![market.id()];

                let mut value = Vec::with_capacity(80);
                value.extend_from_slice(&market.state.mark_price.raw().to_le_bytes());
                value.extend_from_slice(&market.state.index_price.raw().to_le_bytes());
                value.extend_from_slice(&market.state.funding_rate.raw().to_le_bytes());
                value.extend_from_slice(&market.state.open_interest_long.raw().to_le_bytes());
                value.extend_from_slice(&market.state.open_interest_short.raw().to_le_bytes());

                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries).root()
    }

    /// Compute Merkle root of leverage settings
    ///
    /// Key: address (20 bytes) || market_id (1 byte) = 21 bytes
    /// Value: leverage (1 byte)
    pub fn compute_leverage_root(&self) -> [u8; 32] {
        let engine = self.engine.try_read()
            .expect("Engine lock should not be held during state commitment");
        let all_leverage = engine.get_all_leverage_global();

        // Flatten and sort for deterministic ordering
        let mut entries: Vec<_> = all_leverage
            .iter()
            .flat_map(|(addr, markets)| {
                markets.iter().map(move |(market_id, lev)| (*addr, *market_id, *lev))
            })
            .collect();
        entries.sort_by_key(|(addr, mid, _)| (*addr, *mid));

        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|(addr, market_id, leverage)| {
                let mut key = Vec::with_capacity(21);
                key.extend_from_slice(addr.as_slice());
                key.push(*market_id);

                let value = vec![*leverage];

                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries).root()
    }

    /// Compute Merkle root of CLOID mappings
    ///
    /// Key: address (20 bytes) || cloid_hash (32 bytes) = 52 bytes
    /// Value: market_id (1 byte) || order_id (8 bytes) = 9 bytes
    pub fn compute_cloid_root(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        // Sort for deterministic ordering
        let mut entries: Vec<_> = self.cloid_to_oid.iter().collect();
        entries.sort_by_key(|((addr, cloid), _)| (*addr, cloid.clone()));

        let merkle_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .iter()
            .map(|((addr, cloid), (market_id, order_id))| {
                // Hash the cloid string for fixed-length key
                let cloid_hash: [u8; 32] = Keccak256::digest(cloid.as_bytes()).into();

                let mut key = Vec::with_capacity(52);
                key.extend_from_slice(addr.as_slice());
                key.extend_from_slice(&cloid_hash);

                let mut value = Vec::with_capacity(9);
                value.push(*market_id);
                value.extend_from_slice(&order_id.to_le_bytes());

                (key, value)
            })
            .collect();

        MerkleTree::from_entries(&merkle_entries).root()
    }

    /// Compute a single hash for scalar engine state values
    ///
    /// Includes: insurance_fund, next_order_id
    pub fn compute_engine_scalars_hash(&self) -> [u8; 32] {
        use sha3::{Digest, Keccak256};

        let engine = self.engine.try_read()
            .expect("Engine lock should not be held during state commitment");

        let mut hasher = Keccak256::new();
        hasher.update(&engine.insurance_fund.to_le_bytes());
        hasher.update(&engine.peek_next_order_id().to_le_bytes());

        hasher.finalize().into()
    }

    /// End block processing and compute app hash
    ///
    /// Phase 3C: Now caches Merkle trees for proof generation.
    pub fn end_block(&mut self) -> [u8; 32] {
        // Build and cache Merkle trees
        self.unified_state_tree = Some(self.build_unified_state_tree());
        self.nonce_tree = Some(self.build_nonce_tree());

        let app_hash = self.compute_app_hash();
        self.app_hash = app_hash;
        self.block_hashes.insert(self.height, app_hash);
        app_hash
    }

    // === State Proof Methods (Phase 3C) ===

    /// Get the unified state Merkle root
    pub fn get_unified_state_root(&self) -> [u8; 32] {
        self.unified_state_tree.as_ref()
            .map(|t| t.root())
            .unwrap_or_else(|| self.compute_unified_state_root())
    }

    /// Get the nonce Merkle root
    pub fn get_nonce_root(&self) -> [u8; 32] {
        self.nonce_tree.as_ref()
            .map(|t| t.root())
            .unwrap_or_else(|| self.compute_nonce_root())
    }

    /// Generate a Merkle proof for a balance entry
    ///
    /// Returns the proof if the balance exists, None otherwise.
    /// This enables light client verification of specific balances.
    pub fn prove_balance(&self, address: AccountAddress, token: TokenIndex) -> Option<MerkleProof> {
        let tree = self.unified_state_tree.as_ref()?;

        // Build the expected key to find the leaf index
        let unified = self.unified_state.read().unwrap();
        let all_balances = unified.get_all_balances_global();

        // Sort entries to match tree order
        let mut entries: Vec<_> = all_balances.iter().collect();
        entries.sort_by_key(|((addr, tok), _)| (*addr, *tok));

        // Find the index of the requested balance
        let leaf_index = entries.iter()
            .position(|((addr, tok), _)| *addr == address && *tok == token)?;

        tree.prove(leaf_index)
    }

    /// Verify a balance proof against the current state root
    ///
    /// This is the primary method for light client verification.
    /// Returns true if the proof is valid for the current state.
    pub fn verify_balance_proof(
        &self,
        address: AccountAddress,
        token: TokenIndex,
        balance: &hypercore_primitives::UnifiedBalance,
        proof: &MerkleProof,
    ) -> bool {
        // Reconstruct the expected leaf hash
        let mut key = Vec::with_capacity(24);
        key.extend_from_slice(address.as_slice());
        key.extend_from_slice(&token.to_le_bytes());

        let mut value = Vec::with_capacity(48);
        value.extend_from_slice(&balance.total.raw().to_le_bytes());
        value.extend_from_slice(&balance.core_view.raw().to_le_bytes());
        value.extend_from_slice(&balance.evm_view.raw().to_le_bytes());

        let expected_leaf = hash_entry(&key, &value);

        // Verify the proof matches both the leaf and the root
        proof.leaf_hash == expected_leaf && proof.verify(&self.get_unified_state_root())
    }

    /// Generate a Merkle proof for a nonce entry
    pub fn prove_nonce(&self, address: AccountAddress) -> Option<MerkleProof> {
        let tree = self.nonce_tree.as_ref()?;

        // Sort entries to match tree order
        let mut entries: Vec<_> = self.nonces.iter().collect();
        entries.sort_by_key(|(addr, _)| *addr);

        // Find the index of the requested nonce
        let leaf_index = entries.iter()
            .position(|(addr, _)| **addr == address)?;

        tree.prove(leaf_index)
    }

    /// Verify a nonce proof against the current state root
    pub fn verify_nonce_proof(
        &self,
        address: AccountAddress,
        nonce: u64,
        proof: &MerkleProof,
    ) -> bool {
        let key = address.as_slice().to_vec();
        let value = nonce.to_le_bytes().to_vec();

        let expected_leaf = hash_entry(&key, &value);
        proof.leaf_hash == expected_leaf && proof.verify(&self.get_nonce_root())
    }

    /// Get the number of balance entries in the unified state
    pub fn balance_count(&self) -> usize {
        self.unified_state_tree.as_ref()
            .map(|t| t.leaf_count())
            .unwrap_or(0)
    }

    /// Get the number of nonce entries
    pub fn nonce_count(&self) -> usize {
        self.nonce_tree.as_ref()
            .map(|t| t.leaf_count())
            .unwrap_or(0)
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

    // ========================================================================
    // Re-org Support: State Snapshot & Restore (Phase 7C)
    // ========================================================================

    /// Create a coordinated snapshot of all state for re-org handling
    ///
    /// This captures the complete state of:
    /// - UnifiedState (master balance sheet)
    /// - EngineState (perpetual positions, orders, markets)
    /// - SpotEngineState (spot tokens, markets, orders) if present
    /// - AppState-specific fields (nonces, heights, etc.)
    ///
    /// The snapshot can be used to revert state during a blockchain reorganization.
    pub async fn snapshot(&self) -> AppStateSnapshot {
        // Snapshot unified state (balances)
        let unified_snapshot = {
            let state = self.unified_state.read().unwrap();
            state.snapshot()
        };

        // Snapshot engine state (perp positions, orders)
        let engine_snapshot = {
            let state = self.engine.read().await;
            state.snapshot()
        };

        // Snapshot spot engine state if present
        let spot_engine_snapshot = if let Some(ref spot_engine) = self.spot_engine {
            let spot = spot_engine.read().await;
            Some(spot.state.snapshot())
        } else {
            None
        };

        AppStateSnapshot {
            // Core snapshots for all three state layers
            unified_snapshot,
            engine_snapshot,
            spot_engine_snapshot,
            // AppState-specific fields
            height: self.height,
            timestamp: self.timestamp,
            app_hash: self.app_hash,
            nonces: self.nonces.clone(),
            block_hashes: self.block_hashes.clone(),
            block_metadata: self.block_metadata.clone(),
            block_events: self.block_events.clone(),
            cloid_to_oid: self.cloid_to_oid.clone(),
            last_timestamp_nonces: self.last_timestamp_nonces.clone(),
            next_trade_id: self.next_trade_id,
        }
    }

    /// Restore state from a snapshot during re-org
    ///
    /// This atomically restores all three state layers:
    /// - UnifiedState (balances)
    /// - EngineState (perp state)
    /// - SpotEngineState (spot state) if present
    ///
    /// After restoration, the state will be identical to when the snapshot was taken.
    ///
    /// WARNING: This is a destructive operation. Any state changes since the
    /// snapshot was taken will be lost.
    pub async fn restore(&mut self, snapshot: AppStateSnapshot) {
        // Restore unified state (balances)
        {
            let mut state = self.unified_state.write().unwrap();
            state.restore(snapshot.unified_snapshot);
        }

        // Restore engine state (perp positions, orders)
        {
            let mut state = self.engine.write().await;
            state.restore(snapshot.engine_snapshot);
        }

        // Restore spot engine state if present
        if let (Some(ref spot_engine), Some(spot_snapshot)) = (&self.spot_engine, snapshot.spot_engine_snapshot) {
            let mut spot = spot_engine.write().await;
            spot.state.restore(spot_snapshot);
        }

        // Restore AppState-specific fields
        self.height = snapshot.height;
        self.timestamp = snapshot.timestamp;
        self.app_hash = snapshot.app_hash;
        self.nonces = snapshot.nonces;
        self.block_hashes = snapshot.block_hashes;
        self.block_metadata = snapshot.block_metadata;
        self.block_events = snapshot.block_events;
        self.cloid_to_oid = snapshot.cloid_to_oid;
        self.last_timestamp_nonces = snapshot.last_timestamp_nonces;
        self.next_trade_id = snapshot.next_trade_id;

        // Clear pending state (it's invalid after restore)
        self.pending_txs.clear();
        self.pending_block_events.clear();
        self.unified_state_tree = None;
        self.nonce_tree = None;

        tracing::info!(
            "State restored to height {} (app_hash: {})",
            self.height,
            hex::encode(&self.app_hash[..8])
        );
    }

    /// Create a snapshot synchronously (blocking version)
    ///
    /// This is useful when you don't have an async context.
    /// Internally uses blocking locks on async RwLocks.
    pub fn snapshot_blocking(&self) -> AppStateSnapshot {
        // Snapshot unified state (balances) - uses std::sync::RwLock
        let unified_snapshot = {
            let state = self.unified_state.read().unwrap();
            state.snapshot()
        };

        // Snapshot engine state (perp positions, orders) - uses tokio::sync::RwLock
        let engine_snapshot = {
            let state = self.engine.blocking_read();
            state.snapshot()
        };

        // Snapshot spot engine state if present
        let spot_engine_snapshot = if let Some(ref spot_engine) = self.spot_engine {
            let spot = spot_engine.blocking_read();
            Some(spot.state.snapshot())
        } else {
            None
        };

        AppStateSnapshot {
            unified_snapshot,
            engine_snapshot,
            spot_engine_snapshot,
            height: self.height,
            timestamp: self.timestamp,
            app_hash: self.app_hash,
            nonces: self.nonces.clone(),
            block_hashes: self.block_hashes.clone(),
            block_metadata: self.block_metadata.clone(),
            block_events: self.block_events.clone(),
            cloid_to_oid: self.cloid_to_oid.clone(),
            last_timestamp_nonces: self.last_timestamp_nonces.clone(),
            next_trade_id: self.next_trade_id,
        }
    }

    /// Restore state from a snapshot synchronously (blocking version)
    pub fn restore_blocking(&mut self, snapshot: AppStateSnapshot) {
        // Restore unified state
        {
            let mut state = self.unified_state.write().unwrap();
            state.restore(snapshot.unified_snapshot);
        }

        // Restore engine state
        {
            let mut state = self.engine.blocking_write();
            state.restore(snapshot.engine_snapshot);
        }

        // Restore spot engine state if present
        if let (Some(ref spot_engine), Some(spot_snapshot)) = (&self.spot_engine, snapshot.spot_engine_snapshot) {
            let mut spot = spot_engine.blocking_write();
            spot.state.restore(spot_snapshot);
        }

        // Restore AppState fields
        self.height = snapshot.height;
        self.timestamp = snapshot.timestamp;
        self.app_hash = snapshot.app_hash;
        self.nonces = snapshot.nonces;
        self.block_hashes = snapshot.block_hashes;
        self.block_metadata = snapshot.block_metadata;
        self.block_events = snapshot.block_events;
        self.cloid_to_oid = snapshot.cloid_to_oid;
        self.last_timestamp_nonces = snapshot.last_timestamp_nonces;
        self.next_trade_id = snapshot.next_trade_id;

        // Clear pending state
        self.pending_txs.clear();
        self.pending_block_events.clear();
        self.unified_state_tree = None;
        self.nonce_tree = None;

        tracing::info!(
            "State restored (blocking) to height {} (app_hash: {})",
            self.height,
            hex::encode(&self.app_hash[..8])
        );
    }
}

/// Coordinated snapshot of all application state for re-org handling
///
/// This struct captures the complete state of all three layers:
/// 1. **UnifiedState** - Master balance sheet (Core + EVM views)
/// 2. **EngineState** - Perpetual trading state (positions, orders, markets)
/// 3. **SpotEngineState** - Spot trading state (tokens, markets, orders)
///
/// Plus AppState-specific fields like nonces, heights, and metadata.
///
/// ## Usage
///
/// ```ignore
/// // Take a snapshot before processing a block
/// let snapshot = app_state.snapshot().await;
///
/// // Process transactions...
/// // If we need to revert (e.g., re-org):
/// app_state.restore(snapshot).await;
/// ```
#[derive(Debug, Clone)]
pub struct AppStateSnapshot {
    // Core snapshots for all three state layers
    /// Snapshot of unified state (all balances)
    pub unified_snapshot: UnifiedStateSnapshot,
    /// Snapshot of engine state (perp positions, orders, markets)
    pub engine_snapshot: EngineStateSnapshot,
    /// Snapshot of spot engine state (optional - may not be present)
    pub spot_engine_snapshot: Option<SpotEngineStateSnapshot>,

    // AppState-specific fields
    /// Block height at snapshot time
    pub height: BlockHeight,
    /// Timestamp at snapshot time
    pub timestamp: Timestamp,
    /// App hash at snapshot time
    pub app_hash: [u8; 32],
    /// Nonces at snapshot time
    pub nonces: HashMap<AccountAddress, u64>,
    /// Block hashes
    pub block_hashes: HashMap<BlockHeight, [u8; 32]>,
    /// Block metadata
    pub block_metadata: HashMap<BlockHeight, BlockMeta>,
    /// Block events
    pub block_events: HashMap<BlockHeight, Vec<serde_json::Value>>,
    /// Client order ID mappings
    pub cloid_to_oid: HashMap<(AccountAddress, String), (MarketId, OrderId)>,
    /// Last timestamp nonces
    pub last_timestamp_nonces: HashMap<AccountAddress, u64>,
    /// Next trade ID counter
    pub next_trade_id: u64,
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
        // Use a fixed reference time for deterministic testing
        let ref_time_ms: u64 = 1700000000000; // Arbitrary fixed timestamp

        assert_eq!(state.get_nonce(&addr), 0);
        assert!(state.validate_nonce(&addr, 0, ref_time_ms));
        assert!(state.validate_nonce(&addr, 50, ref_time_ms));
        assert!(!state.validate_nonce(&addr, 100, ref_time_ms));

        state.increment_nonce(addr);
        assert_eq!(state.get_nonce(&addr), 1);
    }

    #[test]
    fn test_timestamp_nonce_determinism() {
        // This test verifies that timestamp-based nonce validation is deterministic
        // given the same reference_time_ms input.
        let state = AppState::new();
        let addr = AccountAddress::from([0x42u8; 20]);
        let ref_time_ms: u64 = 1700000000000; // Fixed reference time

        // Timestamp nonce within 1 hour of reference time should be valid
        let valid_nonce = ref_time_ms + 1000; // 1 second after reference
        assert!(state.validate_nonce(&addr, valid_nonce, ref_time_ms));

        // Same call with same inputs should produce same result (deterministic)
        assert!(state.validate_nonce(&addr, valid_nonce, ref_time_ms));
        assert!(state.validate_nonce(&addr, valid_nonce, ref_time_ms));

        // Nonce too far in the past should be invalid
        let old_nonce = ref_time_ms - 4_000_000; // > 1 hour ago
        assert!(!state.validate_nonce(&addr, old_nonce, ref_time_ms));

        // Nonce too far in the future should be invalid
        let future_nonce = ref_time_ms + 4_000_000; // > 1 hour ahead
        assert!(!state.validate_nonce(&addr, future_nonce, ref_time_ms));
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

    // === Phase 3C: State Proof Tests ===

    #[test]
    fn test_merkle_tree_caching() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Credit balance
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }

        // Before end_block, trees should be None
        assert!(state.unified_state_tree.is_none());
        assert!(state.nonce_tree.is_none());

        // Process block
        state.begin_block(1, 1000);
        state.increment_nonce(user);
        state.end_block();

        // After end_block, trees should be cached
        assert!(state.unified_state_tree.is_some());
        assert!(state.nonce_tree.is_some());
    }

    #[test]
    fn test_balance_proof_generation() {
        let mut state = AppState::new();
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Credit balances for multiple users
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
            unified.credit(user1, 1, Decimal::from_raw(500, 6)); // Different token
        }

        // Process block to build Merkle trees
        state.begin_block(1, 1000);
        state.end_block();

        // Generate proofs for all balances
        let proof1 = state.prove_balance(user1, 0);
        assert!(proof1.is_some(), "Should be able to prove user1 token 0");

        let proof2 = state.prove_balance(user2, 0);
        assert!(proof2.is_some(), "Should be able to prove user2 token 0");

        let proof3 = state.prove_balance(user1, 1);
        assert!(proof3.is_some(), "Should be able to prove user1 token 1");

        // Proof for non-existent balance should be None
        let proof_none = state.prove_balance(user2, 1);
        assert!(proof_none.is_none(), "Non-existent balance should return None");
    }

    #[test]
    fn test_balance_proof_verification() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);
        let amount = Decimal::from_raw(1000, 6);

        // Credit balance
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, amount);
        }

        // Process block
        state.begin_block(1, 1000);
        state.end_block();

        // Generate and verify proof
        let proof = state.prove_balance(user, 0).unwrap();

        // Get the actual balance
        let balance = {
            let unified = state.unified_state.read().unwrap();
            unified.get_balance_or_default(user, 0)
        };

        // Verify the proof
        assert!(
            state.verify_balance_proof(user, 0, &balance, &proof),
            "Valid proof should verify"
        );

        // Verify with wrong balance should fail
        let wrong_balance = hypercore_primitives::UnifiedBalance::default();
        assert!(
            !state.verify_balance_proof(user, 0, &wrong_balance, &proof),
            "Wrong balance should fail verification"
        );
    }

    #[test]
    fn test_nonce_proof_generation_and_verification() {
        let mut state = AppState::new();
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Increment nonces
        state.increment_nonce(user1);
        state.increment_nonce(user1);
        state.increment_nonce(user2);

        // Process block
        state.begin_block(1, 1000);
        state.end_block();

        // Generate and verify proofs
        let proof1 = state.prove_nonce(user1).unwrap();
        let proof2 = state.prove_nonce(user2).unwrap();

        assert!(
            state.verify_nonce_proof(user1, 2, &proof1),
            "User1 nonce proof should verify for nonce=2"
        );

        assert!(
            state.verify_nonce_proof(user2, 1, &proof2),
            "User2 nonce proof should verify for nonce=1"
        );

        // Wrong nonce should fail
        assert!(
            !state.verify_nonce_proof(user1, 1, &proof1),
            "Wrong nonce should fail verification"
        );
    }

    #[test]
    fn test_merkle_roots_change_with_state() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Process first block (empty state)
        state.begin_block(1, 1000);
        let hash1 = state.end_block();

        // Credit balance and process second block
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }
        state.begin_block(2, 2000);
        let hash2 = state.end_block();

        // App hashes should be different
        assert_ne!(hash1, hash2, "App hash should change when state changes");

        // Modify balance and process third block
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(500, 6));
        }
        state.begin_block(3, 3000);
        let hash3 = state.end_block();

        assert_ne!(hash2, hash3, "App hash should change when balance changes");
    }

    #[test]
    fn test_balance_and_nonce_counts() {
        let mut state = AppState::new();
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Credit balances
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
            unified.credit(user1, 1, Decimal::from_raw(500, 6));
        }

        // Add nonces
        state.increment_nonce(user1);
        state.increment_nonce(user2);

        // Process block
        state.begin_block(1, 1000);
        state.end_block();

        // Check counts
        assert_eq!(state.balance_count(), 3, "Should have 3 balance entries");
        assert_eq!(state.nonce_count(), 2, "Should have 2 nonce entries");
    }

    #[test]
    fn test_consistent_root_computation() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Credit balance
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }

        // Compute root multiple times - should be deterministic
        let root1 = state.compute_unified_state_root();
        let root2 = state.compute_unified_state_root();
        let root3 = state.compute_unified_state_root();

        assert_eq!(root1, root2);
        assert_eq!(root2, root3);
    }

    #[test]
    fn test_proof_still_verifies_after_new_block() {
        let mut state = AppState::new();
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Credit balance for user1
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
        }

        // Process block and get proof
        state.begin_block(1, 1000);
        state.end_block();

        let proof1 = state.prove_balance(user1, 0).unwrap();
        let root1 = state.get_unified_state_root();

        // Add user2's balance and process another block
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
        }
        state.begin_block(2, 2000);
        state.end_block();

        let root2 = state.get_unified_state_root();

        // Roots should be different
        assert_ne!(root1, root2);

        // Old proof should NOT verify against new root (state changed)
        assert!(!proof1.verify(&root2), "Old proof should not verify against new root");

        // But old proof should still verify against old root (if we had stored it)
        assert!(proof1.verify(&root1), "Old proof should verify against old root");
    }

    // ============================================================================
    // DETERMINISM TEST HARNESS
    //
    // These tests verify that state transitions are fully deterministic.
    // Identical inputs must produce identical outputs (AppHash) across:
    // - Multiple executions
    // - Different execution orderings (where order shouldn't matter)
    // - Various state components (balances, nonces, positions, orders, markets)
    //
    // This is CRITICAL for BFT consensus: all validators must arrive at the
    // same state given the same transactions.
    // ============================================================================

    /// Helper to create a fresh AppState for determinism testing
    fn create_test_state() -> AppState {
        AppState::new()
    }

    /// Helper to apply a standard set of operations and return the AppHash
    fn apply_standard_operations(state: &mut AppState, block_height: u64, block_timestamp: u64) -> [u8; 32] {
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);
        let user3 = AccountAddress::from([0x03u8; 20]);

        // Credit balances
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(10000_000000, 6)); // $10,000 USDC
            unified.credit(user2, 0, Decimal::from_raw(5000_000000, 6));  // $5,000 USDC
            unified.credit(user3, 0, Decimal::from_raw(2500_000000, 6));  // $2,500 USDC
        }

        // Set up nonces
        state.increment_nonce(user1);
        state.increment_nonce(user1);
        state.increment_nonce(user2);

        // Add CLOID mappings
        state.cloid_to_oid.insert((user1, "order-abc".to_string()), (0, 1001));
        state.cloid_to_oid.insert((user2, "order-xyz".to_string()), (0, 1002));

        // Update timestamp nonces
        state.last_timestamp_nonces.insert(user1, block_timestamp - 1000);

        // Process block
        state.begin_block(block_height, block_timestamp);
        state.end_block()
    }

    #[test]
    fn test_determinism_identical_executions() {
        // Execute the same operations twice and verify identical AppHash
        let mut state1 = create_test_state();
        let mut state2 = create_test_state();

        let hash1 = apply_standard_operations(&mut state1, 1, 1000000);
        let hash2 = apply_standard_operations(&mut state2, 1, 1000000);

        assert_eq!(
            hash1, hash2,
            "Identical operations must produce identical AppHash"
        );
    }

    #[test]
    fn test_determinism_multiple_runs() {
        // Run the same operations 10 times and verify all hashes match
        let mut hashes: Vec<[u8; 32]> = Vec::new();

        for _ in 0..10 {
            let mut state = create_test_state();
            let hash = apply_standard_operations(&mut state, 1, 1000000);
            hashes.push(hash);
        }

        let first_hash = hashes[0];
        for (i, hash) in hashes.iter().enumerate() {
            assert_eq!(
                *hash, first_hash,
                "Run {} produced different hash: expected {:?}, got {:?}",
                i, first_hash, hash
            );
        }
    }

    #[test]
    fn test_determinism_balance_order_independence() {
        // Test that the order of crediting balances doesn't affect the final hash
        // (since balances are stored in a HashMap but sorted before hashing)
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);
        let user3 = AccountAddress::from([0x03u8; 20]);

        // Order 1: user1, user2, user3
        let mut state1 = create_test_state();
        {
            let mut unified = state1.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
            unified.credit(user3, 0, Decimal::from_raw(3000, 6));
        }
        state1.begin_block(1, 1000);
        let hash1 = state1.end_block();

        // Order 2: user3, user1, user2
        let mut state2 = create_test_state();
        {
            let mut unified = state2.unified_state.write().unwrap();
            unified.credit(user3, 0, Decimal::from_raw(3000, 6));
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
        }
        state2.begin_block(1, 1000);
        let hash2 = state2.end_block();

        // Order 3: user2, user3, user1
        let mut state3 = create_test_state();
        {
            let mut unified = state3.unified_state.write().unwrap();
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
            unified.credit(user3, 0, Decimal::from_raw(3000, 6));
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
        }
        state3.begin_block(1, 1000);
        let hash3 = state3.end_block();

        assert_eq!(
            hash1, hash2,
            "Balance credit order should not affect AppHash"
        );
        assert_eq!(
            hash2, hash3,
            "Balance credit order should not affect AppHash"
        );
    }

    #[test]
    fn test_determinism_nonce_order_independence() {
        // Test that the order of incrementing nonces doesn't affect the final hash
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Order 1: user1, user1, user2
        let mut state1 = create_test_state();
        state1.increment_nonce(user1);
        state1.increment_nonce(user1);
        state1.increment_nonce(user2);
        state1.begin_block(1, 1000);
        let hash1 = state1.end_block();

        // Order 2: user2, user1, user1
        let mut state2 = create_test_state();
        state2.increment_nonce(user2);
        state2.increment_nonce(user1);
        state2.increment_nonce(user1);
        state2.begin_block(1, 1000);
        let hash2 = state2.end_block();

        assert_eq!(
            hash1, hash2,
            "Nonce increment order should not affect AppHash (same final nonces)"
        );
    }

    #[test]
    fn test_determinism_cloid_order_independence() {
        // Test that the order of CLOID insertions doesn't affect the final hash
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Order 1
        let mut state1 = create_test_state();
        state1.cloid_to_oid.insert((user1, "aaa".to_string()), (0, 1));
        state1.cloid_to_oid.insert((user2, "bbb".to_string()), (0, 2));
        state1.cloid_to_oid.insert((user1, "ccc".to_string()), (1, 3));
        state1.begin_block(1, 1000);
        let hash1 = state1.end_block();

        // Order 2 (reversed)
        let mut state2 = create_test_state();
        state2.cloid_to_oid.insert((user1, "ccc".to_string()), (1, 3));
        state2.cloid_to_oid.insert((user2, "bbb".to_string()), (0, 2));
        state2.cloid_to_oid.insert((user1, "aaa".to_string()), (0, 1));
        state2.begin_block(1, 1000);
        let hash2 = state2.end_block();

        assert_eq!(
            hash1, hash2,
            "CLOID insertion order should not affect AppHash"
        );
    }

    #[test]
    fn test_determinism_different_block_params_different_hash() {
        // Verify that different block parameters produce different hashes
        let mut state1 = create_test_state();
        let mut state2 = create_test_state();
        let mut state3 = create_test_state();

        // Same block height, different timestamp
        state1.begin_block(1, 1000);
        let hash1 = state1.end_block();

        state2.begin_block(1, 1001);
        let hash2 = state2.end_block();

        // Different block height, same timestamp
        state3.begin_block(2, 1000);
        let hash3 = state3.end_block();

        assert_ne!(
            hash1, hash2,
            "Different timestamps should produce different AppHash"
        );
        assert_ne!(
            hash1, hash3,
            "Different heights should produce different AppHash"
        );
    }

    #[test]
    fn test_determinism_sequential_blocks() {
        // Process multiple blocks and verify determinism
        let mut state1 = create_test_state();
        let mut state2 = create_test_state();

        let user = AccountAddress::from([0x42u8; 20]);

        // State 1: Process 3 blocks
        for i in 1..=3 {
            {
                let mut unified = state1.unified_state.write().unwrap();
                unified.credit(user, 0, Decimal::from_raw(1000 * i as i128, 6));
            }
            state1.increment_nonce(user);
            state1.begin_block(i, i * 1000);
            state1.end_block();
        }
        let final_hash1 = state1.app_hash;

        // State 2: Process same 3 blocks
        for i in 1..=3 {
            {
                let mut unified = state2.unified_state.write().unwrap();
                unified.credit(user, 0, Decimal::from_raw(1000 * i as i128, 6));
            }
            state2.increment_nonce(user);
            state2.begin_block(i, i * 1000);
            state2.end_block();
        }
        let final_hash2 = state2.app_hash;

        assert_eq!(
            final_hash1, final_hash2,
            "Sequential blocks must produce identical final AppHash"
        );
    }

    #[test]
    fn test_determinism_timestamp_nonce_validation() {
        // Test that timestamp-based nonce validation is deterministic
        let state = create_test_state();
        let user = AccountAddress::from([0x42u8; 20]);
        let ref_time: u64 = 1700000000000; // Fixed reference time

        // Valid timestamp nonces (within 1 hour)
        let valid_past = ref_time - 1800_000; // 30 minutes ago
        let valid_future = ref_time + 1800_000; // 30 minutes ahead

        // Invalid timestamp nonces (outside 1 hour)
        let invalid_past = ref_time - 4000_000; // > 1 hour ago
        let invalid_future = ref_time + 4000_000; // > 1 hour ahead

        // Run validation multiple times to verify determinism
        for _ in 0..10 {
            assert!(state.validate_nonce(&user, valid_past, ref_time));
            assert!(state.validate_nonce(&user, valid_future, ref_time));
            assert!(!state.validate_nonce(&user, invalid_past, ref_time));
            assert!(!state.validate_nonce(&user, invalid_future, ref_time));
        }
    }

    #[test]
    fn test_determinism_app_hash_chain() {
        // Test that the AppHash chain (each hash includes previous) is deterministic
        let mut state1 = create_test_state();
        let mut state2 = create_test_state();

        let mut hashes1 = Vec::new();
        let mut hashes2 = Vec::new();

        // Process 5 blocks on both states
        for i in 1..=5 {
            state1.begin_block(i, i * 1000);
            hashes1.push(state1.end_block());

            state2.begin_block(i, i * 1000);
            hashes2.push(state2.end_block());
        }

        // All hashes should match
        for i in 0..5 {
            assert_eq!(
                hashes1[i], hashes2[i],
                "Block {} AppHash mismatch",
                i + 1
            );
        }

        // Verify chain integrity - each hash should be different from previous
        for i in 1..5 {
            assert_ne!(
                hashes1[i], hashes1[i - 1],
                "Sequential blocks should have different hashes"
            );
        }
    }

    #[test]
    fn test_determinism_merkle_root_caching() {
        // Test that Merkle root caching doesn't break determinism
        let mut state = create_test_state();
        let user = AccountAddress::from([0x42u8; 20]);

        // Credit balance
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }

        // Process block (this caches Merkle trees)
        state.begin_block(1, 1000);
        let hash1 = state.end_block();

        // Compute roots again - should use cached values but produce same result
        let root1 = state.compute_unified_state_root();
        let root2 = state.compute_unified_state_root();

        assert_eq!(root1, root2, "Cached roots should be identical");

        // Process another identical block sequence on fresh state
        let mut state2 = create_test_state();
        {
            let mut unified = state2.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }
        state2.begin_block(1, 1000);
        let hash2 = state2.end_block();

        assert_eq!(hash1, hash2, "Caching should not affect determinism");
    }

    #[test]
    fn test_determinism_empty_state() {
        // Test that empty state produces consistent hash
        let mut state1 = create_test_state();
        let mut state2 = create_test_state();

        state1.begin_block(1, 1000);
        let hash1 = state1.end_block();

        state2.begin_block(1, 1000);
        let hash2 = state2.end_block();

        assert_eq!(hash1, hash2, "Empty state should produce identical hash");
    }

    #[test]
    fn test_determinism_all_merkle_roots() {
        // Verify that all individual Merkle root computations are deterministic
        let mut state = create_test_state();
        let user = AccountAddress::from([0x42u8; 20]);

        // Set up some state
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }
        state.increment_nonce(user);
        state.cloid_to_oid.insert((user, "test".to_string()), (0, 1));

        // Compute each root multiple times
        let unified_roots: Vec<_> = (0..5).map(|_| state.compute_unified_state_root()).collect();
        let nonce_roots: Vec<_> = (0..5).map(|_| state.compute_nonce_root()).collect();
        let cloid_roots: Vec<_> = (0..5).map(|_| state.compute_cloid_root()).collect();
        let positions_roots: Vec<_> = (0..5).map(|_| state.compute_positions_root()).collect();
        let orders_roots: Vec<_> = (0..5).map(|_| state.compute_orders_root()).collect();
        let markets_roots: Vec<_> = (0..5).map(|_| state.compute_markets_root()).collect();
        let leverage_roots: Vec<_> = (0..5).map(|_| state.compute_leverage_root()).collect();
        let scalars_hashes: Vec<_> = (0..5).map(|_| state.compute_engine_scalars_hash()).collect();

        // All repeated computations should be identical
        for i in 1..5 {
            assert_eq!(unified_roots[0], unified_roots[i], "unified_state_root not deterministic");
            assert_eq!(nonce_roots[0], nonce_roots[i], "nonce_root not deterministic");
            assert_eq!(cloid_roots[0], cloid_roots[i], "cloid_root not deterministic");
            assert_eq!(positions_roots[0], positions_roots[i], "positions_root not deterministic");
            assert_eq!(orders_roots[0], orders_roots[i], "orders_root not deterministic");
            assert_eq!(markets_roots[0], markets_roots[i], "markets_root not deterministic");
            assert_eq!(leverage_roots[0], leverage_roots[i], "leverage_root not deterministic");
            assert_eq!(scalars_hashes[0], scalars_hashes[i], "engine_scalars_hash not deterministic");
        }
    }

    // ========================================================================
    // Re-org Snapshot/Restore Tests (Phase 7C)
    // ========================================================================

    #[test]
    fn test_unified_state_snapshot_restore() {
        let mut state = AppState::new();
        let user1 = AccountAddress::from([0x01u8; 20]);
        let user2 = AccountAddress::from([0x02u8; 20]);

        // Credit initial balances
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(1000, 6));
            unified.credit(user2, 0, Decimal::from_raw(2000, 6));
        }

        // Take snapshot
        let snapshot = state.snapshot_blocking();

        // Modify state after snapshot
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user1, 0, Decimal::from_raw(5000, 6)); // user1 now has 6000
            unified.credit(user2, 1, Decimal::from_raw(500, 6));  // user2 gets new token
        }

        // Verify state was modified
        let balance_before_restore = state.get_core_balance(user1, 0);
        assert_eq!(balance_before_restore.raw(), 6000, "Balance should be 6000 after modification");

        // Restore from snapshot
        state.restore_blocking(snapshot);

        // Verify state was reverted
        let balance1 = state.get_core_balance(user1, 0);
        let balance2 = state.get_core_balance(user2, 0);
        let balance2_token1 = state.get_core_balance(user2, 1);

        assert_eq!(balance1.raw(), 1000, "User1 balance should be restored to 1000");
        assert_eq!(balance2.raw(), 2000, "User2 balance should be restored to 2000");
        assert!(balance2_token1.is_zero(), "User2 token1 should not exist after restore");
    }

    #[test]
    fn test_app_state_snapshot_restores_height_and_nonces() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Process block 1
        state.begin_block(1, 1000);
        state.increment_nonce(user);
        state.end_block();

        // Take snapshot at block 1
        let snapshot = state.snapshot_blocking();
        assert_eq!(snapshot.height, 1);
        assert_eq!(snapshot.nonces.get(&user), Some(&1));

        // Process block 2
        state.begin_block(2, 2000);
        state.increment_nonce(user);
        state.end_block();

        // Verify state at block 2
        assert_eq!(state.height, 2);
        assert_eq!(state.get_nonce(&user), 2);

        // Restore to block 1
        state.restore_blocking(snapshot);

        // Verify restoration
        assert_eq!(state.height, 1);
        assert_eq!(state.get_nonce(&user), 1);
    }

    #[test]
    fn test_snapshot_captures_engine_state() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Deposit to engine state
        {
            let mut engine = state.engine.blocking_write();
            engine.deposit(user, 10000_000000); // $10,000
        }

        // Take snapshot
        let snapshot = state.snapshot_blocking();

        // Modify engine state after snapshot
        {
            let mut engine = state.engine.blocking_write();
            engine.deposit(user, 5000_000000); // Add another $5,000
        }

        // Verify modification
        {
            let engine = state.engine.blocking_read();
            let account = engine.get_account(user).unwrap();
            assert_eq!(account.balance, 15000_000000);
        }

        // Restore from snapshot
        state.restore_blocking(snapshot);

        // Verify engine state was restored
        {
            let engine = state.engine.blocking_read();
            let account = engine.get_account(user).unwrap();
            assert_eq!(account.balance, 10000_000000, "Engine balance should be restored");
        }
    }

    #[test]
    fn test_snapshot_after_multiple_blocks() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Process several blocks
        for i in 1..=5 {
            {
                let mut unified = state.unified_state.write().unwrap();
                unified.credit(user, 0, Decimal::from_raw(100, 6)); // Add 100 each block
            }
            state.begin_block(i, i * 1000);
            state.increment_nonce(user);
            state.end_block();
        }

        // Take snapshot at block 5
        let snapshot_at_5 = state.snapshot_blocking();
        assert_eq!(snapshot_at_5.height, 5);

        // Process more blocks
        for i in 6..=8 {
            {
                let mut unified = state.unified_state.write().unwrap();
                unified.credit(user, 0, Decimal::from_raw(1000, 6)); // Add 1000 each block
            }
            state.begin_block(i, i * 1000);
            state.increment_nonce(user);
            state.end_block();
        }

        // Verify state at block 8
        assert_eq!(state.height, 8);
        let balance_at_8 = state.get_core_balance(user, 0);
        assert_eq!(balance_at_8.raw(), 3500, "Balance at block 8: 5*100 + 3*1000 = 3500");

        // Restore to block 5
        state.restore_blocking(snapshot_at_5);

        // Verify restoration
        assert_eq!(state.height, 5);
        let balance_restored = state.get_core_balance(user, 0);
        assert_eq!(balance_restored.raw(), 500, "Balance should be 5*100 = 500");
        assert_eq!(state.get_nonce(&user), 5);
    }

    #[test]
    fn test_snapshot_restore_clears_pending_state() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Process block 1
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }
        state.begin_block(1, 1000);
        state.end_block();

        // Take snapshot
        let snapshot = state.snapshot_blocking();

        // Start processing block 2 (but don't complete it)
        state.begin_block(2, 2000);
        // Add some pending events (normally done during tx execution)
        state.pending_block_events.push(serde_json::json!({"type": "test"}));

        // Verify pending state exists
        assert!(!state.pending_block_events.is_empty());
        assert_eq!(state.height, 2);

        // Restore from snapshot
        state.restore_blocking(snapshot);

        // Pending state should be cleared
        assert!(state.pending_block_events.is_empty(), "Pending events should be cleared");
        assert!(state.pending_txs.is_empty(), "Pending txs should be cleared");
        assert_eq!(state.height, 1, "Height should be restored to 1");
    }

    #[test]
    fn test_snapshot_determinism() {
        // Two identical state sequences should produce identical snapshots
        let setup_state = |state: &mut AppState| {
            let user1 = AccountAddress::from([0x01u8; 20]);
            let user2 = AccountAddress::from([0x02u8; 20]);

            {
                let mut unified = state.unified_state.write().unwrap();
                unified.credit(user1, 0, Decimal::from_raw(1000, 6));
                unified.credit(user2, 0, Decimal::from_raw(2000, 6));
            }

            state.increment_nonce(user1);
            state.begin_block(1, 1000);
            state.end_block();
        };

        let mut state1 = AppState::new();
        let mut state2 = AppState::new();

        setup_state(&mut state1);
        setup_state(&mut state2);

        let snapshot1 = state1.snapshot_blocking();
        let snapshot2 = state2.snapshot_blocking();

        // Core properties should match
        assert_eq!(snapshot1.height, snapshot2.height);
        assert_eq!(snapshot1.timestamp, snapshot2.timestamp);
        assert_eq!(snapshot1.app_hash, snapshot2.app_hash);
        assert_eq!(snapshot1.nonces, snapshot2.nonces);
        assert_eq!(snapshot1.next_trade_id, snapshot2.next_trade_id);
    }

    #[test]
    fn test_restore_preserves_app_hash_determinism() {
        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Process block 1
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }
        state.begin_block(1, 1000);
        let hash1 = state.end_block();

        // Take snapshot
        let snapshot = state.snapshot_blocking();

        // Process block 2
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(500, 6));
        }
        state.begin_block(2, 2000);
        let hash2 = state.end_block();

        // Verify hashes are different
        assert_ne!(hash1, hash2, "Block 1 and Block 2 should have different hashes");

        // Restore to block 1
        state.restore_blocking(snapshot);

        // The app_hash from snapshot should match original
        assert_eq!(state.app_hash, hash1, "Restored app_hash should match original");

        // After restore, we should be at height 1
        assert_eq!(state.height, 1);

        // Now if we process block 2 again with THE SAME transactions/state changes,
        // we should get the SAME hash as before
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(500, 6)); // Same change as before
        }
        state.begin_block(2, 2000); // Same height and timestamp
        let hash2_replay = state.end_block();

        // Determinism: same state changes should produce same hash
        assert_eq!(hash2, hash2_replay, "Replayed block 2 should produce same hash");
    }

    #[test]
    fn test_reorg_scenario_basic() {
        // Simulate a basic re-org scenario:
        // 1. Process blocks A, B, C
        // 2. Revert to A
        // 3. Process blocks B', C' (different chain)

        let mut state = AppState::new();
        let user = AccountAddress::from([0x42u8; 20]);

        // Initial deposit
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(1000, 6));
        }

        // Block A (height 1)
        state.begin_block(1, 1000);
        state.increment_nonce(user);
        let hash_a = state.end_block();

        // SNAPSHOT: Take checkpoint at block A
        let checkpoint_a = state.snapshot_blocking();

        // Block B (height 2) - Original chain
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(100, 6)); // +100 on original chain
        }
        state.begin_block(2, 2000);
        state.increment_nonce(user);
        let _hash_b = state.end_block();

        // Block C (height 3) - Original chain
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(100, 6)); // +100 on original chain
        }
        state.begin_block(3, 3000);
        state.increment_nonce(user);
        let _hash_c = state.end_block();

        // At this point: balance = 1000 + 100 + 100 = 1200, nonce = 3
        assert_eq!(state.get_core_balance(user, 0).raw(), 1200);
        assert_eq!(state.get_nonce(&user), 3);

        // RE-ORG: Revert to block A
        state.restore_blocking(checkpoint_a);

        // Verify we're back at block A
        assert_eq!(state.height, 1);
        assert_eq!(state.app_hash, hash_a);
        assert_eq!(state.get_core_balance(user, 0).raw(), 1000);
        assert_eq!(state.get_nonce(&user), 1);

        // Block B' (height 2) - New chain with different transactions
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(500, 6)); // +500 on new chain (different!)
        }
        state.begin_block(2, 2000);
        state.increment_nonce(user);
        let hash_b_prime = state.end_block();

        // Block C' (height 3) - New chain
        {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(user, 0, Decimal::from_raw(500, 6)); // +500 on new chain
        }
        state.begin_block(3, 3000);
        state.increment_nonce(user);
        let _hash_c_prime = state.end_block();

        // Final state on new chain: balance = 1000 + 500 + 500 = 2000
        assert_eq!(state.get_core_balance(user, 0).raw(), 2000);
        assert_eq!(state.get_nonce(&user), 3);

        // Hashes should be different because transactions were different
        assert_ne!(_hash_b, hash_b_prime, "Different transactions should produce different hashes");
    }

    #[test]
    fn test_snapshot_multiple_users() {
        let mut state = AppState::new();
        let users: Vec<AccountAddress> = (0..10)
            .map(|i| AccountAddress::from([i as u8; 20]))
            .collect();

        // Credit all users
        for (i, user) in users.iter().enumerate() {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(*user, 0, Decimal::from_raw((i as i128 + 1) * 1000, 6));
        }

        // Process block
        state.begin_block(1, 1000);
        for user in &users {
            state.increment_nonce(*user);
        }
        state.end_block();

        // Take snapshot
        let snapshot = state.snapshot_blocking();

        // Modify all users
        for (i, user) in users.iter().enumerate() {
            let mut unified = state.unified_state.write().unwrap();
            unified.credit(*user, 0, Decimal::from_raw(9999, 6));
        }

        // Restore
        state.restore_blocking(snapshot);

        // Verify all users were restored correctly
        for (i, user) in users.iter().enumerate() {
            let balance = state.get_core_balance(*user, 0);
            assert_eq!(
                balance.raw(),
                (i as i128 + 1) * 1000,
                "User {} balance should be restored",
                i
            );
            assert_eq!(state.get_nonce(user), 1, "User {} nonce should be 1", i);
        }
    }
}
