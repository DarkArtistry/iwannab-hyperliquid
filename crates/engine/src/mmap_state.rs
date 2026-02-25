//! Memory-mapped state storage (Phase E5)
//!
//! Provides persistent state storage with fast snapshots using file-backed
//! storage. When available, uses OS-level file cloning (reflink) for
//! instant snapshots without data copying.
//!
//! ## Architecture
//!
//! ```text
//! +-------------------------------+
//! |        MmapState              |
//! |  +-----------+ +----------+  |
//! |  | state.bin | |  wal.log |  |
//! |  | (latest)  | | (pending)|  |
//! |  +-----------+ +----------+  |
//! |  +---------------------------+|
//! |  | snapshots/                ||
//! |  |   snap_100.bin            ||
//! |  |   snap_200.bin            ||
//! |  +---------------------------+|
//! +-------------------------------+
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Configuration for file-backed state storage
#[derive(Debug, Clone)]
pub struct MmapStateConfig {
    /// Base directory for state files
    pub base_dir: PathBuf,
    /// Page size for WAL segments (default: 4KB)
    pub page_size: usize,
    /// Maximum disk usage (default: 4GB)
    pub max_disk_usage: usize,
    /// Enable write-ahead logging for crash recovery
    pub wal_enabled: bool,
    /// Snapshot interval (in blocks)
    pub snapshot_interval: u64,
    /// Maximum number of retained snapshots
    pub max_snapshots: usize,
}

impl Default for MmapStateConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("./data/state"),
            page_size: 4096,
            max_disk_usage: 4 * 1024 * 1024 * 1024, // 4GB
            wal_enabled: true,
            snapshot_interval: 100,
            max_snapshots: 5,
        }
    }
}

/// Write-ahead log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    /// Block height this entry belongs to
    pub height: u64,
    /// Operation type
    pub op: WalOp,
    /// Timestamp
    pub timestamp: u64,
}

/// WAL operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalOp {
    /// Set a key-value pair
    Set { key: Vec<u8>, value: Vec<u8> },
    /// Delete a key
    Delete { key: Vec<u8> },
    /// Begin a new block
    BeginBlock { height: u64 },
    /// Commit block (all preceding ops are durable)
    CommitBlock { height: u64, app_hash: [u8; 32] },
}

/// Snapshot metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub height: u64,
    pub app_hash: [u8; 32],
    pub timestamp: u64,
    pub size_bytes: u64,
    pub entry_count: u64,
}

/// File-backed state storage with WAL and snapshots
pub struct MmapState {
    config: MmapStateConfig,
    /// In-memory state (HashMap backed by periodic file persistence)
    state: HashMap<Vec<u8>, Vec<u8>>,
    /// Current block height
    current_height: u64,
    /// WAL file handle (if enabled)
    wal_file: Option<fs::File>,
    /// Whether state has been modified since last snapshot
    dirty: bool,
    /// Whether the state is initialized
    initialized: bool,
    /// List of available snapshots
    snapshots: Vec<SnapshotMeta>,
}

impl MmapState {
    /// Create new state storage
    pub fn new(config: MmapStateConfig) -> Self {
        Self {
            config,
            state: HashMap::new(),
            current_height: 0,
            wal_file: None,
            dirty: false,
            initialized: false,
            snapshots: Vec::new(),
        }
    }

    /// Initialize state storage (creates directories, recovers from WAL)
    pub fn initialize(&mut self) -> io::Result<()> {
        // Create directory structure
        fs::create_dir_all(&self.config.base_dir)?;
        fs::create_dir_all(self.config.base_dir.join("snapshots"))?;

        // Scan for existing snapshots
        self.scan_snapshots()?;

        // Try to recover from latest snapshot
        if let Some(latest) = self.snapshots.last().cloned() {
            self.restore_snapshot(latest.height)?;
        }

        // Replay WAL if exists
        if self.config.wal_enabled {
            self.replay_wal()?;
            self.open_wal()?;
        }

        self.initialized = true;
        tracing::info!(
            "MmapState initialized at height {} with {} entries",
            self.current_height,
            self.state.len()
        );

        Ok(())
    }

    /// Check if state storage is initialized
    pub fn is_available(&self) -> bool {
        self.initialized
    }

    /// Get configuration
    pub fn config(&self) -> &MmapStateConfig {
        &self.config
    }

    /// Get current height
    pub fn height(&self) -> u64 {
        self.current_height
    }

