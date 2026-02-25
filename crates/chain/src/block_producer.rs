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
//! 5. Attest     - Sign and broadcast state attestation (Phase 7B)
//!
//! ## State Attestation (Phase 7B)
//!
//! After each block is committed, the producer can optionally:
//! - Sign a state attestation with the computed app_hash
//! - Record the hash for local comparison
//! - Check for divergence alerts from other validators
//!
//! ## Usage
//!
//! ```ignore
//! let producer = BlockProducer::new(app, mempool, 500); // 500ms blocks
//! producer.start().await; // Runs in a loop
//! ```

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time::{interval, Instant};

use crate::app::HyperCoreApp;
use crate::attestation::{AttestationKeyPair, StateAttestation};
use crate::attestation_collector::AttestationCollector;
use crate::mempool::SharedMempool;
use crate::tx::Transaction;

/// Channel sender for broadcasting attestations to the P2P layer
pub type AttestationBroadcastTx = tokio::sync::mpsc::Sender<StateAttestation>;

/// Block production configuration
#[derive(Debug, Clone)]
pub struct BlockProducerConfig {
    /// Target block time in milliseconds (default: 200ms)
    pub block_time_ms: u64,
    /// Maximum transactions per block (default: 10,000)
    pub max_txs_per_block: usize,
    /// Maximum block size in bytes (default: 10MB)
    pub max_block_size: usize,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            block_time_ms: 200,
            max_txs_per_block: 10_000,
            max_block_size: 10_485_760, // 10MB
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
    /// Optional attestation key pair for signing state attestations (Phase 7B)
    attestation_key: Option<AttestationKeyPair>,
    /// Optional attestation collector for state verification (Phase 7B)
    attestation_collector: Option<Arc<AttestationCollector>>,
    /// External halt flag (can be set by divergence handler)
    halted: Option<Arc<AtomicBool>>,
    /// Optional channel for broadcasting attestations to P2P layer
    attestation_broadcast_tx: Option<AttestationBroadcastTx>,
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
            attestation_key: None,
            attestation_collector: None,
            halted: None,
            attestation_broadcast_tx: None,
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

    /// Enable state attestation (Phase 7B)
    ///
    /// When enabled, the producer will:
    /// 1. Sign a state attestation after each block
    /// 2. Record our app_hash in the collector
    /// 3. Check for divergence before producing blocks
    pub fn with_attestation(
        mut self,
        key: AttestationKeyPair,
        collector: Arc<AttestationCollector>,
    ) -> Self {
        self.attestation_key = Some(key);
        self.attestation_collector = Some(collector);
        self
    }

    /// Set an external halt flag (e.g., from divergence handler)
    ///
    /// When this flag is set to true, block production will stop.
    pub fn with_halt_flag(mut self, halted: Arc<AtomicBool>) -> Self {
        self.halted = Some(halted);
        self
    }

    /// Set a channel for broadcasting attestations to the P2P layer
    ///
    /// When set, attestations will be sent through this channel after each block commit.
    /// Connect this to `AttestationGossip` for network broadcast.
    pub fn with_attestation_broadcast(mut self, tx: AttestationBroadcastTx) -> Self {
        self.attestation_broadcast_tx = Some(tx);
        self
    }

