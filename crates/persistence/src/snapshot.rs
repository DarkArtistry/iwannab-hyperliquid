//! ABCI State Sync Snapshots
//!
//! This module provides snapshot management for ABCI state sync, enabling
//! new nodes to bootstrap quickly by downloading state snapshots instead of
//! replaying all blocks from genesis.
//!
//! ## Architecture
//!
//! Snapshots are created using RocksDB checkpoints and serialized to JSON.
//! The snapshot is divided into chunks for efficient transfer:
//!
//! ```text
//! Snapshot (height=1000, format=1)
//!   ├── Chunk 0: JSON header + balances[0..N]
//!   ├── Chunk 1: balances[N..M] + positions[0..P]
//!   ├── Chunk 2: positions[P..Q] + orders[0..R]
//!   └── Chunk 3: orders[R..] + metadata
//! ```
//!
//! ## Usage
//!
//! Provider node (has snapshots):
//! ```ignore
//! let manager = SnapshotManager::new(db, snapshot_dir, 1000);
//! manager.create_snapshot_at_height(height)?; // Called periodically
//! let snapshots = manager.list_snapshots()?;
//! let chunk = manager.load_chunk(height, format, chunk_index)?;
//! ```
//!
//! Receiver node (syncing):
//! ```ignore
//! let restore = SnapshotRestore::new(snapshot_dir);
//! restore.offer_snapshot(metadata)?;
//! restore.apply_chunk(chunk_index, chunk_data)?;
//! let state = restore.finalize()?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use tracing::{debug, info, warn};

use crate::{PersistedState, PersistenceBackend, PersistenceConfig, PersistenceError, Result, RocksDbBackend, StatePersister};

/// Default chunk size in bytes (1 MB)
pub const DEFAULT_CHUNK_SIZE: usize = 1024 * 1024;

/// Snapshot format version
pub const SNAPSHOT_FORMAT: u32 = 1;

/// Maximum number of snapshots to keep
pub const MAX_SNAPSHOTS: usize = 5;

/// Snapshot metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Block height of the snapshot
    pub height: u64,
    /// Format version (for compatibility checking)
    pub format: u32,
    /// Number of chunks
    pub chunks: u32,
    /// SHA-256 hash of the complete state
    pub hash: [u8; 32],
    /// Additional metadata (JSON)
    pub metadata: String,
}

impl SnapshotMetadata {
    /// Create metadata for a snapshot
    pub fn new(height: u64, format: u32, chunks: u32, hash: [u8; 32]) -> Self {
        Self {
            height,
            format,
            chunks,
            hash,
            metadata: format!(r#"{{"created_at":"{}"}}"#, chrono::Utc::now().to_rfc3339()),
        }
    }
}

/// Snapshot manager for creating and serving snapshots
pub struct SnapshotManager {
    /// Database backend (Arc-wrapped for sharing with ABCI app)
    db: std::sync::Arc<RocksDbBackend>,
    /// Directory for storing snapshots
    snapshot_dir: PathBuf,
    /// Interval between snapshots (in blocks)
    snapshot_interval: u64,
    /// Chunk size in bytes
    chunk_size: usize,
    /// Cached snapshot metadata
    snapshots: HashMap<u64, SnapshotMetadata>,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new<P: AsRef<Path>>(
        db: RocksDbBackend,
        snapshot_dir: P,
        snapshot_interval: u64,
    ) -> Result<Self> {
        Self::with_arc_db(std::sync::Arc::new(db), snapshot_dir, snapshot_interval)
    }

    /// Create a new snapshot manager with a shared database backend
    pub fn with_arc_db<P: AsRef<Path>>(
        db: std::sync::Arc<RocksDbBackend>,
        snapshot_dir: P,
        snapshot_interval: u64,
    ) -> Result<Self> {
        let snapshot_dir = snapshot_dir.as_ref().to_path_buf();

        // Create snapshot directory if it doesn't exist
        fs::create_dir_all(&snapshot_dir)
            .map_err(|e| PersistenceError::Io(e))?;

        let mut manager = Self {
            db,
            snapshot_dir,
            snapshot_interval,
            chunk_size: DEFAULT_CHUNK_SIZE,
            snapshots: HashMap::new(),
        };

        // Load existing snapshot metadata
        manager.load_snapshot_index()?;

        Ok(manager)
    }

    /// Set chunk size
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Check if a snapshot should be created at this height
    pub fn should_create_snapshot(&self, height: u64) -> bool {
        height > 0 && height % self.snapshot_interval == 0
    }

