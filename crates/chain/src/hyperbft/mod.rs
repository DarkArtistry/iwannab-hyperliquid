//! HyperBFT: Custom HotStuff-style BFT consensus (Phase D2)
//!
//! Key properties:
//! - Blocks proposed on-demand (when txs arrive) with 5ms max wait
//! - One round-trip in happy case (~10ms)
//! - 2f+1 quorum for commit
//! - Leader rotation per block

pub mod leader;
pub mod voter;
pub mod protocol;
pub mod runner;

/// HyperBFT configuration
#[derive(Debug, Clone)]
pub struct HyperBftConfig {
    /// Maximum time to wait for transactions before proposing empty block (ms)
    pub max_wait_ms: u64,
    /// Number of validators
    pub num_validators: usize,
    /// Fault tolerance (f in 3f+1)
    pub fault_tolerance: usize,
    /// This validator's index
    pub validator_index: usize,
}

impl Default for HyperBftConfig {
    fn default() -> Self {
        Self {
            max_wait_ms: 5,
            num_validators: 4,
            fault_tolerance: 1,
            validator_index: 0,
        }
    }
}

impl HyperBftConfig {
    /// Quorum size: 2f+1
    pub fn quorum_size(&self) -> usize {
        2 * self.fault_tolerance + 1
    }

    /// Total validators needed: 3f+1
    pub fn min_validators(&self) -> usize {
        3 * self.fault_tolerance + 1
    }
}

/// HyperBFT consensus state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BftPhase {
    /// Waiting for transactions
    Idle,
    /// Leader proposing a block
    Propose,
    /// Collecting votes
    Vote,
    /// Block committed
    Committed,
}

/// Block proposal message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockProposal {
    /// Block height
    pub height: u64,
    /// Proposer index
    pub proposer: usize,
    /// Transaction hashes in the block
    pub tx_hashes: Vec<[u8; 32]>,
    /// Parent block hash
    pub parent_hash: [u8; 32],
    /// Timestamp
    pub timestamp: u64,
    /// Proposer's signature
    pub signature: Vec<u8>,
}

/// Vote message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Vote {
    /// Block height
    pub height: u64,
    /// Voter index
    pub voter: usize,
    /// Block proposal hash being voted on
    pub block_hash: [u8; 32],
    /// Voter's signature
    pub signature: Vec<u8>,
}

/// Commit certificate (2f+1 votes)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitCertificate {
    /// Block height
    pub height: u64,
    /// Block hash
    pub block_hash: [u8; 32],
    /// Votes from 2f+1 validators
    pub votes: Vec<Vote>,
}

impl CommitCertificate {
    /// Check if certificate has enough votes
    pub fn has_quorum(&self, quorum_size: usize) -> bool {
        self.votes.len() >= quorum_size
    }
}
