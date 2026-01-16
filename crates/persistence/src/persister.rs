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
    pub fn persist_state(&self, state: &PersistedState) -> Result<()> {
        info!("Persisting state at height {}", state.height);

        // Validate state before persisting
        state.validate().map_err(PersistenceError::invalid_state)?;

        let mut batch = self.backend.create_batch();

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

        // Persist positions
        for position in &state.core.positions {
            let key = KeyEncoder::position_key(&position.account, position.market);
            let value = bincode::serialize(&position.position)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Positions, key, value);
        }

        // Persist leverage settings
        for leverage in &state.core.leverage {
            let key = KeyEncoder::position_key(&leverage.account, leverage.market);
            batch.put(ColumnFamily::Leverage, key, vec![leverage.leverage]);
        }

        // Persist markets
        for market in &state.core.markets {
            let key = KeyEncoder::market_key(market.id);
            let value = serde_json::to_vec(market)
                .map_err(PersistenceError::serialization)?;
            batch.put(ColumnFamily::Markets, key, value);
        }

        // Persist orders
        for order in &state.core.orders {
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
            },
        });

        let result = validate_state(&state);
        assert!(result.is_err());
    }
}
