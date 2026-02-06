//! State Persister - Handles state extraction, serialization, and persistence
//!
//! This module provides the `StatePersister` which is responsible for:
//! - Extracting state from various engines
//! - Converting to serializable `PersistedState`
//! - Saving to and loading from RocksDB
//! - Validating state invariants

use crate::{
    column_families::ColumnFamily,
    error::{PersistenceError, Result},
    keys::KeyEncoder,
    state::{
        AccountEntry, BalanceEntry, BlockMetaEntry, ChainState, CloidEntry,
        CoreState, EvmAccountEntry, EvmBlockHashEntry, EvmCodeEntry,
        EvmStateData, EvmStorageEntry, LeverageEntry, MarketEntry, NonceEntry,
        OrderEntry, PersistedState, PositionEntry, ReservedEntry, SpotMarketEntry,
        SpotState, SpotTokenEntry, UnifiedBalanceData, SCHEMA_VERSION,
    },
    PersistenceBackend, WriteBatch,
};
use hypercore_primitives::{
    AccountAddress, Decimal, MarketId, Order, OrderId, Position, TokenIndex,
};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

/// State persister for saving and loading blockchain state
pub struct StatePersister<'a, B: PersistenceBackend> {
    backend: &'a B,
}

impl<'a, B: PersistenceBackend> StatePersister<'a, B> {
    /// Create a new state persister with the given backend
    pub fn new(backend: &'a B) -> Self {
        Self { backend }
    }

    /// Get the current persisted block height
    pub fn get_height(&self) -> Result<u64> {
        self.backend.get_height()
    }

    /// Check if we have persisted state
    pub fn has_state(&self) -> Result<bool> {
        Ok(self.backend.get_height()? > 0)
    }

    /// Get the underlying backend
    pub fn backend(&self) -> &B {
        self.backend
    }

    /// Persist the complete application state at a given height
    ///
    /// This method extracts data from all state components and atomically
    /// writes them to RocksDB.
    ///
    /// IMPORTANT: For column families that represent "full state snapshots" (CLOIDs, Orders,
    /// Positions), we first delete all existing entries before writing new ones. This ensures
    /// that removed/filled/closed items don't persist as stale data.
    pub fn persist_state(&self, state: &PersistedState) -> Result<()> {
        info!("Persisting state at height {}", state.height);

        // Validate state before persisting
        state.validate().map_err(PersistenceError::invalid_state)?;

        let mut batch = self.backend.create_batch();

        // === Clear stale entries from column families that are fully replaced ===
        // CLOIDs: When orders are cancelled/filled, CLOIDs are removed from memory but
        // were never deleted from RocksDB. This caused stale CLOIDs to accumulate.
        let existing_cloids = self.backend.prefix_scan(ColumnFamily::CloidIndex, &[])?;
        for (key, _) in existing_cloids {
            batch.delete(ColumnFamily::CloidIndex, key);
        }

        // Orders: Same issue - cancelled/filled orders need to be removed
        let existing_orders = self.backend.prefix_scan(ColumnFamily::Orders, &[])?;
        for (key, _) in existing_orders {
            batch.delete(ColumnFamily::Orders, key);
        }

        // Positions: Closed positions need to be removed
        let existing_positions = self.backend.prefix_scan(ColumnFamily::Positions, &[])?;
        for (key, _) in existing_positions {
            batch.delete(ColumnFamily::Positions, key);
        }

        // Leverage: Remove stale leverage entries for closed positions
        let existing_leverage = self.backend.prefix_scan(ColumnFamily::Leverage, &[])?;
        for (key, _) in existing_leverage {
            batch.delete(ColumnFamily::Leverage, key);
        }

        // Balances: While balances are keyed by (account, token), if an account is removed
        // or a token balance becomes zero, the entry should be deleted to prevent stale data.
        // Clear all and re-write the current balances.
        let existing_balances = self.backend.prefix_scan(ColumnFamily::Balances, &[])?;
        for (key, _) in existing_balances {
            batch.delete(ColumnFamily::Balances, key);
        }

        // Nonces: Same issue - accounts might have been removed or nonces reset
        let existing_nonces = self.backend.prefix_scan(ColumnFamily::Nonces, &[])?;
        for (key, _) in existing_nonces {
            batch.delete(ColumnFamily::Nonces, key);
        }

        // Persist unified balances
        for balance in &state.core.balances {
            let key = KeyEncoder::balance_key(&balance.account, balance.token);
            let value = bincode::serialize(&balance.balance)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Balances, key, value);
        }

