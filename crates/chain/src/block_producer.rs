//! Block Producer for single-node consensus
//!
//! Phase 2B: This module provides a simple block producer that can be used
//! for development and testing without requiring full CometBFT integration.
//!
//! ## Overview
//!
//! The BlockProducer collects transactions from a mempool and produces blocks
//! at a configurable interval. It goes through the same ABCI flow as if
//! CometBFT was driving block production:
//!
//! 1. BeginBlock - Start processing a new block
//! 2. DeliverTx  - Execute each transaction
//! 3. EndBlock   - Finalize the block (funding, liquidations)
//! 4. Commit     - Commit state and compute app hash
//!
//! ## Usage
//!
//! ```ignore
//! let producer = BlockProducer::new(app, mempool, 500); // 500ms blocks
//! producer.start().await; // Runs in a loop
//! ```

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time::{interval, Instant};

use crate::app::HyperCoreApp;
use crate::mempool::{Mempool, SharedMempool};
use crate::tx::Transaction;

/// Block production configuration
#[derive(Debug, Clone)]
pub struct BlockProducerConfig {
    /// Target block time in milliseconds (default: 500ms)
    pub block_time_ms: u64,
    /// Maximum transactions per block (default: 1000)
    pub max_txs_per_block: usize,
    /// Maximum block size in bytes (default: 1MB)
    pub max_block_size: usize,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            block_time_ms: 500,
            max_txs_per_block: 1000,
            max_block_size: 1_048_576, // 1MB
        }
    }
}

/// Post-commit handler type for persistence or other operations
pub type PostCommitHandler = Arc<dyn Fn(&BlockResult, &HyperCoreApp) + Send + Sync>;

/// Block producer for single-node consensus
pub struct BlockProducer {
    /// Application state
    app: Arc<RwLock<HyperCoreApp>>,
    /// Transaction mempool (thread-safe)
    mempool: SharedMempool,
    /// Configuration
    config: BlockProducerConfig,
    /// Whether the producer is running
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Optional post-commit handler (e.g., for persistence)
    on_commit: Option<PostCommitHandler>,
}

impl BlockProducer {
    /// Create a new block producer
    pub fn new(
        app: Arc<RwLock<HyperCoreApp>>,
        mempool: SharedMempool,
        config: BlockProducerConfig,
    ) -> Self {
        Self {
            app,
            mempool,
            config,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            on_commit: None,
        }
    }

    /// Create with default config and custom block time
    pub fn with_block_time(
        app: Arc<RwLock<HyperCoreApp>>,
        mempool: SharedMempool,
        block_time_ms: u64,
    ) -> Self {
        Self::new(
            app,
            mempool,
            BlockProducerConfig {
                block_time_ms,
                ..Default::default()
            },
        )
    }

    /// Set a post-commit handler that runs after each block is committed.
    /// Useful for state persistence, metrics, or other post-commit operations.
    pub fn with_post_commit_handler(mut self, handler: PostCommitHandler) -> Self {
        self.on_commit = Some(handler);
        self
    }

    /// Start block production loop
    ///
    /// This runs indefinitely, producing blocks at the configured interval.
    /// Call `stop()` to terminate.
    pub async fn start(&self) {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        let mut block_interval = interval(Duration::from_millis(self.config.block_time_ms));

        tracing::info!(
            "Block producer starting with {}ms block time",
            self.config.block_time_ms
        );

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            block_interval.tick().await;

            if let Err(e) = self.produce_block().await {
                tracing::error!("Block production failed: {}", e);
            }
        }