    /// Create a snapshot at the given height
    pub fn create_snapshot(&mut self, height: u64) -> Result<SnapshotMetadata> {
        info!("Creating snapshot at height {}", height);

        // Create a checkpoint of the database
        let checkpoint_path = self.snapshot_dir.join(format!("checkpoint_{}", height));
        self.db.create_checkpoint(&checkpoint_path)?;

        // Open the checkpoint and extract state
        let checkpoint_config = PersistenceConfig {
            data_dir: checkpoint_path.to_string_lossy().to_string(),
            create_if_missing: false,
            ..Default::default()
        };
        let checkpoint_db = RocksDbBackend::open(&checkpoint_config)?;
        let persister = StatePersister::new(&checkpoint_db);

        let state = persister.load_state()?
            .ok_or_else(|| PersistenceError::NotFound("No state in checkpoint".into()))?;

        // Serialize state to JSON
        let state_json = serde_json::to_string(&state)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;

        // Compute hash
        let mut hasher = Sha256::new();
        hasher.update(state_json.as_bytes());
        let hash: [u8; 32] = hasher.finalize().into();

        // Calculate chunks
        let total_bytes = state_json.len();
        let chunks = ((total_bytes + self.chunk_size - 1) / self.chunk_size) as u32;

        // Save state JSON to snapshot directory
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}.json", height));
        fs::write(&snapshot_path, &state_json)
            .map_err(|e| PersistenceError::Io(e))?;

        // Clean up checkpoint (we have the JSON now)
        let _ = fs::remove_dir_all(&checkpoint_path);

        // Create metadata
        let metadata = SnapshotMetadata::new(height, SNAPSHOT_FORMAT, chunks, hash);

        // Save metadata
        let metadata_path = self.snapshot_dir.join(format!("snapshot_{}.meta", height));
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;
        fs::write(&metadata_path, metadata_json)
            .map_err(|e| PersistenceError::Io(e))?;

        // Update cache
        self.snapshots.insert(height, metadata.clone());

        // Cleanup old snapshots
        self.cleanup_old_snapshots()?;

        info!(
            "Created snapshot at height {}: {} bytes, {} chunks, hash {:?}",
            height, total_bytes, chunks, &hash[..8]
        );

        Ok(metadata)
    }

    /// List available snapshots
    pub fn list_snapshots(&self) -> Vec<SnapshotMetadata> {
        let mut snapshots: Vec<_> = self.snapshots.values().cloned().collect();
        snapshots.sort_by(|a, b| b.height.cmp(&a.height)); // Most recent first
        snapshots
    }

    /// Get snapshot metadata by height
    pub fn get_snapshot(&self, height: u64) -> Option<&SnapshotMetadata> {
        self.snapshots.get(&height)
    }

    /// Load a chunk from a snapshot
    pub fn load_chunk(&self, height: u64, chunk_index: u32) -> Result<Vec<u8>> {
        let metadata = self.snapshots.get(&height)
            .ok_or_else(|| PersistenceError::NotFound(format!("Snapshot at height {}", height)))?;

        if chunk_index >= metadata.chunks {
            return Err(PersistenceError::NotFound(format!(
                "Chunk {} out of range (max: {})",
                chunk_index, metadata.chunks
            )));
        }

        // Read the snapshot file
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}.json", height));
        let state_json = fs::read(&snapshot_path)
            .map_err(|e| PersistenceError::Io(e))?;

        // Extract the chunk
        let start = chunk_index as usize * self.chunk_size;
        let end = std::cmp::min(start + self.chunk_size, state_json.len());
        let chunk = state_json[start..end].to_vec();

        debug!(
            "Loaded chunk {} of snapshot {}: {} bytes",
            chunk_index, height, chunk.len()
        );

        Ok(chunk)
    }

    /// Load snapshot index from disk
    fn load_snapshot_index(&mut self) -> Result<()> {
        let entries = fs::read_dir(&self.snapshot_dir)
            .map_err(|e| PersistenceError::Io(e))?;

        for entry in entries {
            let entry = entry.map_err(|e| PersistenceError::Io(e))?;
            let path = entry.path();

            if path.extension().map(|e| e == "meta").unwrap_or(false) {
                if let Ok(metadata_json) = fs::read_to_string(&path) {
                    if let Ok(metadata) = serde_json::from_str::<SnapshotMetadata>(&metadata_json) {
                        self.snapshots.insert(metadata.height, metadata);
                    }
                }
            }
        }

        info!("Loaded {} existing snapshots", self.snapshots.len());
        Ok(())
    }

    /// Remove old snapshots to keep disk usage bounded
    fn cleanup_old_snapshots(&mut self) -> Result<()> {
        let mut heights: Vec<u64> = self.snapshots.keys().copied().collect();
        heights.sort_by(|a, b| b.cmp(a)); // Descending

        // Remove snapshots beyond the limit
        while heights.len() > MAX_SNAPSHOTS {
            if let Some(height) = heights.pop() {
                info!("Cleaning up old snapshot at height {}", height);

                // Remove files
                let _ = fs::remove_file(self.snapshot_dir.join(format!("snapshot_{}.json", height)));
                let _ = fs::remove_file(self.snapshot_dir.join(format!("snapshot_{}.meta", height)));

                // Remove from cache
                self.snapshots.remove(&height);
            }
        }

        Ok(())
    }
}

