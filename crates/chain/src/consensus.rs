//! Consensus abstraction layer (Phase D1)
//!
//! Defines a common trait for different consensus implementations:
//! - SingleNodeConsensus: Built-in block producer for development
//! - CometBftConsensus: CometBFT ABCI for production multi-node
//! - HyperBftConsensus: Custom HotStuff-style BFT (Phase D2)

use async_trait::async_trait;
use crate::block_producer::BlockResult;

// Re-export HyperBFT consensus runner
pub use crate::hyperbft::runner::HyperBftConsensus;

/// Consensus trait abstracting block production and finalization
#[async_trait]
pub trait Consensus: Send + Sync {
    /// Propose a new block with the given transactions
    async fn propose_block(&self) -> Result<BlockResult, ConsensusError>;

    /// Finalize a block (called after consensus agreement)
    async fn finalize_block(&self, height: u64) -> Result<[u8; 32], ConsensusError>;

    /// Get the current committed block height
    async fn current_height(&self) -> u64;

    /// Check if consensus is healthy
    async fn is_healthy(&self) -> bool;

    /// Get consensus mode name
    fn mode_name(&self) -> &'static str;
}

/// Consensus errors
#[derive(Debug, thiserror::Error)]
pub enum ConsensusError {
    #[error("Block production failed: {0}")]
    ProductionFailed(String),
    #[error("Finalization failed: {0}")]
    FinalizationFailed(String),
    #[error("Consensus not ready")]
    NotReady,
    #[error("Node halted due to divergence")]
    Halted,
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Single-node consensus implementation (wraps BlockProducer)
pub struct SingleNodeConsensus {
    producer: std::sync::Arc<crate::block_producer::BlockProducer>,
}

impl SingleNodeConsensus {
    pub fn new(producer: std::sync::Arc<crate::block_producer::BlockProducer>) -> Self {
        Self { producer }
    }
}

#[async_trait]
impl Consensus for SingleNodeConsensus {
    async fn propose_block(&self) -> Result<BlockResult, ConsensusError> {
        self.producer
            .produce_block()
            .await
            .map_err(|e| ConsensusError::ProductionFailed(e.to_string()))
    }

    async fn finalize_block(&self, _height: u64) -> Result<[u8; 32], ConsensusError> {
        // In single-node mode, finalization happens in propose_block
        Ok([0u8; 32])
    }

    async fn current_height(&self) -> u64 {
        self.producer.current_height().await
    }

    async fn is_healthy(&self) -> bool {
        !self.producer.is_halted()
    }

    fn mode_name(&self) -> &'static str {
        "single-node"
    }
}

/// CometBFT consensus implementation (Phase D1)
/// This wraps the existing CometBFT ABCI integration
#[cfg(feature = "cometbft")]
pub struct CometBftConsensus {
    app: std::sync::Arc<tokio::sync::RwLock<crate::app::HyperCoreApp>>,
}

#[cfg(feature = "cometbft")]
impl CometBftConsensus {
    pub fn new(app: std::sync::Arc<tokio::sync::RwLock<crate::app::HyperCoreApp>>) -> Self {
        Self { app }
    }
}

#[cfg(feature = "cometbft")]
#[async_trait]
impl Consensus for CometBftConsensus {
    async fn propose_block(&self) -> Result<BlockResult, ConsensusError> {
        // CometBFT drives block production externally via ABCI
        Err(ConsensusError::Internal(
            "CometBFT blocks are proposed externally".to_string(),
        ))
    }

    async fn finalize_block(&self, _height: u64) -> Result<[u8; 32], ConsensusError> {
        // Finalization happens in ABCI FinalizeBlock callback
        let app = self.app.read().await;
        Ok(app.state.app_hash)
    }

    async fn current_height(&self) -> u64 {
        self.app.read().await.current_height()
    }

    async fn is_healthy(&self) -> bool {
        true
    }

    fn mode_name(&self) -> &'static str {
        "cometbft"
    }
}

/// Optimistic execution state (Phase D3)
///
/// Speculatively executes transactions while consensus votes are in-flight.
/// If consensus succeeds, the result is already computed.
/// If consensus fails, state is rolled back using snapshot/restore.
pub struct OptimisticExecutor {
    /// Snapshot of state before speculative execution
    snapshot_height: Option<u64>,
    /// Whether we're currently executing optimistically
    is_speculative: bool,
    /// Saved engine state snapshot for rollback
    engine_snapshot: Option<hypercore_engine::EngineStateSnapshot>,
    /// Saved app hash before speculation
    pre_speculative_app_hash: Option<[u8; 32]>,
}

impl OptimisticExecutor {
    pub fn new() -> Self {
        Self {
            snapshot_height: None,
            is_speculative: false,
            engine_snapshot: None,
            pre_speculative_app_hash: None,
        }
    }