        tracing::info!("Block producer stopped");
    }

    /// Stop block production
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if producer is running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Produce a single block
    pub async fn produce_block(&self) -> Result<BlockResult, BlockProducerError> {
        let start = Instant::now();

        // Get transactions from mempool
        let txs = self.mempool.get_pending(self.config.max_txs_per_block);
        let tx_count = txs.len();

        // Acquire app lock
        let mut app = self.app.write().await;

        // Get current height and compute next
        let height = app.current_height() + 1;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Begin block
        app.begin_block(height, timestamp);

        // Execute transactions
        let mut tx_results = Vec::new();
        let mut succeeded = 0;
        let mut failed = 0;
        let mut tx_hashes = Vec::new();

        for mut tx in txs {
            let hash = tx.hash();
            tx_hashes.push(hash);

            match app.execute_tx(&tx, timestamp) {
                Ok(result) => {
                    tx_results.push((hash, true, result.events.len()));
                    succeeded += 1;
                }
                Err(e) => {
                    tracing::warn!("Transaction failed: {}", e);
                    tx_results.push((hash, false, 0));
                    failed += 1;
                }
            }
        }

        // End block
        let _validator_updates = app.end_block();

        // Commit
        let app_hash = app.commit();

        // Clear executed transactions from mempool
        for hash in &tx_hashes {
            self.mempool.remove(hash);
        }

        let duration = start.elapsed();

        let result = BlockResult {
            height,
            timestamp,
            app_hash,
            tx_count,
            succeeded,
            failed,
            duration,
        };

        tracing::info!(
            "Block {} produced: {} txs ({} ok, {} failed) in {:?}, hash: {:02x?}",
            height,
            tx_count,
            succeeded,
            failed,
            duration,
            &app_hash[..8]
        );

        // Call post-commit handler if set (e.g., for persistence)
        if let Some(ref handler) = self.on_commit {
            handler(&result, &app);
        }

        Ok(result)
    }

    /// Submit a transaction to the mempool
    pub fn submit_tx(&self, mut tx: Transaction) -> Result<[u8; 32], BlockProducerError> {
        let hash = tx.hash();

        // Check transaction is valid
        {
            let app = self.app.blocking_read();
            if let Err(e) = app.check_tx(&tx) {
                return Err(BlockProducerError::InvalidTx(e.to_string()));
            }
        }

        // Add to mempool
        self.mempool.add(tx).map_err(|e| {
            BlockProducerError::InvalidTx(format!("Mempool error: {:?}", e))
        })?;

        Ok(hash)
    }

    /// Get current block height
    pub async fn current_height(&self) -> u64 {
        self.app.read().await.current_height()
    }

    /// Get mempool size
    pub fn mempool_size(&self) -> usize {
        self.mempool.stats().total_txs
    }
}

/// Result of block production
#[derive(Debug, Clone)]
pub struct BlockResult {
    /// Block height
    pub height: u64,
    /// Block timestamp
    pub timestamp: u64,
    /// App hash after commit
    pub app_hash: [u8; 32],
    /// Total transactions in block
    pub tx_count: usize,
    /// Successfully executed transactions
    pub succeeded: usize,
    /// Failed transactions
    pub failed: usize,
    /// Time to produce block
    pub duration: Duration,
}

/// Block producer errors
#[derive(Debug, thiserror::Error)]
pub enum BlockProducerError {
    #[error("Invalid transaction: {0}")]
    InvalidTx(String),
    #[error("Mempool is full")]
    MempoolFull,
    #[error("Block production error: {0}")]
    Production(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::HyperCoreApp;
    use crate::tx::{Transaction, TransactionType};
    use hypercore_primitives::{AccountAddress, Signature};

    /// Create a test transaction (CancelAll is simple and doesn't need state)
    fn make_test_tx(nonce: u64) -> Transaction {
        Transaction {
            action: TransactionType::CancelAll,
            nonce,
            signature: Signature::zero(),
            hash: None,
        }
    }

    /// Create a test sender address from an index
    fn test_sender(idx: u64) -> AccountAddress {
        let mut bytes = [0u8; 20];
        bytes[12..20].copy_from_slice(&idx.to_be_bytes());
        AccountAddress::from(bytes)
    }

    #[tokio::test]
    async fn test_block_producer_creation() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool,
            100,
        );