        // Persist perpetual accounts
        for account in &state.core.accounts {
            let key = KeyEncoder::nonce_key(&account.account);
            let value = bincode::serialize(account)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Accounts, key, value);
        }

        // Persist positions (prefer perp field, fall back to core)
        // In CometBFT mode, extract_state() writes positions to perp.positions (canonical).
        // The core.positions field is empty in CometBFT mode (shared EngineState has no positions).
        let positions = if !state.perp.positions.is_empty() { &state.perp.positions } else { &state.core.positions };
        for position in positions {
            let key = KeyEncoder::position_key(&position.account, position.market);
            let value = bincode::serialize(&position.position)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Positions, key, value);
        }

        // Persist leverage settings (prefer perp field, fall back to core)
        let leverage = if !state.perp.leverage.is_empty() { &state.perp.leverage } else { &state.core.leverage };
        for lev in leverage {
            let key = KeyEncoder::position_key(&lev.account, lev.market);
            batch.put(ColumnFamily::Leverage, key, vec![lev.leverage]);
        }

        // Persist markets (prefer perp field, fall back to core)
        // In CometBFT mode, perp.markets has current mark_price, funding state, fee rates.
        let markets = if !state.perp.markets.is_empty() { &state.perp.markets } else { &state.core.markets };
        for market in markets {
            let key = KeyEncoder::market_key(market.id);
            let value = serde_json::to_vec(market)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Markets, key, value);
        }

        // Persist orders (prefer perp field, fall back to core)
        let orders = if !state.perp.orders.is_empty() { &state.perp.orders } else { &state.core.orders };
        for order in orders {
            let key = KeyEncoder::order_key(order.market, order.order_id);
            let value = bincode::serialize(&order.order)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Orders, key, value);
        }

        // Persist spot tokens
        for token in &state.spot.tokens {
            let key = KeyEncoder::token_key(token.index);
            let value = serde_json::to_vec(token)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::SpotTokens, key, value);
        }

        // Persist spot markets
        for market in &state.spot.markets {
            let key = vec![market.id as u8]; // SpotMarketId is u32, use single byte for small IDs
            let value = serde_json::to_vec(market)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::SpotMarkets, key, value);
        }

        // Persist reserved balances
        for reserved in &state.spot.reserved {
            let key = KeyEncoder::balance_key(&reserved.account, reserved.token);
            let value = reserved.amount.as_bytes().to_vec();
            batch.put(ColumnFamily::SpotReserved, key, value);
        }

        // Persist spot orders
        for order in &state.spot.orders {
            // Use 128+ offset for spot market IDs
            let key = KeyEncoder::order_key(order.market.saturating_add(128), order.order_id);
            let value = bincode::serialize(&order.order)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Orders, key, value);
        }

        // Persist EVM accounts
        for account in &state.evm.accounts {
            let key = account.address.to_vec();
            let value = bincode::serialize(account)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::EvmAccounts, key, value);
        }

        // Persist EVM storage
        for storage in &state.evm.storage {
            let key = KeyEncoder::evm_storage_key(&storage.address, &storage.slot);
            batch.put(ColumnFamily::EvmStorage, key, storage.value.to_vec());
        }

        // Persist EVM code
        for code in &state.evm.code {
            batch.put(ColumnFamily::EvmCode, code.code_hash.to_vec(), code.bytecode.clone());
        }

        // Persist EVM block hashes
        for block_hash in &state.evm.block_hashes {
            let key = KeyEncoder::block_key(block_hash.height);
            batch.put(ColumnFamily::EvmBlockHashes, key, block_hash.hash.to_vec());
        }

        // Persist nonces
        for nonce in &state.chain.nonces {
            let key = KeyEncoder::nonce_key(&nonce.account);
            batch.put(ColumnFamily::Nonces, key, nonce.nonce.to_le_bytes().to_vec());
        }

        // Persist block metadata
        for block in &state.chain.blocks {
            let key = KeyEncoder::block_key(block.height);
            let value = serde_json::to_vec(block)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::BlockMeta, key, value);
        }

        // Persist CLOID index
        for cloid in &state.chain.cloid_index {
            let key = KeyEncoder::cloid_key(&cloid.account, &cloid.cloid);
            let mut value = Vec::with_capacity(9);
            value.push(cloid.market);
            value.extend_from_slice(&cloid.order_id.to_le_bytes());
            batch.put(ColumnFamily::CloidIndex, key, value);
        }

        // Persist metadata
        batch.put(
            ColumnFamily::Metadata,
            KeyEncoder::METADATA_HEIGHT.to_vec(),
            state.height.to_le_bytes().to_vec(),
        );
        batch.put(
            ColumnFamily::Metadata,
            KeyEncoder::METADATA_TIMESTAMP.to_vec(),
            state.timestamp.to_le_bytes().to_vec(),
        );
        batch.put(
            ColumnFamily::Metadata,
            KeyEncoder::METADATA_NEXT_ORDER_ID.to_vec(),
            state.core.next_order_id.to_le_bytes().to_vec(),
        );
        batch.put(
            ColumnFamily::Metadata,
            KeyEncoder::METADATA_INSURANCE_FUND.to_vec(),
            state.core.insurance_fund.as_bytes().to_vec(),
        );
        batch.put(
            ColumnFamily::Metadata,
            KeyEncoder::METADATA_SCHEMA_VERSION.to_vec(),
            SCHEMA_VERSION.to_le_bytes().to_vec(),
        );

        // Store app hash
        let key = KeyEncoder::block_key(state.height);
        batch.put(ColumnFamily::AppHashes, key, state.app_hash.to_vec());

        // Commit batch atomically
        self.backend.commit_batch(batch)?;

        info!(
            "Persisted state at height {} with {} balances, {} orders",
            state.height,
            state.core.balances.len(),
            state.core.orders.len() + state.spot.orders.len()
        );

        Ok(())
    }

    /// Load the complete application state from persistence
    ///
    /// Returns None if no state has been persisted yet.
    pub fn load_state(&self) -> Result<Option<PersistedState>> {
        let height = self.backend.get_height()?;
        if height == 0 {
            info!("No persisted state found");
            return Ok(None);
        }

        info!("Loading state at height {}", height);

        let mut state = PersistedState::new();
        state.height = height;

        // Load timestamp
        if let Some(bytes) = self.backend.get(ColumnFamily::Metadata, KeyEncoder::METADATA_TIMESTAMP)? {
            if bytes.len() >= 8 {
                state.timestamp = u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
            }
        }

        // Load app hash
        if let Some(hash) = self.backend.get_app_hash(height)? {
            state.app_hash = hash;
        }

        // Load unified balances
        let balances = self.backend.prefix_scan(ColumnFamily::Balances, &[])?;
        for (key, value) in balances {
            if let Some((account, token)) = KeyEncoder::decode_balance_key(&key) {
                if let Ok(balance) = bincode::deserialize::<UnifiedBalanceData>(&value) {
                    state.core.balances.push(BalanceEntry {
                        account,
                        token,
                        balance,
                    });
                }
            }
        }

        // Load positions
        let positions = self.backend.prefix_scan(ColumnFamily::Positions, &[])?;
        for (key, value) in positions {
            if let Some((account, market)) = KeyEncoder::decode_position_key(&key) {
                if let Ok(position) = bincode::deserialize::<Position>(&value) {
                    state.core.positions.push(PositionEntry {
                        account,
                        market,
                        position,
                    });
                }
            }
        }

        // Load leverage settings
        let leverage = self.backend.prefix_scan(ColumnFamily::Leverage, &[])?;
        for (key, value) in leverage {
            if let Some((account, market)) = KeyEncoder::decode_position_key(&key) {
                if !value.is_empty() {
                    state.core.leverage.push(LeverageEntry {
                        account,
                        market,
                        leverage: value[0],
                    });
                }
            }
        }

        // Load markets
        let markets = self.backend.prefix_scan(ColumnFamily::Markets, &[])?;
        for (key, value) in markets {
            if let Ok(market) = serde_json::from_slice::<MarketEntry>(&value) {
                state.core.markets.push(market);
            }
        }

        // Load perp orders
        let orders = self.backend.prefix_scan(ColumnFamily::Orders, &[])?;
        for (key, value) in orders {
            if let Some((market, order_id)) = KeyEncoder::decode_order_key(&key) {
                if let Ok(order) = bincode::deserialize::<Order>(&value) {
                    // Spot orders have market ID >= 128
                    if market < 128 {
                        state.core.orders.push(OrderEntry {
                            market,
                            order_id,
                            order,
                        });
                    } else {
                        state.spot.orders.push(OrderEntry {
                            market: market.saturating_sub(128),
                            order_id,
                            order,
                        });
                    }
                }
            }
        }

        // Load next_order_id
        if let Some(bytes) = self.backend.get(ColumnFamily::Metadata, KeyEncoder::METADATA_NEXT_ORDER_ID)? {
            if bytes.len() >= 8 {
                state.core.next_order_id = u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
            }
        }

        // Load insurance fund
        if let Some(bytes) = self.backend.get(ColumnFamily::Metadata, KeyEncoder::METADATA_INSURANCE_FUND)? {
            state.core.insurance_fund = String::from_utf8(bytes).unwrap_or_else(|_| "0".to_string());
        }

        // Load spot tokens
        let tokens = self.backend.prefix_scan(ColumnFamily::SpotTokens, &[])?;
        for (_key, value) in tokens {
            if let Ok(token) = serde_json::from_slice::<SpotTokenEntry>(&value) {
                state.spot.tokens.push(token);
            }
        }

        // Load spot markets
        let spot_markets = self.backend.prefix_scan(ColumnFamily::SpotMarkets, &[])?;
        for (_key, value) in spot_markets {
            if let Ok(market) = serde_json::from_slice::<SpotMarketEntry>(&value) {
                state.spot.markets.push(market);
            }
        }

        // Load EVM accounts
        let evm_accounts = self.backend.prefix_scan(ColumnFamily::EvmAccounts, &[])?;
        for (_key, value) in evm_accounts {
            if let Ok(account) = bincode::deserialize::<EvmAccountEntry>(&value) {
                state.evm.accounts.push(account);
            }
        }

        // Load EVM storage
        let storage = self.backend.prefix_scan(ColumnFamily::EvmStorage, &[])?;
        for (key, value) in storage {
            if let Some((address, slot)) = KeyEncoder::decode_evm_storage_key(&key) {
                let mut val = [0u8; 32];
                val.copy_from_slice(&value[..32.min(value.len())]);
                state.evm.storage.push(EvmStorageEntry {
                    address,
                    slot,
                    value: val,
                });
            }
        }

        // Load EVM code
        let code = self.backend.prefix_scan(ColumnFamily::EvmCode, &[])?;
        for (key, value) in code {
            if key.len() == 32 {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&key);
                state.evm.code.push(EvmCodeEntry {
                    code_hash: hash,
                    bytecode: value,
                });
            }
        }

        // Load nonces
        let nonces = self.backend.prefix_scan(ColumnFamily::Nonces, &[])?;
        for (key, value) in nonces {
            if key.len() == 20 && value.len() >= 8 {
                let mut addr = [0u8; 20];
                addr.copy_from_slice(&key);
                let nonce = u64::from_le_bytes([
                    value[0], value[1], value[2], value[3],
                    value[4], value[5], value[6], value[7],
                ]);
                state.chain.nonces.push(NonceEntry {
                    account: AccountAddress::from(addr),
                    nonce,
                });
            }
        }

        // Load block metadata
        let blocks = self.backend.prefix_scan(ColumnFamily::BlockMeta, &[])?;
        for (_key, value) in blocks {
            if let Ok(block) = serde_json::from_slice::<BlockMetaEntry>(&value) {
                state.chain.blocks.push(block);
            }
        }

        // Load CLOID index
        let cloids = self.backend.prefix_scan(ColumnFamily::CloidIndex, &[])?;
        for (key, value) in cloids {
            if key.len() > 20 && value.len() >= 9 {
                let mut addr = [0u8; 20];
                addr.copy_from_slice(&key[..20]);
                let cloid = String::from_utf8_lossy(&key[20..]).to_string();
                let market = value[0];
                let order_id = u64::from_le_bytes([
                    value[1], value[2], value[3], value[4],
                    value[5], value[6], value[7], value[8],
                ]);
                state.chain.cloid_index.push(CloidEntry {
                    account: AccountAddress::from(addr),
                    cloid,
                    market,
                    order_id,
                });
            }
        }

        // Validate loaded state
        if let Err(e) = state.validate() {
            warn!("Loaded state failed validation: {}", e);
            return Err(PersistenceError::invalid_state(e));
        }

        info!(
            "Loaded state: {} balances, {} positions, {} orders",
            state.core.balances.len(),
            state.core.positions.len(),
            state.core.orders.len() + state.spot.orders.len()
        );

        Ok(Some(state))
    }

    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<()> {
        self.backend.flush()
    }

    /// Close the persistence backend
    pub fn close(&self) -> Result<()> {
        self.backend.close()
    }
}

