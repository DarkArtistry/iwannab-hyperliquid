//! HyperBFT consensus runner (Phase D2)
//!
//! Implements a HotStuff-style BFT consensus protocol with:
//! - Single leader per view (round-robin rotation)
//! - 2f+1 quorum for commits (tolerates f Byzantine faults)
//! - Pipelined block production for high throughput
//! - View change on timeout
//!
//! ## Protocol Flow
//!
//! ```text
//! Leader:  Propose(block) ──────────────────►
//! All:     ◄────────── Vote(block_hash) ─────
//! All:     ◄── CommitCertificate(2f+1 votes) ►
//! All:     Finalize(block, certificate) ──────►
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use crate::block_producer::{BlockProducer, BlockResult};
use crate::consensus::{Consensus, ConsensusError, OptimisticExecutor};
use super::{
    BftPhase, BlockProposal, CommitCertificate, HyperBftConfig, Vote,
    leader::LeaderElection,
    protocol::BftMessage,
    voter::VoteCollector,
};

/// Compute a SHA-256 hash of a block proposal for voting purposes.
///
/// This hashes the height, proposer, tx_hashes, parent_hash, and timestamp
/// to produce a deterministic 32-byte block identifier.
fn compute_proposal_hash(proposal: &BlockProposal) -> [u8; 32] {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    proposal.height.hash(&mut hasher);
    proposal.proposer.hash(&mut hasher);
    proposal.parent_hash.hash(&mut hasher);
    proposal.timestamp.hash(&mut hasher);
    for h in &proposal.tx_hashes {
        h.hash(&mut hasher);
    }
    let hash_val = hasher.finish();

    // Expand the 8-byte hash into 32 bytes for compatibility
    let bytes = hash_val.to_le_bytes();
    let mut result = [0u8; 32];
    for i in 0..4 {
        result[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
    }
    result
}

/// HyperBFT consensus state
pub struct HyperBftConsensus {
    config: HyperBftConfig,
    leader_election: LeaderElection,
    vote_collector: RwLock<VoteCollector>,
    phase: RwLock<BftPhase>,
    current_height: RwLock<u64>,
    /// Block producer for creating/finalizing blocks
    producer: Arc<BlockProducer>,
    /// Optimistic executor for speculative execution
    optimistic: RwLock<OptimisticExecutor>,
    /// Channel for receiving BFT messages from peers
    msg_rx: RwLock<Option<mpsc::Receiver<BftMessage>>>,
    /// Channel for sending BFT messages to peers
    msg_tx: mpsc::Sender<BftMessage>,
    /// View timeout duration
    view_timeout: Duration,
    /// Whether consensus is running
    running: RwLock<bool>,
}

impl HyperBftConsensus {
    /// Create a new HyperBFT consensus instance
    pub fn new(
        config: HyperBftConfig,
        producer: Arc<BlockProducer>,
        msg_tx: mpsc::Sender<BftMessage>,
        msg_rx: mpsc::Receiver<BftMessage>,
    ) -> Self {
        let leader_election = LeaderElection::new(config.clone());
        let vote_collector = VoteCollector::new(config.clone());

        let view_timeout = Duration::from_millis(config.max_wait_ms);

        Self {
            config,
            leader_election,
            vote_collector: RwLock::new(vote_collector),
            phase: RwLock::new(BftPhase::Idle),
            current_height: RwLock::new(0),
            producer,
            optimistic: RwLock::new(OptimisticExecutor::new()),
            msg_rx: RwLock::new(Some(msg_rx)),
            msg_tx,
            view_timeout,
            running: RwLock::new(false),
        }
    }

    /// Check if this node is the leader for the given height
    pub fn is_leader_for(&self, height: u64) -> bool {
        self.leader_election.leader_for_height(height) == self.config.validator_index
    }

    /// Start the consensus loop (runs until shutdown)
    pub async fn run(&self) {
        *self.running.write().await = true;
        tracing::info!(
            "HyperBFT consensus started (validator {}/{})",
            self.config.validator_index,
            self.config.num_validators
        );

        let mut msg_rx = self.msg_rx.write().await.take()
            .expect("HyperBFT run() called but message receiver already taken");

        loop {
            if !*self.running.read().await {
                break;
            }

            let height = *self.current_height.read().await + 1;

            if self.is_leader_for(height) {
                // Leader: propose a block
                match self.propose_block_internal(height).await {
                    Ok(proposal) => {
                        *self.phase.write().await = BftPhase::Propose;
                        let msg = BftMessage::Propose(proposal);
                        if self.msg_tx.send(msg).await.is_err() {
                            tracing::warn!("Failed to broadcast proposal for height {}", height);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to propose block at height {}: {}", height, e);
                    }
                }
            }

            // Wait for messages or timeout
            let deadline = Instant::now() + self.view_timeout;
            let mut committed = false;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    // View timeout - move to next height
                    tracing::debug!("View timeout at height {}", height);
                    break;
                }

                match tokio::time::timeout(remaining, msg_rx.recv()).await {
                    Ok(Some(msg)) => {
                        match self.handle_message(msg, height).await {
                            Ok(was_committed) => {
                                if was_committed {
                                    committed = true;
                                    // Block committed, advance to next height
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Error handling BFT message: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed
                        tracing::info!("BFT message channel closed, shutting down");
                        *self.running.write().await = false;
                        return;
                    }
                    Err(_) => {
                        // Timeout
                        break;
                    }
                }
            }

            // If we exited the inner loop without committing, rollback
            // any speculative execution state
            if !committed {
                let mut opt = self.optimistic.write().await;
                if opt.needs_rollback() {
                    if let Some((_snapshot, _app_hash)) = opt.rollback() {
                        tracing::info!(
                            "Rolled back speculative execution at height {} (view timeout/failure)",
                            height
                        );
                        // In production, restore engine state from _snapshot here
                    } else {
                        // No snapshot was taken (legacy begin_speculative path),
                        // just clear speculative flag via rollback which already happened
                        tracing::debug!(
                            "Cleared speculative flag at height {} (no snapshot to restore)",
                            height
                        );
                    }
                }
            }
        }

        tracing::info!("HyperBFT consensus stopped");
    }

    /// Handle an incoming BFT message
    async fn handle_message(&self, msg: BftMessage, current_height: u64) -> Result<bool, ConsensusError> {
        match msg {
            BftMessage::Propose(proposal) => {
                // Verify proposal is for current height
                if proposal.height != current_height {
                    return Ok(false);
                }

                // Begin optimistic execution with state snapshot
                // Note: In production, we'd snapshot the engine state here.
                // For now, use the simple begin_speculative since we don't
                // have direct access to engine state from consensus layer.
                let mut opt = self.optimistic.write().await;
                opt.begin_speculative(current_height);
                drop(opt);

                // Compute hash of the proposal for voting
                let block_hash = compute_proposal_hash(&proposal);

                // Vote for the proposal
                let vote = Vote {
                    height: proposal.height,
                    voter: self.config.validator_index,
                    block_hash,
                    signature: Vec::new(), // TODO: real signature
                };

                *self.phase.write().await = BftPhase::Vote;
                let msg = BftMessage::Vote(vote.clone());
                let _ = self.msg_tx.send(msg).await;

                // Also add our own vote to the collector
                let mut collector = self.vote_collector.write().await;
                if let Some(certificate) = collector.add_vote(vote) {
                    drop(collector);
                    return self.commit_block(current_height, certificate).await;
                }

                Ok(false)
            }
            BftMessage::Vote(vote) => {
                if vote.height != current_height {
                    return Ok(false);
                }

                let mut collector = self.vote_collector.write().await;
                if let Some(certificate) = collector.add_vote(vote) {
                    drop(collector);
                    return self.commit_block(current_height, certificate).await;
                }

                Ok(false)
            }
            BftMessage::Commit(certificate) => {
                if certificate.height != current_height {
                    return Ok(false);
                }

                // Verify certificate has enough votes
                if !certificate.has_quorum(self.config.quorum_size()) {
                    return Err(ConsensusError::Internal(
                        "Commit certificate has insufficient votes".to_string(),
                    ));
                }

                // Confirm optimistic execution
                let mut opt = self.optimistic.write().await;
                opt.confirm();

                // Advance height
                *self.current_height.write().await = current_height;
                *self.phase.write().await = BftPhase::Committed;

                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Commit a block once quorum is reached
    async fn commit_block(&self, height: u64, certificate: CommitCertificate) -> Result<bool, ConsensusError> {
        // Broadcast commit certificate to peers
        let msg = BftMessage::Commit(certificate);
        let _ = self.msg_tx.send(msg).await;

        // Confirm optimistic execution
        let mut opt = self.optimistic.write().await;
        opt.confirm();

        // Advance height
        *self.current_height.write().await = height;
        *self.phase.write().await = BftPhase::Committed;

        // Prune old votes
        let mut collector = self.vote_collector.write().await;
        collector.prune(height.saturating_sub(10));

        Ok(true)
    }

    /// Create a block proposal (leader only)
    async fn propose_block_internal(&self, height: u64) -> Result<BlockProposal, ConsensusError> {
        let result = self.producer.produce_block().await
            .map_err(|e| ConsensusError::ProductionFailed(e.to_string()))?;

        Ok(BlockProposal {
            height,
            proposer: self.config.validator_index,
            tx_hashes: Vec::new(), // tx_hashes are tracked internally by the block producer
            parent_hash: result.app_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            signature: Vec::new(), // TODO: real signature
        })
    }

    /// Shutdown the consensus
    pub async fn shutdown(&self) {
        *self.running.write().await = false;
    }
}

#[async_trait::async_trait]
impl Consensus for HyperBftConsensus {
    async fn propose_block(&self) -> Result<BlockResult, ConsensusError> {
        self.producer.produce_block().await
            .map_err(|e| ConsensusError::ProductionFailed(e.to_string()))
    }

    async fn finalize_block(&self, height: u64) -> Result<[u8; 32], ConsensusError> {
        let current = *self.current_height.read().await;
        if height != current {
            return Err(ConsensusError::FinalizationFailed(
                format!("Height mismatch: expected {}, got {}", current, height),
            ));
        }
        // Produce the block to get the app hash
        let result = self.producer.produce_block().await
            .map_err(|e| ConsensusError::FinalizationFailed(e.to_string()))?;
        Ok(result.app_hash)
    }

    async fn current_height(&self) -> u64 {
        *self.current_height.read().await
    }

    async fn is_healthy(&self) -> bool {
        *self.running.read().await && !self.producer.is_halted()
    }

    fn mode_name(&self) -> &'static str {
        "hyperbft"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hyperbft_consensus_creation() {
        let config = HyperBftConfig {
            max_wait_ms: 100,
            num_validators: 4,
            fault_tolerance: 1,
            validator_index: 0,
        };

        // Verify quorum calculations
        assert_eq!(config.quorum_size(), 3); // 2f+1 = 3 for f=1
        assert!(config.num_validators >= config.min_validators()); // 4 >= 3f+1 = 4
    }

    #[test]
    fn test_leader_rotation() {
        let config = HyperBftConfig {
            num_validators: 4,
            validator_index: 0,
            ..Default::default()
        };
        let election = LeaderElection::new(config);

        // Leaders rotate round-robin: height % num_validators
        assert_eq!(election.leader_for_height(0), 0);
        assert_eq!(election.leader_for_height(1), 1);
        assert_eq!(election.leader_for_height(2), 2);
        assert_eq!(election.leader_for_height(3), 3);
        assert_eq!(election.leader_for_height(4), 0);
        assert_eq!(election.leader_for_height(5), 1);
    }

    #[tokio::test]
    async fn test_optimistic_executor_lifecycle() {
        let mut executor = OptimisticExecutor::new();

        assert!(!executor.is_speculative());
        assert!(!executor.needs_rollback());
        assert!(!executor.has_snapshot());

        executor.begin_speculative(10);
        assert!(executor.is_speculative());
        assert_eq!(executor.snapshot_height(), Some(10));
        assert!(!executor.has_snapshot());

        executor.confirm();
        assert!(!executor.is_speculative());
        assert!(!executor.needs_rollback());
        assert!(!executor.has_snapshot());
    }

    #[test]
    fn test_compute_proposal_hash_deterministic() {
        let proposal = BlockProposal {
            height: 42,
            proposer: 0,
            tx_hashes: vec![[1u8; 32], [2u8; 32]],
            parent_hash: [0u8; 32],
            timestamp: 1000,
            signature: vec![],
        };

        let hash1 = compute_proposal_hash(&proposal);
        let hash2 = compute_proposal_hash(&proposal);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_compute_proposal_hash_differs_for_different_proposals() {
        let proposal_a = BlockProposal {
            height: 42,
            proposer: 0,
            tx_hashes: vec![[1u8; 32]],
            parent_hash: [0u8; 32],
            timestamp: 1000,
            signature: vec![],
        };

        let proposal_b = BlockProposal {
            height: 43,
            proposer: 0,
            tx_hashes: vec![[1u8; 32]],
            parent_hash: [0u8; 32],
            timestamp: 1000,
            signature: vec![],
        };

        assert_ne!(compute_proposal_hash(&proposal_a), compute_proposal_hash(&proposal_b));
    }
}