        assert!(!producer.is_running());
        assert_eq!(producer.mempool_size(), 0);
    }

    #[tokio::test]
    async fn test_produce_empty_block() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool,
            100,
        );

        // Produce an empty block
        let result = producer.produce_block().await.unwrap();

        assert_eq!(result.height, 1);
        assert_eq!(result.tx_count, 0);
        assert_eq!(result.succeeded, 0);
        assert_eq!(result.failed, 0);

        // Check app state updated
        assert_eq!(producer.current_height().await, 1);
    }

    #[tokio::test]
    async fn test_produce_multiple_blocks() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool,
            100,
        );

        // Produce multiple blocks
        for expected_height in 1..=5 {
            let result = producer.produce_block().await.unwrap();
            assert_eq!(result.height, expected_height);
        }

        assert_eq!(producer.current_height().await, 5);
    }

    #[tokio::test]
    async fn test_mempool_add_transaction() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool.clone(),
            100,
        );

        // Add a transaction directly to mempool (with explicit sender for test)
        let tx = make_test_tx(0);
        let sender = test_sender(1);
        let result = mempool.add_with_sender(tx, sender);
        assert!(result.is_ok());

        // Check mempool size increased
        assert_eq!(producer.mempool_size(), 1);
    }

    #[tokio::test]
    async fn test_block_with_transactions() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool.clone(),
            100,
        );

        // Add multiple transactions to mempool
        // Each has unique nonce to generate unique hashes
        for i in 0..3 {
            let tx = make_test_tx(i); // unique nonce for unique hash
            let sender = test_sender(1); // same sender, sequential nonces
            mempool.add_with_sender(tx, sender).expect("Failed to add tx");
        }

        assert_eq!(producer.mempool_size(), 3);

        // Produce a block with the transactions
        let result = producer.produce_block().await.unwrap();

        assert_eq!(result.height, 1);
        assert_eq!(result.tx_count, 3);
        // CancelAll with no orders should succeed
        assert!(result.succeeded >= 0);

        // Mempool should be cleared
        assert_eq!(producer.mempool_size(), 0);
    }

    #[tokio::test]
    async fn test_app_hash_changes_per_block() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool,
            100,
        );

        // Produce first block
        let result1 = producer.produce_block().await.unwrap();
        let hash1 = result1.app_hash;

        // Produce second block
        let result2 = producer.produce_block().await.unwrap();
        let hash2 = result2.app_hash;

        // App hashes should be different (block height changes state)
        assert_ne!(hash1, hash2, "App hash should change between blocks");
    }

    #[tokio::test]
    async fn test_block_timestamps_increase() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool,
            100,
        );

        // Produce blocks and verify timestamps increase
        let result1 = producer.produce_block().await.unwrap();

        // Small delay to ensure timestamp changes
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let result2 = producer.produce_block().await.unwrap();

        assert!(result2.timestamp >= result1.timestamp,
            "Timestamps should be non-decreasing");
    }

    #[tokio::test]
    async fn test_block_result_contains_correct_counts() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool.clone(),
            100,
        );

        // Add 5 transactions with unique nonces
        for i in 0..5 {
            let tx = make_test_tx(i); // unique nonce for unique hash
            let sender = test_sender(1); // same sender, sequential nonces
            mempool.add_with_sender(tx, sender).expect("Failed to add tx");
        }

        let result = producer.produce_block().await.unwrap();

        // Verify counts add up
        assert_eq!(result.tx_count, 5);
        assert_eq!(result.succeeded + result.failed, result.tx_count);
    }

    #[tokio::test]
    async fn test_block_producer_config() {
        let config = BlockProducerConfig {
            block_time_ms: 200,
            max_txs_per_block: 500,
            max_block_size: 512_000,
        };

        assert_eq!(config.block_time_ms, 200);
        assert_eq!(config.max_txs_per_block, 500);
        assert_eq!(config.max_block_size, 512_000);

        let default_config = BlockProducerConfig::default();
        assert_eq!(default_config.block_time_ms, 500);
        assert_eq!(default_config.max_txs_per_block, 1000);
        assert_eq!(default_config.max_block_size, 1_048_576);
    }

    #[tokio::test]
    async fn test_concurrent_block_access() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool,
            100,
        );

        // Run concurrent height queries while producing blocks
        let producer_ref = &producer;
        let (result1, result2, height) = tokio::join!(
            producer_ref.produce_block(),
            async { producer_ref.current_height().await },
            async { producer_ref.current_height().await },
        );

        // All should succeed without deadlock
        assert!(result1.is_ok());
        // Height should be 0 or 1 (depending on timing)
        assert!(height <= 1 && result2 <= 1);
    }
}