/// Validate that a state passes all invariants
pub fn validate_state(state: &PersistedState) -> Result<()> {
    // Check schema version
    if state.schema_version != SCHEMA_VERSION {
        return Err(PersistenceError::VersionMismatch {
            expected: SCHEMA_VERSION,
            found: state.schema_version,
        });
    }

    // Check balance invariants
    for balance in &state.core.balances {
        if !balance.balance.is_valid() {
            return Err(PersistenceError::invariant(format!(
                "Balance invariant violated for {:?} token {}: total != core_view + evm_view",
                balance.account, balance.token
            )));
        }
    }

    // Check order IDs are unique
    let mut seen_orders = std::collections::HashSet::new();
    for order in &state.core.orders {
        let key = (order.market, order.order_id);
        if !seen_orders.insert(key) {
            return Err(PersistenceError::invariant(format!(
                "Duplicate order ID: market={}, order_id={}",
                order.market, order.order_id
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PersistenceConfig, RocksDbBackend};
    use tempfile::TempDir;

    fn create_test_backend() -> (RocksDbBackend, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            create_if_missing: true,
            ..Default::default()
        };
        let backend = RocksDbBackend::open(&config).unwrap();
        (backend, temp_dir)
    }

    #[test]
    fn test_persist_and_load_empty_state() {
        let (backend, _temp_dir) = create_test_backend();
        let persister = StatePersister::new(&backend);

        // Initially no state
        assert!(!persister.has_state().unwrap());

        // Create minimal state
        let mut state = PersistedState::new();
        state.height = 1;
        state.timestamp = 1000;

        persister.persist_state(&state).unwrap();

        // Now has state
        assert!(persister.has_state().unwrap());
        assert_eq!(persister.get_height().unwrap(), 1);

        // Load state
        let loaded = persister.load_state().unwrap().unwrap();
        assert_eq!(loaded.height, 1);
        assert_eq!(loaded.timestamp, 1000);
    }

    #[test]
    fn test_persist_and_load_balances() {
        let (backend, _temp_dir) = create_test_backend();
        let persister = StatePersister::new(&backend);

        let account = AccountAddress::from([1u8; 20]);
        let mut state = PersistedState::new();
        state.height = 1;
        state.core.balances.push(BalanceEntry {
            account,
            token: 0,
            balance: UnifiedBalanceData {
                total: "100".to_string(),
                core_view: "60".to_string(),
                evm_view: "40".to_string(),
                decimals: 6,
            },
        });

        persister.persist_state(&state).unwrap();

        let loaded = persister.load_state().unwrap().unwrap();
        assert_eq!(loaded.core.balances.len(), 1);
        assert_eq!(loaded.core.balances[0].account, account);
        assert_eq!(loaded.core.balances[0].balance.total, "100");
    }

    #[test]
    fn test_validation_fails_for_invalid_balance() {
        let mut state = PersistedState::new();
        state.height = 1;
        state.core.balances.push(BalanceEntry {
            account: AccountAddress::from([1u8; 20]),
            token: 0,
            balance: UnifiedBalanceData {
                total: "100".to_string(),
                core_view: "60".to_string(),
                evm_view: "50".to_string(), // 60 + 50 != 100
                decimals: 6,
            },
        });

        let result = validate_state(&state);
        assert!(result.is_err());
    }

    // ============ Export/Import Tests ============

    /// Create a comprehensive test state with all data types
    fn create_comprehensive_test_state() -> PersistedState {
        use hypercore_primitives::{Order, OrderSide, OrderStatus, OrderType, Position, TimeInForce};

        let mut state = PersistedState::new();
        state.height = 12345;
        state.timestamp = 1706054400000; // 2024-01-24 00:00:00 UTC
        state.app_hash = [0xab; 32];

        // Add multiple balances for different accounts and tokens
        let account1 = AccountAddress::from([1u8; 20]);
        let account2 = AccountAddress::from([2u8; 20]);

        state.core.balances.push(BalanceEntry {
            account: account1,
            token: 0, // USDC
            balance: UnifiedBalanceData {
                total: "100000000000".to_string(), // 100,000 USDC
                core_view: "80000000000".to_string(),
                evm_view: "20000000000".to_string(),
                decimals: 6,
            },
        });
        state.core.balances.push(BalanceEntry {
            account: account1,
            token: 1, // TEST
            balance: UnifiedBalanceData {
                total: "5000".to_string(), // 5,000 TEST (human-readable, parsed with 18 decimals)
                core_view: "5000".to_string(),
                evm_view: "0".to_string(),
                decimals: 18,
            },
        });
        state.core.balances.push(BalanceEntry {
            account: account2,
            token: 0,
            balance: UnifiedBalanceData {
                total: "50000000000".to_string(),
                core_view: "50000000000".to_string(),
                evm_view: "0".to_string(),
                decimals: 6,
            },
        });

        // Add positions (using apply_fill to create them)
        let mut pos1 = Position::new();
        pos1.apply_fill(
            hypercore_primitives::Decimal::size("0.5"),
            hypercore_primitives::Decimal::price("65000"),
            true, // is_buy (long)
        );
        state.core.positions.push(PositionEntry {
            account: account1,
            market: 0, // BTC-PERP
            position: pos1,
        });

        let mut pos2 = Position::new();
        pos2.apply_fill(
            hypercore_primitives::Decimal::size("2.0"),
            hypercore_primitives::Decimal::price("3500"),
            false, // is_buy (short = sell)
        );
        state.core.positions.push(PositionEntry {
            account: account2,
            market: 1, // ETH-PERP
            position: pos2,
        });

        // Add leverage settings
        state.core.leverage.push(LeverageEntry {
            account: account1,
            market: 0,
            leverage: 10,
        });
        state.core.leverage.push(LeverageEntry {
            account: account2,
            market: 1,
            leverage: 20,
        });

        // Add orders
        state.core.orders.push(OrderEntry {
            market: 0,
            order_id: 100,
            order: Order {
                id: 100,
                owner: account1,
                market_id: 0,
                side: OrderSide::Buy,
                price: hypercore_primitives::Decimal::price("64000"),
                original_size: hypercore_primitives::Decimal::size("0.1"),
                remaining_size: hypercore_primitives::Decimal::size("0.1"),
                status: OrderStatus::Open,
                order_type: OrderType::Limit { tif: TimeInForce::Gtc },
                reduce_only: false,
                post_only: false,
                client_order_id: Some("test-order-1".to_string()),
                timestamp: 1706054400000,
            },
        });

        // Set next order ID
        state.core.next_order_id = 101;

        // Set insurance fund
        state.core.insurance_fund = "1000000000".to_string(); // 1,000 USDC

        // Add spot tokens
        state.spot.tokens.push(SpotTokenEntry {
            index: 0,
            symbol: "USDC".to_string(),
            name: "USD Coin".to_string(),
            wei_decimals: 6,
            sz_decimals: 2,
            max_supply: "1000000000000000".to_string(),
            deployer: AccountAddress::from([0u8; 20]),
        });
        state.spot.tokens.push(SpotTokenEntry {
            index: 1,
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            wei_decimals: 18,
            sz_decimals: 4,
            max_supply: "1000000000000000000000000000".to_string(),
            deployer: AccountAddress::from([0u8; 20]),
        });

        // Set spot counters
        state.spot.next_token_index = 2;
        state.spot.next_market_id = 1;

        // Add nonces
        state.chain.nonces.push(NonceEntry {
            account: account1,
            nonce: 42,
        });
        state.chain.nonces.push(NonceEntry {
            account: account2,
            nonce: 15,
        });

        state
    }

    #[test]
    fn test_json_serialization_roundtrip() {
        // Test that PersistedState can be serialized to JSON and back
        let original = create_comprehensive_test_state();

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&original)
            .expect("Failed to serialize state to JSON");

        // Deserialize from JSON
        let restored: PersistedState = serde_json::from_str(&json)
            .expect("Failed to deserialize state from JSON");

        // Verify all fields match
        assert_eq!(restored.height, original.height);
        assert_eq!(restored.timestamp, original.timestamp);
        assert_eq!(restored.app_hash, original.app_hash);
        assert_eq!(restored.schema_version, original.schema_version);

        // Verify balances
        assert_eq!(restored.core.balances.len(), original.core.balances.len());
        for (orig, rest) in original.core.balances.iter().zip(restored.core.balances.iter()) {
            assert_eq!(orig.account, rest.account);
            assert_eq!(orig.token, rest.token);
            assert_eq!(orig.balance.total, rest.balance.total);
            assert_eq!(orig.balance.core_view, rest.balance.core_view);
            assert_eq!(orig.balance.evm_view, rest.balance.evm_view);
        }

        // Verify positions
        assert_eq!(restored.core.positions.len(), original.core.positions.len());

        // Verify leverage
        assert_eq!(restored.core.leverage.len(), original.core.leverage.len());

        // Verify orders
        assert_eq!(restored.core.orders.len(), original.core.orders.len());

        // Verify spot tokens
        assert_eq!(restored.spot.tokens.len(), original.spot.tokens.len());

        // Verify nonces
        assert_eq!(restored.chain.nonces.len(), original.chain.nonces.len());
    }

    #[test]
    fn test_json_export_produces_valid_structure() {
        let state = create_comprehensive_test_state();

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&state)
            .expect("Failed to serialize state");

        // Parse as generic JSON to verify structure
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("Failed to parse JSON");

        // Verify top-level structure
        assert!(parsed.is_object());
        assert!(parsed.get("schema_version").is_some());
        assert!(parsed.get("height").is_some());
        assert!(parsed.get("timestamp").is_some());
        assert!(parsed.get("app_hash").is_some());
        assert!(parsed.get("core").is_some());
        assert!(parsed.get("spot").is_some());
        assert!(parsed.get("evm").is_some());
        assert!(parsed.get("chain").is_some());

        // Verify nested structure
        let core = parsed.get("core").unwrap();
        assert!(core.get("balances").is_some());
        assert!(core.get("positions").is_some());
        assert!(core.get("orders").is_some());
    }

    #[test]
    fn test_import_validates_state() {
        // Valid state should pass validation
        let valid_state = create_comprehensive_test_state();
        assert!(valid_state.validate().is_ok());

        // Invalid state should fail validation
        let mut invalid_state = PersistedState::new();
        invalid_state.height = 1;
        invalid_state.core.balances.push(BalanceEntry {
            account: AccountAddress::from([1u8; 20]),
            token: 0,
            balance: UnifiedBalanceData {
                total: "100".to_string(),
                core_view: "70".to_string(),
                evm_view: "40".to_string(), // 70 + 40 = 110 != 100
                decimals: 6,
            },
        });
        assert!(invalid_state.validate().is_err());
    }

    #[test]
    fn test_persist_load_comprehensive_state() {
        let (backend, _temp_dir) = create_test_backend();
        let persister = StatePersister::new(&backend);

        let original = create_comprehensive_test_state();

        // Persist state
        persister.persist_state(&original).expect("Failed to persist state");

        // Load state
        let loaded = persister.load_state()
            .expect("Failed to load state")
            .expect("No state found");

        // Verify height and timestamp
        assert_eq!(loaded.height, original.height);
        assert_eq!(loaded.timestamp, original.timestamp);

        // Verify balances count
        assert_eq!(loaded.core.balances.len(), original.core.balances.len());

        // Verify positions count
        assert_eq!(loaded.core.positions.len(), original.core.positions.len());

        // Verify orders count
        assert_eq!(loaded.core.orders.len(), original.core.orders.len());

        // Verify nonces count
        assert_eq!(loaded.chain.nonces.len(), original.chain.nonces.len());
    }

    #[test]
    fn test_export_import_roundtrip_via_persistence() {
        // This simulates the full export -> import workflow
        let (backend1, _temp_dir1) = create_test_backend();
        let (backend2, _temp_dir2) = create_test_backend();

        let persister1 = StatePersister::new(&backend1);
        let persister2 = StatePersister::new(&backend2);

        // Create and persist original state
        let original = create_comprehensive_test_state();
        persister1.persist_state(&original).expect("Failed to persist");

        // Load state (simulates export)
        let exported = persister1.load_state()
            .expect("Failed to load")
            .expect("No state found");

        // Serialize to JSON (export step)
        let json = serde_json::to_string(&exported)
            .expect("Failed to serialize");

        // Deserialize from JSON (import step)
        let imported: PersistedState = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        // Validate imported state
        imported.validate().expect("Imported state validation failed");

        // Persist to new backend (import step)
        persister2.persist_state(&imported).expect("Failed to persist imported state");

        // Load from new backend to verify
        let final_state = persister2.load_state()
            .expect("Failed to load final state")
            .expect("No final state found");

        // Verify consistency
        assert_eq!(final_state.height, original.height);
        assert_eq!(final_state.timestamp, original.timestamp);
        assert_eq!(final_state.core.balances.len(), original.core.balances.len());
        assert_eq!(final_state.core.positions.len(), original.core.positions.len());
        assert_eq!(final_state.core.orders.len(), original.core.orders.len());
    }

    #[test]
    fn test_import_rejects_corrupted_json() {
        // Test various forms of corrupted JSON
        let test_cases = vec![
            ("", "empty string"),
            ("{}", "empty object"),
            ("{\"height\": \"not_a_number\"}", "invalid type"),
            ("not json at all", "invalid JSON syntax"),
        ];

        for (json, description) in test_cases {
            let result: std::result::Result<PersistedState, _> = serde_json::from_str(json);
            assert!(
                result.is_err(),
                "Should reject: {}",
                description
            );
        }

        // Test schema version mismatch separately (requires valid JSON structure)
        let wrong_version_json = r#"{
            "schema_version": 999,
            "height": 1,
            "timestamp": 0,
            "app_hash": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
            "core": {"balances":[],"accounts":[],"positions":[],"leverage":[],"markets":[],"orders":[],"next_order_id":0,"insurance_fund":"0"},
            "spot": {"tokens":[],"markets":[],"reserved":[],"orders":[],"next_token_index":0,"next_market_id":0,"next_order_id":0},
            "evm": {"accounts":[],"storage":[],"code":[],"block_hashes":[]},
            "chain": {"nonces":[],"blocks":[],"cloid_index":[]}
        }"#;
        let parsed: std::result::Result<PersistedState, _> = serde_json::from_str(wrong_version_json);
        if let Ok(state) = parsed {
            assert!(state.validate().is_err(), "Should reject wrong schema version");
        }
    }

    #[test]
    fn test_export_handles_empty_state() {
        let (backend, _temp_dir) = create_test_backend();
        let persister = StatePersister::new(&backend);

        // Persist minimal state
        let mut state = PersistedState::new();
        state.height = 1;
        state.timestamp = 1000;

        persister.persist_state(&state).expect("Failed to persist");

        // Load and serialize to JSON
        let loaded = persister.load_state()
            .expect("Failed to load")
            .expect("No state found");

        let json = serde_json::to_string_pretty(&loaded)
            .expect("Failed to serialize");

        // Should produce valid JSON even for empty state
        let parsed: PersistedState = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        assert_eq!(parsed.height, 1);
        assert!(parsed.core.balances.is_empty());
        assert!(parsed.core.positions.is_empty());
        assert!(parsed.core.orders.is_empty());
    }

    #[test]
    fn test_state_overwrite_on_import() {
        let (backend, _temp_dir) = create_test_backend();
        let persister = StatePersister::new(&backend);

        // First persist some state
        let mut state1 = PersistedState::new();
        state1.height = 100;
        state1.timestamp = 1000;
        state1.core.balances.push(BalanceEntry {
            account: AccountAddress::from([1u8; 20]),
            token: 0,
            balance: UnifiedBalanceData {
                total: "500".to_string(),
                core_view: "500".to_string(),
                evm_view: "0".to_string(),
                decimals: 6,
            },
        });
        persister.persist_state(&state1).expect("Failed to persist state1");

        // Then persist different state (simulating import overwrite)
        let mut state2 = PersistedState::new();
        state2.height = 200;
        state2.timestamp = 2000;
        state2.core.balances.push(BalanceEntry {
            account: AccountAddress::from([2u8; 20]),
            token: 1,
            balance: UnifiedBalanceData {
                total: "1000".to_string(),
                core_view: "1000".to_string(),
                evm_view: "0".to_string(),
                decimals: 6,
            },
        });
        persister.persist_state(&state2).expect("Failed to persist state2");

        // Load and verify we have state2
        let loaded = persister.load_state()
            .expect("Failed to load")
            .expect("No state found");

        assert_eq!(loaded.height, 200, "Height should be from state2");
        assert_eq!(loaded.timestamp, 2000, "Timestamp should be from state2");
        // Note: balances accumulate in RocksDB, but height/timestamp reflect latest
    }

    #[test]
    fn test_large_state_json_serialization() {
        let mut state = PersistedState::new();
        state.height = 1;
        state.timestamp = 1000;

        // Add many balances
        for i in 0..100 {
            let mut addr = [0u8; 20];
            addr[0] = (i / 256) as u8;
            addr[1] = (i % 256) as u8;

            state.core.balances.push(BalanceEntry {
                account: AccountAddress::from(addr),
                token: 0,
                balance: UnifiedBalanceData {
                    total: format!("{}", 1000 * (i + 1)),
                    core_view: format!("{}", 1000 * (i + 1)),
                    evm_view: "0".to_string(),
                    decimals: 6,
                },
            });
        }

        // Serialize to JSON
        let json = serde_json::to_string(&state)
            .expect("Failed to serialize large state");

        // Deserialize back
        let restored: PersistedState = serde_json::from_str(&json)
            .expect("Failed to deserialize large state");

        assert_eq!(restored.core.balances.len(), 100);
    }

    #[test]
    fn test_special_characters_in_client_order_id() {
        use hypercore_primitives::{Order, OrderSide, OrderStatus, OrderType, TimeInForce};

        let mut state = PersistedState::new();
        state.height = 1;

        // Test with special characters in client_order_id
        state.core.orders.push(OrderEntry {
            market: 0,
            order_id: 1,
            order: Order {
                id: 1,
                owner: AccountAddress::from([1u8; 20]),
                market_id: 0,
                side: OrderSide::Buy,
                price: hypercore_primitives::Decimal::price("100"),
                original_size: hypercore_primitives::Decimal::size("1"),
                remaining_size: hypercore_primitives::Decimal::size("1"),
                status: OrderStatus::Open,
                order_type: OrderType::Limit { tif: TimeInForce::Gtc },
                reduce_only: false,
                post_only: false,
                client_order_id: Some("test-🚀-order-\n\t\"special\"".to_string()),
                timestamp: 1000,
            },
        });

        // Should serialize and deserialize correctly
        let json = serde_json::to_string(&state)
            .expect("Failed to serialize");
        let restored: PersistedState = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        assert_eq!(
            restored.core.orders[0].order.client_order_id,
            Some("test-🚀-order-\n\t\"special\"".to_string())
        );
    }
}
