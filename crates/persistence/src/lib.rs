//! HyperCore State Persistence Layer
//!
//! This crate provides RocksDB-based state persistence for HyperCore.
//! It uses column families to organize different types of state data
//! and supports atomic batch writes for block-level consistency.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    PersistenceLayer                         │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
//! │  │  Balances   │  │   Orders    │  │    EVM      │  ...    │
//! │  │  (CF)       │  │   (CF)      │  │   (CF)      │         │
//! │  └─────────────┘  └─────────────┘  └─────────────┘         │
//! │                                                             │
//! │  ┌───────────────────────────────────────────────────────┐ │
//! │  │                    RocksDB                             │ │
//! │  │  ┌─────────┐  ┌─────────┐                             │ │
//! │  │  │   WAL   │  │  SST    │                             │ │
//! │  │  │  Files  │  │  Files  │                             │ │
//! │  │  └─────────┘  └─────────┘                             │ │
//! │  └───────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod column_families;
mod error;
mod extractor;
mod keys;
mod persister;
mod rocksdb_backend;
mod snapshot;
mod state;

pub use column_families::ColumnFamily;
pub use error::{PersistenceError, Result};
pub use extractor::{
    cloid_index_to_entries, extract_unified_balances, leverage_to_entries,
    nonces_to_entries, orders_to_entries, positions_to_entries, StateExtractor,
};
pub use keys::{KeyEncoder, KeyPrefix};
pub use persister::{validate_state, StatePersister};
pub use rocksdb_backend::RocksDbBackend;
pub use snapshot::{
    SnapshotManager, SnapshotMetadata, SnapshotRestore, DEFAULT_CHUNK_SIZE,
    MAX_SNAPSHOTS, SNAPSHOT_FORMAT,
};
pub use state::{
    AccountEntry, BalanceEntry, BlockMetaEntry, ChainState, CloidEntry, CoreState,
    EvmAccountEntry, EvmBlockHashEntry, EvmCodeEntry, EvmStateData, EvmStorageEntry,
    LeverageEntry, MarketEntry, NonceEntry, OrderEntry, PerpState, PersistedState,
    PositionEntry, ReservedEntry, SpotMarketEntry, SpotState, SpotTokenEntry,
    StateSnapshot, UnifiedBalanceData, SCHEMA_VERSION,
};


/// Configuration for the persistence layer
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// Path to the RocksDB data directory
    pub data_dir: String,
    /// Whether to create the database if it doesn't exist
    pub create_if_missing: bool,
    /// Maximum number of open files
    pub max_open_files: i32,
    /// Write buffer size in bytes (default: 64MB)
    pub write_buffer_size: usize,
    /// Maximum number of write buffers
    pub max_write_buffer_number: i32,
    /// Enable WAL (write-ahead log) for crash recovery
    pub enable_wal: bool,
    /// Sync WAL on every write (slower but safer)
    pub sync_wal: bool,
    /// Enable compression (LZ4)
    pub enable_compression: bool,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data/chain".to_string(),
            create_if_missing: true,
            max_open_files: 1000,
            write_buffer_size: 64 * 1024 * 1024, // 64MB
            max_write_buffer_number: 3,
            enable_wal: true,
            sync_wal: false, // async for performance
            enable_compression: true,
        }
    }
}

/// Trait for persistence backends
pub trait PersistenceBackend: Send + Sync {
    /// Open or create the database
    fn open(config: &PersistenceConfig) -> Result<Self>
    where
        Self: Sized;

    /// Close the database gracefully
    fn close(&self) -> Result<()>;

    /// Get a value by key from a column family
    fn get(&self, cf: ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Put a value by key into a column family
    fn put(&self, cf: ColumnFamily, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key from a column family
    fn delete(&self, cf: ColumnFamily, key: &[u8]) -> Result<()>;

    /// Create a new write batch
    fn create_batch(&self) -> WriteBatch;

    /// Commit a write batch atomically
    fn commit_batch(&self, batch: WriteBatch) -> Result<()>;

    /// Iterate over all keys with a given prefix
    fn prefix_scan(
        &self,
        cf: ColumnFamily,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;

    /// Get current block height
    fn get_height(&self) -> Result<u64>;

    /// Set current block height
    fn set_height(&self, height: u64) -> Result<()>;

    /// Get app hash for a height
    fn get_app_hash(&self, height: u64) -> Result<Option<[u8; 32]>>;

    /// Flush all memtables to disk
    fn flush(&self) -> Result<()>;

    /// Create a point-in-time snapshot
    fn snapshot(&self) -> Result<Box<dyn SnapshotReader>>;
}

/// Read-only snapshot for consistent reads
pub trait SnapshotReader: Send + Sync {
    fn get(&self, cf: ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn prefix_scan(
        &self,
        cf: ColumnFamily,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
}

/// Write batch for atomic operations
#[derive(Default)]
pub struct WriteBatch {
    operations: Vec<BatchOperation>,
}

enum BatchOperation {
    Put {
        cf: ColumnFamily,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        cf: ColumnFamily,
        key: Vec<u8>,
    },
}

impl WriteBatch {
    /// Create a new empty batch
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a put operation to the batch
    pub fn put(&mut self, cf: ColumnFamily, key: Vec<u8>, value: Vec<u8>) {
        self.operations.push(BatchOperation::Put { cf, key, value });
    }

    /// Add a delete operation to the batch
    pub fn delete(&mut self, cf: ColumnFamily, key: Vec<u8>) {
        self.operations.push(BatchOperation::Delete { cf, key });
    }

    /// Get the number of operations in the batch
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Consume the batch and return the operations
    pub fn into_operations(self) -> Vec<BatchOperation> {
        self.operations
    }
}

/// Helper to access batch operations internally
impl WriteBatch {
    pub(crate) fn operations(&self) -> &[BatchOperation] {
        &self.operations
    }
}

impl BatchOperation {
    pub(crate) fn cf(&self) -> ColumnFamily {
        match self {
            BatchOperation::Put { cf, .. } => *cf,
            BatchOperation::Delete { cf, .. } => *cf,
        }
    }

    pub(crate) fn key(&self) -> &[u8] {
        match self {
            BatchOperation::Put { key, .. } => key,
            BatchOperation::Delete { key, .. } => key,
        }
    }

    pub(crate) fn value(&self) -> Option<&[u8]> {
        match self {
            BatchOperation::Put { value, .. } => Some(value),
            BatchOperation::Delete { .. } => None,
        }
    }

    pub(crate) fn is_delete(&self) -> bool {
        matches!(self, BatchOperation::Delete { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_batch() {
        let mut batch = WriteBatch::new();
        assert!(batch.is_empty());

        batch.put(ColumnFamily::Balances, vec![1, 2, 3], vec![4, 5, 6]);
        assert_eq!(batch.len(), 1);

        batch.delete(ColumnFamily::Balances, vec![1, 2, 3]);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn test_default_config() {
        let config = PersistenceConfig::default();
        assert!(config.create_if_missing);
        assert!(config.enable_wal);
        assert!(config.enable_compression);
    }
}