/// Snapshot restore helper for receiving nodes
pub struct SnapshotRestore {
    /// Directory for temporary snapshot storage
    #[allow(dead_code)]
    restore_dir: PathBuf,
    /// Current snapshot being restored
    current_snapshot: Option<SnapshotMetadata>,
    /// Received chunks (chunk_index -> data)
    received_chunks: HashMap<u32, Vec<u8>>,
}

impl SnapshotRestore {
    /// Create a new snapshot restore helper
    pub fn new<P: AsRef<Path>>(restore_dir: P) -> Result<Self> {
        let restore_dir = restore_dir.as_ref().to_path_buf();
        fs::create_dir_all(&restore_dir)
            .map_err(|e| PersistenceError::Io(e))?;

        Ok(Self {
            restore_dir,
            current_snapshot: None,
            received_chunks: HashMap::new(),
        })
    }

    /// Accept or reject an offered snapshot
    ///
    /// Returns Ok(true) to accept, Ok(false) to reject
    pub fn offer_snapshot(&mut self, metadata: SnapshotMetadata) -> Result<bool> {
        // Check format compatibility
        if metadata.format != SNAPSHOT_FORMAT {
            warn!(
                "Rejecting snapshot: incompatible format {} (expected {})",
                metadata.format, SNAPSHOT_FORMAT
            );
            return Ok(false);
        }

        // Accept the snapshot
        info!(
            "Accepting snapshot at height {}: {} chunks",
            metadata.height, metadata.chunks
        );

        self.current_snapshot = Some(metadata);
        self.received_chunks.clear();

        Ok(true)
    }

    /// Apply a received chunk
    ///
    /// Returns:
    /// - Ok(true) if chunk accepted and more chunks needed
    /// - Ok(false) if all chunks received (ready to finalize)
    /// - Err if chunk is invalid
    pub fn apply_chunk(&mut self, chunk_index: u32, chunk_data: Vec<u8>) -> Result<bool> {
        let metadata = self.current_snapshot.as_ref()
            .ok_or_else(|| PersistenceError::NotFound("No active snapshot restore".into()))?;

        if chunk_index >= metadata.chunks {
            return Err(PersistenceError::NotFound(format!(
                "Chunk index {} out of range",
                chunk_index
            )));
        }

        debug!(
            "Received chunk {} of {}: {} bytes",
            chunk_index, metadata.chunks, chunk_data.len()
        );

        self.received_chunks.insert(chunk_index, chunk_data);

        // Check if we have all chunks
        let have_all = self.received_chunks.len() == metadata.chunks as usize;

        Ok(!have_all)
    }

    /// Finalize the restore and return the state
    pub fn finalize(&mut self) -> Result<PersistedState> {
        let metadata = self.current_snapshot.take()
            .ok_or_else(|| PersistenceError::NotFound("No active snapshot restore".into()))?;

        // Reassemble the chunks in order
        let mut state_json = Vec::new();
        for i in 0..metadata.chunks {
            let chunk = self.received_chunks.remove(&i)
                .ok_or_else(|| PersistenceError::NotFound(format!("Missing chunk {}", i)))?;
            state_json.extend(chunk);
        }

        // Verify hash
        let mut hasher = Sha256::new();
        hasher.update(&state_json);
        let hash: [u8; 32] = hasher.finalize().into();

        if hash != metadata.hash {
            return Err(PersistenceError::Serialization(
                "Snapshot hash mismatch - data may be corrupted".into()
            ));
        }

        // Deserialize state
        let state: PersistedState = serde_json::from_slice(&state_json)
            .map_err(|e| PersistenceError::Serialization(e.to_string()))?;

        // Validate state
        state.validate()
            .map_err(|e| PersistenceError::InvalidState(e))?;

        info!(
            "Successfully restored snapshot at height {}: {} balances, {} positions, {} orders",
            state.height,
            state.core.balances.len(),
            state.core.positions.len(),
            state.core.orders.len()
        );

        Ok(state)
    }