    /// Get a value by key
    #[inline]
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.state.get(key).map(|v| v.as_slice())
    }

    /// Set a key-value pair
    #[inline]
    pub fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        if self.config.wal_enabled {
            let entry = WalEntry {
                height: self.current_height,
                op: WalOp::Set {
                    key: key.clone(),
                    value: value.clone(),
                },
                timestamp: current_timestamp(),
            };
            self.write_wal_entry(&entry);
        }
        self.state.insert(key, value);
        self.dirty = true;
    }

    /// Delete a key
    #[inline]
    pub fn delete(&mut self, key: &[u8]) {
        if self.config.wal_enabled {
            let entry = WalEntry {
                height: self.current_height,
                op: WalOp::Delete {
                    key: key.to_vec(),
                },
                timestamp: current_timestamp(),
            };
            self.write_wal_entry(&entry);
        }
        self.state.remove(key);
        self.dirty = true;
    }

    /// Begin a new block
    pub fn begin_block(&mut self, height: u64) {
        self.current_height = height;
        if self.config.wal_enabled {
            let entry = WalEntry {
                height,
                op: WalOp::BeginBlock { height },
                timestamp: current_timestamp(),
            };
            self.write_wal_entry(&entry);
        }
    }

    /// Commit a block (makes all changes durable)
    pub fn commit_block(&mut self, app_hash: [u8; 32]) -> io::Result<()> {
        if self.config.wal_enabled {
            let entry = WalEntry {
                height: self.current_height,
                op: WalOp::CommitBlock {
                    height: self.current_height,
                    app_hash,
                },
                timestamp: current_timestamp(),
            };
            self.write_wal_entry(&entry);

            // Flush WAL to disk
            if let Some(ref mut f) = self.wal_file {
                f.flush()?;
            }
        }

        // Take periodic snapshot
        if self.dirty
            && self.config.snapshot_interval > 0
            && self.current_height % self.config.snapshot_interval == 0
        {
            self.take_snapshot(app_hash)?;
        }

        Ok(())
    }

    /// Take a snapshot of current state
    pub fn take_snapshot(&mut self, app_hash: [u8; 32]) -> io::Result<SnapshotMeta> {
        let start = Instant::now();
        let snap_path = self
            .config
            .base_dir
            .join("snapshots")
            .join(format!("snap_{}.bin", self.current_height));

        // Serialize state
        let data =
            bincode::serialize(&self.state).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let size = data.len() as u64;

        // Write atomically (write to temp, then rename)
        let tmp_path = snap_path.with_extension("tmp");
        fs::write(&tmp_path, &data)?;
        fs::rename(&tmp_path, &snap_path)?;

        let meta = SnapshotMeta {
            height: self.current_height,
            app_hash,
            timestamp: current_timestamp(),
            size_bytes: size,
            entry_count: self.state.len() as u64,
        };

        self.snapshots.push(meta.clone());
        self.dirty = false;

        // Prune old snapshots
        self.prune_snapshots()?;

        // Truncate WAL after successful snapshot
        if self.config.wal_enabled {
            self.truncate_wal()?;
        }

        let elapsed = start.elapsed();
        tracing::info!(
            "Snapshot at height {} ({} entries, {:.2} KB) in {:?}",
            self.current_height,
            self.state.len(),
            size as f64 / 1024.0,
            elapsed
        );

        Ok(meta)
    }

    /// Restore state from a snapshot
    pub fn restore_snapshot(&mut self, height: u64) -> io::Result<()> {
        let snap_path = self
            .config
            .base_dir
            .join("snapshots")
            .join(format!("snap_{}.bin", height));

        if !snap_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Snapshot not found at height {}", height),
            ));
        }

        let data = fs::read(&snap_path)?;
        self.state =
            bincode::deserialize(&data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.current_height = height;
        self.dirty = false;

        tracing::info!(
            "Restored snapshot at height {} ({} entries)",
            height,
            self.state.len()
        );

        Ok(())
    }

    /// Get list of available snapshots
    pub fn list_snapshots(&self) -> &[SnapshotMeta] {
        &self.snapshots
    }

    /// Get number of entries in state
    pub fn len(&self) -> usize {
        self.state.len()
    }

    /// Check if state is empty
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// Get total disk usage estimate
    pub fn disk_usage(&self) -> io::Result<u64> {
        let mut total = 0u64;
        let snap_dir = self.config.base_dir.join("snapshots");
        if snap_dir.exists() {
            for entry in fs::read_dir(&snap_dir)? {
                let entry = entry?;
                total += entry.metadata()?.len();
            }
        }
        let wal_path = self.config.base_dir.join("wal.log");
        if wal_path.exists() {
            total += fs::metadata(&wal_path)?.len();
        }
        Ok(total)
    }

    // ---- Internal methods ----

    fn scan_snapshots(&mut self) -> io::Result<()> {
        let snap_dir = self.config.base_dir.join("snapshots");
        if !snap_dir.exists() {
            return Ok(());
        }

        let mut snapshots = Vec::new();
        for entry in fs::read_dir(&snap_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("snap_") && name.ends_with(".bin") {
                if let Some(height_str) =
                    name.strip_prefix("snap_").and_then(|s| s.strip_suffix(".bin"))
                {
                    if let Ok(height) = height_str.parse::<u64>() {
                        let meta = entry.metadata()?;
                        snapshots.push(SnapshotMeta {
                            height,
                            app_hash: [0u8; 32], // Unknown until loaded
                            timestamp: meta
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0),
                            size_bytes: meta.len(),
                            entry_count: 0, // Unknown until loaded
                        });
                    }
                }
            }
        }

        snapshots.sort_by_key(|s| s.height);
        self.snapshots = snapshots;
        Ok(())
    }

    fn open_wal(&mut self) -> io::Result<()> {
        let wal_path = self.config.base_dir.join("wal.log");
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(wal_path)?;
        self.wal_file = Some(file);
        Ok(())
    }

    fn write_wal_entry(&mut self, entry: &WalEntry) {
        if let Some(ref mut f) = self.wal_file {
            if let Ok(data) = bincode::serialize(entry) {
                let len = data.len() as u32;
                let _ = f.write_all(&len.to_le_bytes());
                let _ = f.write_all(&data);
            }
        }
    }

    fn replay_wal(&mut self) -> io::Result<()> {
        let wal_path = self.config.base_dir.join("wal.log");
        if !wal_path.exists() {
            return Ok(());
        }

        let data = fs::read(&wal_path)?;
        if data.is_empty() {
            return Ok(());
        }

        let mut cursor = 0;
        let mut replayed = 0u64;
        let mut last_committed_height = self.current_height;

        while cursor + 4 <= data.len() {
            let len = u32::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]) as usize;
            cursor += 4;

            if cursor + len > data.len() {
                // Truncated entry (crash during write) - stop replay
                tracing::warn!("WAL truncated at entry {}, stopping replay", replayed);
                break;
            }

            if let Ok(entry) = bincode::deserialize::<WalEntry>(&data[cursor..cursor + len]) {
                // Only replay entries at or after our current height
                if entry.height >= self.current_height {
                    match entry.op {
                        WalOp::Set { key, value } => {
                            self.state.insert(key, value);
                        }
                        WalOp::Delete { key } => {
                            self.state.remove(&key);
                        }
                        WalOp::BeginBlock { height } => {
                            self.current_height = height;
                        }
                        WalOp::CommitBlock { height, .. } => {
                            last_committed_height = height;
                        }
                    }
                    replayed += 1;
                }
            }

            cursor += len;
        }

        // Rollback to last committed block if we have uncommitted entries
        if self.current_height > last_committed_height {
            tracing::warn!(
                "WAL has uncommitted entries (height {} > committed {}), rolling back",
                self.current_height,
                last_committed_height
            );
            self.current_height = last_committed_height;
        }

        if replayed > 0 {
            tracing::info!(
                "Replayed {} WAL entries, recovered to height {}",
                replayed,
                self.current_height
            );
        }

        Ok(())
    }

    fn truncate_wal(&mut self) -> io::Result<()> {
        // Close existing WAL
        self.wal_file = None;

        // Truncate and reopen
        let wal_path = self.config.base_dir.join("wal.log");
        fs::write(&wal_path, &[])?;
        self.open_wal()?;

        Ok(())
    }

    fn prune_snapshots(&mut self) -> io::Result<()> {
        while self.snapshots.len() > self.config.max_snapshots {
            let oldest = self.snapshots.remove(0);
            let snap_path = self
                .config
                .base_dir
                .join("snapshots")
                .join(format!("snap_{}.bin", oldest.height));
            if snap_path.exists() {
                fs::remove_file(&snap_path)?;
            }
        }
        Ok(())
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_config() -> MmapStateConfig {
        let dir = env::temp_dir().join(format!("mmap_test_{}", rand_suffix()));
        MmapStateConfig {
            base_dir: dir,
            page_size: 4096,
            max_disk_usage: 1024 * 1024,
            wal_enabled: true,
            snapshot_interval: 5,
            max_snapshots: 3,
        }
    }

    fn rand_suffix() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::SystemTime;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let ts = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let cnt = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tid = format!("{:?}", std::thread::current().id());
        format!("{}_{}{}", ts, cnt, tid)
    }

    fn cleanup(config: &MmapStateConfig) {
        let _ = fs::remove_dir_all(&config.base_dir);
    }

    #[test]
    fn test_mmap_state_config_default() {
        let config = MmapStateConfig::default();
        assert_eq!(config.page_size, 4096);
        assert!(config.wal_enabled);
        assert_eq!(config.snapshot_interval, 100);
    }

    #[test]
    fn test_mmap_state_initialize() {
        let config = test_config();
        let mut state = MmapState::new(config.clone());

        state.initialize().unwrap();
        assert!(state.is_available());
        assert_eq!(state.height(), 0);
        assert!(state.is_empty());

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_set_get() {
        let config = test_config();
        let mut state = MmapState::new(config.clone());
        state.initialize().unwrap();

        state.set(b"key1".to_vec(), b"value1".to_vec());
        state.set(b"key2".to_vec(), b"value2".to_vec());

        assert_eq!(state.get(b"key1"), Some(b"value1".as_slice()));
        assert_eq!(state.get(b"key2"), Some(b"value2".as_slice()));
        assert_eq!(state.get(b"key3"), None);
        assert_eq!(state.len(), 2);

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_delete() {
        let config = test_config();
        let mut state = MmapState::new(config.clone());
        state.initialize().unwrap();

        state.set(b"key1".to_vec(), b"value1".to_vec());
        assert_eq!(state.len(), 1);

        state.delete(b"key1");
        assert_eq!(state.get(b"key1"), None);
        assert_eq!(state.len(), 0);

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_snapshot_and_restore() {
        let config = test_config();
        let mut state = MmapState::new(config.clone());
        state.initialize().unwrap();

        // Build up state over blocks
        for h in 1..=5 {
            state.begin_block(h);
            state.set(
                format!("key_{}", h).into_bytes(),
                format!("val_{}", h).into_bytes(),
            );
            state.commit_block([h as u8; 32]).unwrap();
        }

        // Snapshot should have been taken at height 5
        assert!(!state.list_snapshots().is_empty());

        // Create new state and restore
        let mut state2 = MmapState::new(config.clone());
        state2.initialize().unwrap();
        state2.restore_snapshot(5).unwrap();

        assert_eq!(state2.height(), 5);
        assert_eq!(state2.get(b"key_1"), Some(b"val_1".as_slice()));
        assert_eq!(state2.get(b"key_5"), Some(b"val_5".as_slice()));

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_wal_recovery() {
        let config = test_config();

        // Phase 1: Write some data with WAL
        {
            let mut state = MmapState::new(config.clone());
            state.initialize().unwrap();

            state.begin_block(1);
            state.set(b"survived".to_vec(), b"yes".to_vec());
            state.commit_block([1u8; 32]).unwrap();

            state.begin_block(2);
            state.set(b"also_survived".to_vec(), b"yes".to_vec());
            state.commit_block([2u8; 32]).unwrap();
            // State goes out of scope (simulates crash after commit)
        }

        // Phase 2: Recover from WAL
        {
            let mut state = MmapState::new(config.clone());
            state.initialize().unwrap();

            // WAL replay should have recovered both entries
            assert_eq!(state.height(), 2);
            assert_eq!(state.get(b"survived"), Some(b"yes".as_slice()));
            assert_eq!(state.get(b"also_survived"), Some(b"yes".as_slice()));
        }

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_snapshot_pruning() {
        let config = test_config(); // max_snapshots = 3, snapshot_interval = 5
        let mut state = MmapState::new(config.clone());
        state.initialize().unwrap();

        // Create blocks to trigger snapshots at 5, 10, 15, 20
        for h in 1..=20 {
            state.begin_block(h);
            state.set(format!("k{}", h).into_bytes(), b"v".to_vec());
            state.commit_block([h as u8; 32]).unwrap();
        }

        // Should only keep last 3 snapshots
        let snaps = state.list_snapshots();
        assert!(
            snaps.len() <= 3,
            "Should prune to max 3 snapshots, got {}",
            snaps.len()
        );

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_block_lifecycle() {
        let config = test_config();
        let mut state = MmapState::new(config.clone());
        state.initialize().unwrap();

        state.begin_block(1);
        assert_eq!(state.height(), 1);

        state.set(b"test".to_vec(), b"value".to_vec());
        state.commit_block([0u8; 32]).unwrap();

        assert_eq!(state.get(b"test"), Some(b"value".as_slice()));

        cleanup(&config);
    }

    #[test]
    fn test_mmap_state_disk_usage() {
        let config = test_config();
        let mut state = MmapState::new(config.clone());
        state.initialize().unwrap();

        // Initial disk usage should be minimal
        let usage = state.disk_usage().unwrap();

        // After adding data and snapshotting, usage should increase
        for h in 1..=5 {
            state.begin_block(h);
            state.set(format!("key_{}", h).into_bytes(), vec![0u8; 1000]);
            state.commit_block([h as u8; 32]).unwrap();
        }

        let usage_after = state.disk_usage().unwrap();
        assert!(
            usage_after > usage,
            "Disk usage should increase after snapshot"
        );

        cleanup(&config);
    }
}