    /// Begin optimistic execution: take a snapshot of current state
    pub fn begin_speculative_with_snapshot(
        &mut self,
        height: u64,
        engine_snapshot: hypercore_engine::EngineStateSnapshot,
        app_hash: [u8; 32],
    ) {
        self.snapshot_height = Some(height);
        self.is_speculative = true;
        self.engine_snapshot = Some(engine_snapshot);
        self.pre_speculative_app_hash = Some(app_hash);
        tracing::debug!("Began optimistic execution at height {} with state snapshot", height);
    }

    /// Begin optimistic execution without snapshot (legacy, for backward compat)
    pub fn begin_speculative(&mut self, height: u64) {
        self.snapshot_height = Some(height);
        self.is_speculative = true;
        tracing::debug!("Began optimistic execution at height {}", height);
    }

    /// Confirm speculative execution (consensus succeeded)
    /// Discards the snapshot since we don't need to rollback
    pub fn confirm(&mut self) {
        self.snapshot_height = None;
        self.is_speculative = false;
        self.engine_snapshot = None;
        self.pre_speculative_app_hash = None;
        tracing::debug!("Confirmed optimistic execution");
    }

    /// Rollback speculative execution (consensus failed)
    /// Returns the saved snapshot for state restoration
    pub fn rollback(&mut self) -> Option<(hypercore_engine::EngineStateSnapshot, [u8; 32])> {
        let snapshot = self.engine_snapshot.take();
        let app_hash = self.pre_speculative_app_hash.take();
        self.snapshot_height = None;
        self.is_speculative = false;
        tracing::warn!("Rolling back optimistic execution");
        match (snapshot, app_hash) {
            (Some(s), Some(h)) => Some((s, h)),
            _ => None,
        }
    }

    /// Check if rollback is needed
    pub fn needs_rollback(&self) -> bool {
        self.is_speculative
    }

    /// Get the snapshot height for rollback
    pub fn snapshot_height(&self) -> Option<u64> {
        self.snapshot_height
    }

    /// Check if currently executing optimistically
    pub fn is_speculative(&self) -> bool {
        self.is_speculative
    }

    /// Check if a state snapshot is available for rollback
    pub fn has_snapshot(&self) -> bool {
        self.engine_snapshot.is_some()
    }
}

impl Default for OptimisticExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimistic_executor_lifecycle() {
        let mut executor = OptimisticExecutor::new();

        assert!(!executor.is_speculative());
        assert!(!executor.needs_rollback());
        assert!(!executor.has_snapshot());

        // Begin without snapshot (legacy)
        executor.begin_speculative(10);
        assert!(executor.is_speculative());
        assert_eq!(executor.snapshot_height(), Some(10));
        assert!(!executor.has_snapshot());

        executor.confirm();
        assert!(!executor.is_speculative());
        assert!(!executor.needs_rollback());
    }

    #[test]
    fn test_optimistic_executor_with_snapshot() {
        use hypercore_engine::{EngineState, EngineStateSnapshot};

        let mut executor = OptimisticExecutor::new();

        // Create a minimal snapshot
        let state = EngineState::new();
        let snapshot = state.snapshot();
        let app_hash = [42u8; 32];

        executor.begin_speculative_with_snapshot(10, snapshot, app_hash);
        assert!(executor.is_speculative());
        assert!(executor.has_snapshot());
        assert_eq!(executor.snapshot_height(), Some(10));

        // Confirm discards snapshot
        executor.confirm();
        assert!(!executor.has_snapshot());
        assert!(!executor.is_speculative());
    }

    #[test]
    fn test_optimistic_executor_rollback() {
        use hypercore_engine::{EngineState, EngineStateSnapshot};

        let mut executor = OptimisticExecutor::new();

        let state = EngineState::new();
        let snapshot = state.snapshot();
        let app_hash = [42u8; 32];

        executor.begin_speculative_with_snapshot(10, snapshot, app_hash);
        assert!(executor.needs_rollback());

        // Rollback returns the snapshot
        let result = executor.rollback();
        assert!(result.is_some());
        let (_, hash) = result.unwrap();
        assert_eq!(hash, [42u8; 32]);
        assert!(!executor.is_speculative());
        assert!(!executor.has_snapshot());
    }

    #[test]
    fn test_optimistic_executor_rollback_without_snapshot() {
        let mut executor = OptimisticExecutor::new();

        executor.begin_speculative(10);

        // Rollback without snapshot returns None
        let result = executor.rollback();
        assert!(result.is_none());
        assert!(!executor.is_speculative());
    }

    #[test]
    fn test_optimistic_executor_default() {
        let executor = OptimisticExecutor::default();
        assert!(!executor.is_speculative());
        assert_eq!(executor.snapshot_height(), None);
        assert!(!executor.has_snapshot());
    }
}