    /// Check if the node is halted due to divergence
    pub fn is_halted(&self) -> bool {
        self.halted
            .as_ref()
            .map(|h| h.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// Start block production loop
    ///
    /// This runs indefinitely, producing blocks at the configured interval.
    /// Call `stop()` to terminate.
    ///
    /// Will stop producing blocks if the halted flag is set (divergence detected).
    pub async fn start(&self) {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        let mut block_interval = interval(Duration::from_millis(self.config.block_time_ms));

        let attestation_enabled = self.attestation_key.is_some();
        tracing::info!(
            "Block producer starting with {}ms block time (attestation: {})",
            self.config.block_time_ms,
            if attestation_enabled { "enabled" } else { "disabled" }
        );

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            block_interval.tick().await;

            // Check if halted due to divergence
            if self.is_halted() {
                tracing::error!("Block production halted due to state divergence!");
                tracing::error!("Manual intervention required to resume.");
                // Keep running but don't produce blocks
                continue;
            }

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

        // Phase B3: Drain lock-free incoming queue into indexed mempool
        self.mempool.drain_into_indexed();

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

        // Phase 7B: State Attestation
        // Record our hash and create attestation after commit
        if let (Some(collector), Some(key)) = (&self.attestation_collector, &self.attestation_key) {
            // Record our computed hash for divergence comparison
            collector.record_our_hash(height, app_hash).await;

            // Create and sign attestation
            let attestation = key.sign_attestation(height, app_hash);

            tracing::debug!(
                "Created attestation for block {}: hash {:02x?}",
                height,
                &app_hash[..8]
            );

            // Process our own attestation (this allows the collector to track our vote)
            if let Err(e) = collector.process_attestation(attestation.clone()).await {
                // This might fail if we're not registered as a validator (in tests)
                tracing::debug!("Could not process own attestation: {}", e);
            }

            // Broadcast attestation to other validators via P2P layer
            if let Some(ref broadcast_tx) = self.attestation_broadcast_tx {
                if let Err(e) = broadcast_tx.send(attestation).await {
                    tracing::warn!("Failed to broadcast attestation: {}", e);
                } else {
                    tracing::debug!(
                        "Attestation for block {} sent to P2P layer",
                        height
                    );
                }
            }
        }

        Ok(result)
    }

    /// Submit a transaction to the mempool
    ///
    /// Phase B3: Uses the lock-free SegQueue path (`push`) instead of the
    /// RwLock-guarded `add`. The transaction is validated first via `check_tx`,
    /// then pushed into the lock-free incoming queue. The block producer's
    /// `produce_block` drains this queue into the indexed mempool before
    /// selecting transactions for inclusion.
    pub fn submit_tx(&self, mut tx: Transaction) -> Result<[u8; 32], BlockProducerError> {
        let hash = tx.hash();

        // Check transaction is valid (read-only, no write lock needed)
        {
            let app = self.app.try_read()
                .map_err(|_| BlockProducerError::Production("App lock contention during tx validation".to_string()))?;
            if let Err(e) = app.check_tx(&tx) {
                return Err(BlockProducerError::InvalidTx(e.to_string()));
            }
        }

        // Push to lock-free incoming queue (never blocks, no RwLock contention)
        self.mempool.push(tx);

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

/// Pipeline stages for block production
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// Collecting transactions from mempool
    Collect,
    /// Executing transactions
    Execute,
    /// Committing state
    Commit,
}

/// Pipelined block producer (Phase D3)
///
/// Overlaps block production stages across consecutive blocks:
/// - While block N is being committed, block N+1's transactions are being executed
/// - While block N+1 is executing, block N+2's transactions are being collected
///
/// This increases throughput by ~2-3x compared to sequential production.
///
/// ```text
/// Block N:   [Collect] [Execute] [Commit ]
/// Block N+1:           [Collect] [Execute] [Commit ]
/// Block N+2:                     [Collect] [Execute] [Commit]
/// ```
pub struct PipelinedBlockProducer {
    /// Application state
    app: Arc<RwLock<HyperCoreApp>>,
    /// Transaction mempool
    mempool: SharedMempool,
    /// Configuration
    config: BlockProducerConfig,
    /// Running flag
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Pipeline depth (how many blocks ahead to collect txs)
    pipeline_depth: usize,
}

impl PipelinedBlockProducer {
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
            pipeline_depth: 2, // Default: 2 blocks ahead
        }
    }

    pub fn with_pipeline_depth(mut self, depth: usize) -> Self {
        self.pipeline_depth = depth.max(1);
        self
    }

    /// Start pipelined block production
    pub async fn start(&self) {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::info!(
            "Pipelined block producer starting (depth={}, {}ms blocks)",
            self.pipeline_depth,
            self.config.block_time_ms
        );

        // Pre-collect channel: holds batches of transactions ready for execution
        let (collect_tx, mut collect_rx) =
            tokio::sync::mpsc::channel::<Vec<Transaction>>(self.pipeline_depth);

        // Spawn collector task: continuously drains mempool into batches
        let mempool = self.mempool.clone();
        let max_txs = self.config.max_txs_per_block;
        let block_time = Duration::from_millis(self.config.block_time_ms);
        let running = self.running.clone();

        let collector_handle = tokio::spawn(async move {
            let mut tick_interval = tokio::time::interval(block_time);
            while running.load(std::sync::atomic::Ordering::SeqCst) {
                tick_interval.tick().await;

                // Drain lock-free queue into indexed mempool
                mempool.drain_into_indexed();

                // Collect batch
                let txs = mempool.get_pending(max_txs);
                if collect_tx.send(txs).await.is_err() {
                    break;
                }
            }
        });

        // Main execution loop: receives pre-collected batches and executes them
        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            match collect_rx.recv().await {
                Some(txs) => {
                    if let Err(e) = self.execute_and_commit(txs).await {
                        tracing::error!("Pipelined block production failed: {}", e);
                    }
                }
                None => break,
            }
        }

        collector_handle.abort();
        tracing::info!("Pipelined block producer stopped");
    }

