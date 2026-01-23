//! RocksDB Backend Implementation
//!
//! This module implements the PersistenceBackend trait using RocksDB.
//! It handles database lifecycle, column families, and atomic operations.

use crate::{
    column_families::ColumnFamily,
    error::{PersistenceError, Result},
    keys::KeyEncoder,
    state::SCHEMA_VERSION,
    PersistenceBackend, PersistenceConfig, SnapshotReader, WriteBatch,
};
use rocksdb::{
    ColumnFamilyDescriptor, Options,
    WriteBatch as RocksWriteBatch, WriteOptions, DB,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// RocksDB-based persistence backend
pub struct RocksDbBackend {
    db: Arc<DB>,
    cf_handles: HashMap<ColumnFamily, String>,
    config: PersistenceConfig,
}

impl RocksDbBackend {
    /// Create column family options
    fn column_family_options() -> Options {
        let mut opts = Options::default();
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts.set_write_buffer_size(32 * 1024 * 1024); // 32MB
        opts.set_max_write_buffer_number(2);
        opts.set_target_file_size_base(64 * 1024 * 1024); // 64MB
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts
    }

    /// Get column family handle name
    fn cf_name(&self, cf: ColumnFamily) -> &str {
        self.cf_handles.get(&cf).map(|s| s.as_str()).unwrap_or("default")
    }

    /// Initialize schema version if not set
    fn init_schema_version(&self) -> Result<()> {
        let cf = self.db.cf_handle(ColumnFamily::Metadata.name());
        let key = KeyEncoder::METADATA_SCHEMA_VERSION;

        if let Some(cf) = cf {
            if self.db.get_cf(&cf, key)?.is_none() {
                let version_bytes = SCHEMA_VERSION.to_le_bytes();
                self.db.put_cf(&cf, key, version_bytes)?;
                info!("Initialized schema version: {}", SCHEMA_VERSION);
            }
        }
        Ok(())
    }

    /// Check schema version compatibility
    fn check_schema_version(&self) -> Result<()> {
        let cf = self.db.cf_handle(ColumnFamily::Metadata.name());

        if let Some(cf) = cf {
            if let Some(bytes) = self.db.get_cf(&cf, KeyEncoder::METADATA_SCHEMA_VERSION)? {
                if bytes.len() >= 4 {
                    let version = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    if version != SCHEMA_VERSION {
                        return Err(PersistenceError::VersionMismatch {
                            expected: SCHEMA_VERSION,
                            found: version,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl PersistenceBackend for RocksDbBackend {
    fn open(config: &PersistenceConfig) -> Result<Self> {
        let path = Path::new(&config.data_dir);
        info!("Opening RocksDB at: {:?}", path);

        // Create database options
        let mut opts = Options::default();
        opts.create_if_missing(config.create_if_missing);
        opts.create_missing_column_families(true);
        opts.set_max_open_files(config.max_open_files);
        opts.set_write_buffer_size(config.write_buffer_size);
        opts.set_max_write_buffer_number(config.max_write_buffer_number as i32);

        if config.enable_compression {
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }

        // WAL configuration
        if !config.enable_wal {
            opts.set_manual_wal_flush(true);
        }

        // Helper to create column family descriptors
        let create_descriptors = || {
            ColumnFamily::all()
                .iter()
                .map(|cf| ColumnFamilyDescriptor::new(cf.name(), Self::column_family_options()))
                .collect::<Vec<_>>()
        };

        // Open database with column families
        let db = if path.exists() {
            // Try to open existing database
            match DB::open_cf_descriptors(&opts, path, create_descriptors()) {
                Ok(db) => db,
                Err(e) => {
                    warn!("Failed to open existing DB: {}. Creating new.", e);
                    // If it fails, try creating new
                    std::fs::create_dir_all(path)?;
                    DB::open_cf_descriptors(&opts, path, create_descriptors())?
                }
            }
        } else {
            std::fs::create_dir_all(path)?;
            DB::open_cf_descriptors(&opts, path, create_descriptors())?
        };

        // Build CF handle map
        let mut cf_handles = HashMap::new();
        for cf in ColumnFamily::all() {
            cf_handles.insert(*cf, cf.name().to_string());
        }

        let backend = Self {
            db: Arc::new(db),
            cf_handles,
            config: config.clone(),
        };

        // Initialize or check schema version
        backend.init_schema_version()?;
        backend.check_schema_version()?;

        info!("RocksDB opened successfully with {} column families", ColumnFamily::all().len());
        Ok(backend)
    }

    fn close(&self) -> Result<()> {
        info!("Closing RocksDB...");
        self.flush()?;
        // DB is closed when dropped
        Ok(())
    }

    fn get(&self, cf: ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let cf_handle = self.db.cf_handle(cf.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(cf.name().to_string()))?;

        Ok(self.db.get_cf(&cf_handle, key)?)
    }

    fn put(&self, cf: ColumnFamily, key: &[u8], value: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(cf.name().to_string()))?;

        let mut write_opts = WriteOptions::default();
        if self.config.sync_wal {
            write_opts.set_sync(true);
        }

        Ok(self.db.put_cf_opt(&cf_handle, key, value, &write_opts)?)
    }

    fn delete(&self, cf: ColumnFamily, key: &[u8]) -> Result<()> {
        let cf_handle = self.db.cf_handle(cf.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(cf.name().to_string()))?;

        Ok(self.db.delete_cf(&cf_handle, key)?)
    }

    fn create_batch(&self) -> WriteBatch {
        WriteBatch::new()
    }

    fn commit_batch(&self, batch: WriteBatch) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        let mut rocks_batch = RocksWriteBatch::default();

        for op in batch.operations() {
            let cf_handle = self.db.cf_handle(op.cf().name())
                .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(op.cf().name().to_string()))?;

            if op.is_delete() {
                rocks_batch.delete_cf(&cf_handle, op.key());
            } else if let Some(value) = op.value() {
                rocks_batch.put_cf(&cf_handle, op.key(), value);
            }
        }

        let mut write_opts = WriteOptions::default();
        if self.config.sync_wal {
            write_opts.set_sync(true);
        }

        debug!("Committing batch with {} operations", batch.len());
        Ok(self.db.write_opt(rocks_batch, &write_opts)?)
    }

    fn prefix_scan(
        &self,
        cf: ColumnFamily,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let cf_handle = self.db.cf_handle(cf.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(cf.name().to_string()))?;

        let mut results = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf_handle, prefix);

        for item in iter {
            match item {
                Ok((key, value)) => {
                    if key.starts_with(prefix) {
                        results.push((key.to_vec(), value.to_vec()));
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    error!("Error during prefix scan: {}", e);
                    return Err(PersistenceError::RocksDb(e));
                }
            }
        }

        Ok(results)
    }

    fn get_height(&self) -> Result<u64> {
        let cf_handle = self.db.cf_handle(ColumnFamily::Metadata.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound("metadata".to_string()))?;

        match self.db.get_cf(&cf_handle, KeyEncoder::METADATA_HEIGHT)? {
            Some(bytes) if bytes.len() >= 8 => {
                Ok(u64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5], bytes[6], bytes[7],
                ]))
            }
            _ => Ok(0),
        }
    }

    fn set_height(&self, height: u64) -> Result<()> {
        self.put(
            ColumnFamily::Metadata,
            KeyEncoder::METADATA_HEIGHT,
            &height.to_le_bytes(),
        )
    }

    fn get_app_hash(&self, height: u64) -> Result<Option<[u8; 32]>> {
        let key = KeyEncoder::block_key(height);
        match self.get(ColumnFamily::AppHashes, &key)? {
            Some(bytes) if bytes.len() == 32 => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&bytes);
                Ok(Some(hash))
            }
            _ => Ok(None),
        }
    }

    fn flush(&self) -> Result<()> {
        info!("Flushing RocksDB memtables...");
        for cf in ColumnFamily::all() {
            if let Some(cf_handle) = self.db.cf_handle(cf.name()) {
                self.db.flush_cf(&cf_handle)?;
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Box<dyn SnapshotReader>> {
        // Create a consistent snapshot using RocksDB's checkpoint feature
        //
        // Note: Due to Rust lifetime constraints with RocksDB's Snapshot<'a> type,
        // we use a two-phase approach:
        // 1. For simple reads: Arc<DB> with direct reads (consistent within single operation)
        // 2. For state sync: Use checkpoint_to_path() to create a full checkpoint
        //
        // This implementation uses direct DB reads which are consistent within
        // individual operations. For full state sync, use create_checkpoint().
        Ok(Box::new(ConsistentSnapshot::new(Arc::clone(&self.db))))
    }
}

impl RocksDbBackend {
    /// Create a checkpoint of the database at the given path
    ///
    /// This creates a complete, consistent copy of the database that can be used
    /// for state sync or backup. The checkpoint is created atomically using
    /// RocksDB's hard-link based checkpoint mechanism (fast, space-efficient).
    ///
    /// For state sync:
    /// 1. Call `create_checkpoint()` to create a consistent point-in-time copy
    /// 2. Open the checkpoint as a separate `RocksDbBackend`
    /// 3. Stream all state from the checkpoint to the syncing node
    /// 4. Delete the checkpoint after sync completes
    pub fn create_checkpoint(&self, path: &Path) -> Result<()> {
        use rocksdb::checkpoint::Checkpoint;

        let checkpoint = Checkpoint::new(&self.db)
            .map_err(|e| PersistenceError::RocksDb(e))?;

        checkpoint.create_checkpoint(path)
            .map_err(|e| PersistenceError::RocksDb(e))?;

        info!("Created checkpoint at {:?}", path);
        Ok(())
    }
}

/// Consistent snapshot implementation for read operations
///
/// This provides consistent reads by holding a reference to the DB.
/// Within a single read operation (get or prefix_scan), the read is atomic.
/// For multi-operation consistency, use create_checkpoint() to create a
/// full database checkpoint that can be opened as a separate DB.
///
/// For state sync scenarios:
/// 1. Call create_checkpoint() to create a consistent checkpoint
/// 2. Open the checkpoint as a separate RocksDbBackend
/// 3. Read all state from the checkpoint
struct ConsistentSnapshot {
    db: Arc<DB>,
}

// Safety: RocksDB is thread-safe for concurrent reads
unsafe impl Send for ConsistentSnapshot {}
unsafe impl Sync for ConsistentSnapshot {}

impl ConsistentSnapshot {
    fn new(db: Arc<DB>) -> Self {
        Self { db }
    }
}

impl SnapshotReader for ConsistentSnapshot {
    fn get(&self, cf: ColumnFamily, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let cf_handle = self.db.cf_handle(cf.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(cf.name().to_string()))?;

        Ok(self.db.get_cf(&cf_handle, key)?)
    }

    fn prefix_scan(
        &self,
        cf: ColumnFamily,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let cf_handle = self.db.cf_handle(cf.name())
            .ok_or_else(|| PersistenceError::ColumnFamilyNotFound(cf.name().to_string()))?;

        let mut results = Vec::new();
        let iter = self.db.prefix_iterator_cf(&cf_handle, prefix);

        for item in iter {
            match item {
                Ok((key, value)) => {
                    if key.starts_with(prefix) {
                        results.push((key.to_vec(), value.to_vec()));
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    return Err(PersistenceError::RocksDb(e));
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_open_close() {
        let (backend, _temp_dir) = create_test_backend();
        backend.close().unwrap();
    }

    #[test]
    fn test_put_get() {
        let (backend, _temp_dir) = create_test_backend();

        backend.put(ColumnFamily::Balances, b"test_key", b"test_value").unwrap();
        let value = backend.get(ColumnFamily::Balances, b"test_key").unwrap();

        assert_eq!(value, Some(b"test_value".to_vec()));
    }

    #[test]
    fn test_delete() {
        let (backend, _temp_dir) = create_test_backend();

        backend.put(ColumnFamily::Balances, b"test_key", b"test_value").unwrap();
        backend.delete(ColumnFamily::Balances, b"test_key").unwrap();
        let value = backend.get(ColumnFamily::Balances, b"test_key").unwrap();

        assert_eq!(value, None);
    }

    #[test]
    fn test_batch_write() {
        let (backend, _temp_dir) = create_test_backend();

        let mut batch = backend.create_batch();
        batch.put(ColumnFamily::Balances, b"key1".to_vec(), b"value1".to_vec());
        batch.put(ColumnFamily::Balances, b"key2".to_vec(), b"value2".to_vec());
        batch.delete(ColumnFamily::Balances, b"key1".to_vec());

        backend.commit_batch(batch).unwrap();

        assert_eq!(backend.get(ColumnFamily::Balances, b"key1").unwrap(), None);
        assert_eq!(backend.get(ColumnFamily::Balances, b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_height() {
        let (backend, _temp_dir) = create_test_backend();

        assert_eq!(backend.get_height().unwrap(), 0);

        backend.set_height(100).unwrap();
        assert_eq!(backend.get_height().unwrap(), 100);

        backend.set_height(200).unwrap();
        assert_eq!(backend.get_height().unwrap(), 200);
    }

    #[test]
    fn test_prefix_scan() {
        let (backend, _temp_dir) = create_test_backend();

        backend.put(ColumnFamily::Balances, b"prefix_a", b"value_a").unwrap();
        backend.put(ColumnFamily::Balances, b"prefix_b", b"value_b").unwrap();
        backend.put(ColumnFamily::Balances, b"prefix_c", b"value_c").unwrap();
        backend.put(ColumnFamily::Balances, b"other_key", b"other_value").unwrap();

        let results = backend.prefix_scan(ColumnFamily::Balances, b"prefix_").unwrap();

        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_snapshot() {
        let (backend, _temp_dir) = create_test_backend();

        backend.put(ColumnFamily::Balances, b"key1", b"value1").unwrap();

        // Take snapshot
        let snapshot = backend.snapshot().unwrap();

        // Snapshot can read the data
        assert_eq!(
            snapshot.get(ColumnFamily::Balances, b"key1").unwrap(),
            Some(b"value1".to_vec())
        );
    }

    #[test]
    fn test_checkpoint() {
        let (backend, _temp_dir) = create_test_backend();

        // Write some data
        backend.put(ColumnFamily::Balances, b"key1", b"value1").unwrap();
        backend.put(ColumnFamily::Nonces, b"key2", b"value2").unwrap();
        backend.set_height(42).unwrap();

        // Create a checkpoint
        let checkpoint_dir = TempDir::new().unwrap();
        let checkpoint_path = checkpoint_dir.path().join("checkpoint");
        backend.create_checkpoint(&checkpoint_path).unwrap();

        // Open the checkpoint as a new backend
        let checkpoint_config = PersistenceConfig {
            data_dir: checkpoint_path.to_string_lossy().to_string(),
            create_if_missing: false,
            ..Default::default()
        };
        let checkpoint_backend = RocksDbBackend::open(&checkpoint_config).unwrap();

        // Verify the checkpoint has the same data
        assert_eq!(
            checkpoint_backend.get(ColumnFamily::Balances, b"key1").unwrap(),
            Some(b"value1".to_vec())
        );
        assert_eq!(
            checkpoint_backend.get(ColumnFamily::Nonces, b"key2").unwrap(),
            Some(b"value2".to_vec())
        );
        assert_eq!(checkpoint_backend.get_height().unwrap(), 42);

        // Write new data to original - should not affect checkpoint
        backend.put(ColumnFamily::Balances, b"key3", b"value3").unwrap();
        assert_eq!(
            checkpoint_backend.get(ColumnFamily::Balances, b"key3").unwrap(),
            None
        );
    }
}