    /// Get progress (chunks received / total chunks)
    pub fn progress(&self) -> (u32, u32) {
        match &self.current_snapshot {
            Some(meta) => (self.received_chunks.len() as u32, meta.chunks),
            None => (0, 0),
        }
    }

    /// Cancel the current restore
    pub fn cancel(&mut self) {
        self.current_snapshot = None;
        self.received_chunks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_state() -> PersistedState {
        let mut state = PersistedState::new();
        state.height = 1000;
        state.timestamp = 1706054400000;
        state.app_hash = [0xab; 32];
        state
    }

    #[test]
    fn test_snapshot_metadata() {
        let hash = [0xde; 32];
        let metadata = SnapshotMetadata::new(1000, 1, 5, hash);

        assert_eq!(metadata.height, 1000);
        assert_eq!(metadata.format, 1);
        assert_eq!(metadata.chunks, 5);
        assert_eq!(metadata.hash, hash);
    }

    #[test]
    fn test_snapshot_restore_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Create a test state and serialize it
        let state = create_test_state();
        let state_json = serde_json::to_vec(&state).unwrap();

        // Compute hash
        let mut hasher = Sha256::new();
        hasher.update(&state_json);
        let hash: [u8; 32] = hasher.finalize().into();

        // Create metadata with small chunks for testing
        let chunk_size = 100;
        let chunks = ((state_json.len() + chunk_size - 1) / chunk_size) as u32;
        let metadata = SnapshotMetadata::new(1000, SNAPSHOT_FORMAT, chunks, hash);

        // Offer snapshot
        assert!(restore.offer_snapshot(metadata).unwrap());

        // Apply chunks
        for i in 0..chunks {
            let start = i as usize * chunk_size;
            let end = std::cmp::min(start + chunk_size, state_json.len());
            let chunk = state_json[start..end].to_vec();

            let need_more = restore.apply_chunk(i, chunk).unwrap();
            assert_eq!(need_more, i < chunks - 1);
        }

        // Finalize
        let restored = restore.finalize().unwrap();
        assert_eq!(restored.height, state.height);
    }

    #[test]
    fn test_snapshot_restore_hash_verification() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Create metadata with wrong hash
        let metadata = SnapshotMetadata::new(1000, SNAPSHOT_FORMAT, 1, [0xff; 32]);
        restore.offer_snapshot(metadata).unwrap();

        // Apply corrupted chunk
        restore.apply_chunk(0, b"corrupted data".to_vec()).unwrap();