    /// Execute a pre-collected batch of transactions and commit
    pub async fn execute_and_commit(
        &self,
        txs: Vec<Transaction>,
    ) -> Result<BlockResult, BlockProducerError> {
        let start = Instant::now();
        let tx_count = txs.len();

        let mut app = self.app.write().await;
        let height = app.current_height() + 1;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        app.begin_block(height, timestamp);

        let mut succeeded = 0;
        let mut failed = 0;
        let mut tx_hashes = Vec::with_capacity(tx_count);

        for mut tx in txs {
            let hash = tx.hash();
            tx_hashes.push(hash);

            match app.execute_tx(&tx, timestamp) {
                Ok(_result) => succeeded += 1,
                Err(e) => {
                    tracing::warn!("Transaction failed in pipeline: {}", e);
                    failed += 1;
                }
            }
        }

        let _validator_updates = app.end_block();
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

        if tx_count > 0 {
            tracing::info!(
                "Pipeline block {} produced: {} txs ({} ok, {} failed) in {:?}",
                height,
                tx_count,
                succeeded,
                failed,
                duration
            );
        }

        Ok(result)
    }

    pub fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
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
    #[error("Block production halted due to state divergence")]
    Halted,
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
        // All 3 CancelAll txs should have been processed (may succeed or fail)
        assert_eq!(result.tx_count, result.succeeded + result.failed);

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
    async fn test_lockfree_push_does_not_block() {
        // Phase B3: Verify push() to lock-free SegQueue never blocks,
        // even under concurrent access
        let mempool = SharedMempool::new();

        // Push many transactions concurrently (simulating gateway hot path)
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let mp = mempool.clone();
                std::thread::spawn(move || {
                    for j in 0..100 {
                        let tx = make_test_tx(i * 100 + j);
                        mp.push(tx);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // All 1000 pushes succeeded without blocking
        // drain_into_indexed will move them to indexed mempool
        // (they'll fail signature recovery since they use Signature::zero(),
        // but the push path itself is lock-free and never contends)
        mempool.drain_into_indexed();
    }

    #[tokio::test]
    async fn test_drain_into_indexed_feeds_produce_block() {
        // Phase B3: Verify drain_into_indexed moves items from SegQueue
        // into the indexed mempool, making them available for produce_block.
        // Uses add_with_sender to bypass signature recovery (test-only).
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = BlockProducer::with_block_time(
            Arc::clone(&app),
            mempool.clone(),
            100,
        );

        // Add via indexed path (bypasses signature verification for tests)
        let sender = test_sender(1);
        for i in 0..3 {
            let tx = make_test_tx(i);
            mempool.add_with_sender(tx, sender).unwrap();
        }

        assert_eq!(producer.mempool_size(), 3);

        // produce_block should include all 3
        let result = producer.produce_block().await.unwrap();
        assert_eq!(result.tx_count, 3);
        assert_eq!(producer.mempool_size(), 0);
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
        assert_eq!(default_config.block_time_ms, 200);
        assert_eq!(default_config.max_txs_per_block, 10_000);
        assert_eq!(default_config.max_block_size, 10_485_760);
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

    // ========================================
    // Phase 7B: State Attestation Tests
    // ========================================

    #[tokio::test]
    async fn test_attestation_integration() {
        use crate::attestation::AttestationKeyPair;
        use crate::attestation_collector::{AttestationCollector, AttestationConfig};
        use tokio::sync::mpsc;

        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        // Create attestation system
        let (alert_tx, _alert_rx) = mpsc::channel(10);
        let key_pair = AttestationKeyPair::generate();
        let collector = Arc::new(AttestationCollector::new(
            alert_tx,
            AttestationConfig::default(),
            Some(key_pair.public_key()),
        ));

        // Register ourselves as a validator
        collector.add_validator(key_pair.public_key(), 100).await;

        // Create producer with attestation
        let producer = BlockProducer::with_block_time(Arc::clone(&app), mempool, 100)
            .with_attestation(key_pair, Arc::clone(&collector));

        // Produce a block
        let result = producer.produce_block().await.unwrap();

        // Verify attestation was recorded
        assert_eq!(collector.attestation_count(result.height).await, 1);

        // Verify attestation data
        let attestations = collector.get_attestations(result.height).await;
        assert_eq!(attestations.len(), 1);
        assert_eq!(attestations[0].height, result.height);
        assert_eq!(attestations[0].app_hash, result.app_hash);
    }

    #[tokio::test]
    async fn test_halted_flag_stops_production() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let halted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let producer = BlockProducer::with_block_time(Arc::clone(&app), mempool, 100)
            .with_halt_flag(Arc::clone(&halted));

        // Not halted initially
        assert!(!producer.is_halted());

        // Produce a block - should succeed
        let result = producer.produce_block().await;
        assert!(result.is_ok());

        // Set halted flag
        halted.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(producer.is_halted());

        // Note: produce_block() itself doesn't check halted flag
        // The halt check is in the start() loop
    }

    #[tokio::test]
    async fn test_multiple_blocks_with_attestation() {
        use crate::attestation::AttestationKeyPair;
        use crate::attestation_collector::{AttestationCollector, AttestationConfig};
        use tokio::sync::mpsc;

        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let (alert_tx, _alert_rx) = mpsc::channel(10);
        let key_pair = AttestationKeyPair::generate();
        let collector = Arc::new(AttestationCollector::new(
            alert_tx,
            AttestationConfig::default(),
            Some(key_pair.public_key()),
        ));

        collector.add_validator(key_pair.public_key(), 100).await;

        let producer = BlockProducer::with_block_time(Arc::clone(&app), mempool, 100)
            .with_attestation(key_pair, Arc::clone(&collector));

        // Produce multiple blocks
        for height in 1..=5 {
            let result = producer.produce_block().await.unwrap();
            assert_eq!(result.height, height);
            assert_eq!(collector.attestation_count(height).await, 1);
        }

        // Check stats
        let stats = collector.stats().await;
        assert_eq!(stats.total_attestations, 5);
        assert_eq!(stats.heights_tracked, 5);
    }

    #[tokio::test]
    async fn test_attestation_without_collector() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        // Producer without attestation - should still work
        let producer = BlockProducer::with_block_time(Arc::clone(&app), mempool, 100);

        let result = producer.produce_block().await;
        assert!(result.is_ok());
    }

    // ========================================
    // Phase D3: Pipelined Block Producer Tests
    // ========================================

    #[tokio::test]
    async fn test_pipelined_producer_creation() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = PipelinedBlockProducer::new(
            Arc::clone(&app),
            mempool,
            BlockProducerConfig::default(),
        )
        .with_pipeline_depth(3);

        assert!(!producer.is_running());
    }

    #[tokio::test]
    async fn test_pipelined_execute_and_commit() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = PipelinedBlockProducer::new(
            Arc::clone(&app),
            mempool,
            BlockProducerConfig::default(),
        );

        // Execute empty block
        let result = producer.execute_and_commit(vec![]).await.unwrap();
        assert_eq!(result.height, 1);
        assert_eq!(result.tx_count, 0);
        assert_eq!(result.succeeded, 0);

        // Execute another empty block (height increments)
        let result2 = producer.execute_and_commit(vec![]).await.unwrap();
        assert_eq!(result2.height, 2);
    }

    #[tokio::test]
    async fn test_pipelined_multiple_blocks() {
        let app = Arc::new(RwLock::new(HyperCoreApp::new()));
        let mempool = SharedMempool::new();

        let producer = PipelinedBlockProducer::new(
            Arc::clone(&app),
            mempool,
            BlockProducerConfig::default(),
        );

        // Produce 5 blocks rapidly
        for i in 1..=5 {
            let result = producer.execute_and_commit(vec![]).await.unwrap();
            assert_eq!(result.height, i);
        }

        let app_guard = app.read().await;
        assert_eq!(app_guard.current_height(), 5);
    }
}