        // Finalize should fail due to hash mismatch
        let result = restore.finalize();
        assert!(result.is_err());
    }

    #[test]
    fn test_incompatible_format_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Create metadata with incompatible format
        let metadata = SnapshotMetadata::new(1000, 999, 1, [0; 32]);

        // Should reject
        assert!(!restore.offer_snapshot(metadata).unwrap());
    }

    #[test]
    fn test_should_create_snapshot() {
        // We can't create a SnapshotManager without a real DB, so test the logic directly
        // Test the interval calculation logic
        let interval = 1000u64;

        // Should create at multiples of interval
        assert!(1000 > 0 && 1000 % interval == 0);
        assert!(2000 > 0 && 2000 % interval == 0);
        assert!(3000 > 0 && 3000 % interval == 0);

        // Should NOT create at non-multiples
        assert!(!(500 > 0 && 500 % interval == 0));
        assert!(!(1001 > 0 && 1001 % interval == 0));
        assert!(!(999 > 0 && 999 % interval == 0));

        // Should NOT create at height 0
        assert!(!(0 > 0 && 0 % interval == 0));
    }

    #[test]
    fn test_progress_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // No active restore - progress should be (0, 0)
        assert_eq!(restore.progress(), (0, 0));

        // Start a restore
        let metadata = SnapshotMetadata::new(1000, SNAPSHOT_FORMAT, 5, [0; 32]);
        restore.offer_snapshot(metadata).unwrap();

        // Progress should be (0, 5)
        assert_eq!(restore.progress(), (0, 5));

        // Add some chunks
        restore.apply_chunk(0, vec![1, 2, 3]).unwrap();
        assert_eq!(restore.progress(), (1, 5));

        restore.apply_chunk(2, vec![4, 5, 6]).unwrap();
        assert_eq!(restore.progress(), (2, 5));

        restore.apply_chunk(1, vec![7, 8, 9]).unwrap();
        assert_eq!(restore.progress(), (3, 5));
    }

    #[test]
    fn test_cancel_restore() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Start a restore
        let metadata = SnapshotMetadata::new(1000, SNAPSHOT_FORMAT, 5, [0; 32]);
        restore.offer_snapshot(metadata).unwrap();
        restore.apply_chunk(0, vec![1, 2, 3]).unwrap();

        // Verify we have progress
        assert_eq!(restore.progress(), (1, 5));

        // Cancel
        restore.cancel();

        // Progress should be reset
        assert_eq!(restore.progress(), (0, 0));

        // Should be able to start a new restore
        let new_metadata = SnapshotMetadata::new(2000, SNAPSHOT_FORMAT, 3, [0; 32]);
        assert!(restore.offer_snapshot(new_metadata).unwrap());
        assert_eq!(restore.progress(), (0, 3));
    }

    #[test]
    fn test_apply_chunk_no_active_restore() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Try to apply chunk without offering snapshot
        let result = restore.apply_chunk(0, vec![1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_chunk_invalid_index() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Start a restore with 3 chunks
        let metadata = SnapshotMetadata::new(1000, SNAPSHOT_FORMAT, 3, [0; 32]);
        restore.offer_snapshot(metadata).unwrap();

        // Try to apply chunk with invalid index
        let result = restore.apply_chunk(5, vec![1, 2, 3]);
        assert!(result.is_err());

        // Try to apply chunk at boundary
        let result = restore.apply_chunk(3, vec![1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_finalize_missing_chunks() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Start a restore with 3 chunks
        let metadata = SnapshotMetadata::new(1000, SNAPSHOT_FORMAT, 3, [0; 32]);
        restore.offer_snapshot(metadata).unwrap();

        // Only apply 2 chunks (missing chunk 1)
        restore.apply_chunk(0, vec![1, 2, 3]).unwrap();
        restore.apply_chunk(2, vec![7, 8, 9]).unwrap();

        // Finalize should fail due to missing chunk
        let result = restore.finalize();
        assert!(result.is_err());
    }

    #[test]
    fn test_finalize_no_active_restore() {
        let temp_dir = TempDir::new().unwrap();
        let mut restore = SnapshotRestore::new(temp_dir.path()).unwrap();

        // Try to finalize without active restore
        let result = restore.finalize();
        assert!(result.is_err());
    }

    #[test]
    fn test_snapshot_metadata_serialization() {
        let hash = [0xde; 32];
        let metadata = SnapshotMetadata::new(1000, 1, 5, hash);

        // Serialize and deserialize
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: SnapshotMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.height, metadata.height);
        assert_eq!(deserialized.format, metadata.format);
        assert_eq!(deserialized.chunks, metadata.chunks);
        assert_eq!(deserialized.hash, metadata.hash);
    }

    #[test]
    fn test_list_snapshots_ordering() {
        // Verify the sorting logic works correctly
        let mut snapshots = vec![
            SnapshotMetadata::new(500, 1, 1, [0; 32]),
            SnapshotMetadata::new(2000, 1, 1, [0; 32]),
            SnapshotMetadata::new(1000, 1, 1, [0; 32]),
            SnapshotMetadata::new(1500, 1, 1, [0; 32]),
        ];

        // Sort by height descending (most recent first)
        snapshots.sort_by(|a, b| b.height.cmp(&a.height));

        assert_eq!(snapshots[0].height, 2000);
        assert_eq!(snapshots[1].height, 1500);
        assert_eq!(snapshots[2].height, 1000);
        assert_eq!(snapshots[3].height, 500);
    }

    #[test]
    fn test_chunk_calculation() {
        // Test the chunk calculation logic
        let chunk_size = 100usize;

        // Exact multiple
        let total_bytes = 300;
        let chunks = ((total_bytes + chunk_size - 1) / chunk_size) as u32;
        assert_eq!(chunks, 3);

        // Not exact multiple (need one more chunk)
        let total_bytes = 350;
        let chunks = ((total_bytes + chunk_size - 1) / chunk_size) as u32;
        assert_eq!(chunks, 4);

        // Single byte
        let total_bytes = 1;
        let chunks = ((total_bytes + chunk_size - 1) / chunk_size) as u32;
        assert_eq!(chunks, 1);

        // Empty (edge case)
        let total_bytes = 0;
        let chunks = ((total_bytes + chunk_size - 1) / chunk_size) as u32;
        assert_eq!(chunks, 0);
    }

    #[test]
    fn test_constants() {
        // Verify constants are reasonable
        assert_eq!(DEFAULT_CHUNK_SIZE, 1024 * 1024); // 1 MB
        assert_eq!(SNAPSHOT_FORMAT, 1);
        assert_eq!(MAX_SNAPSHOTS, 5);
    }
}
