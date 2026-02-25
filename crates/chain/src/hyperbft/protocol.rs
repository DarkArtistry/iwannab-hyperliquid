//! Wire protocol for HyperBFT messages (Phase D2)
//!
//! Uses the existing libp2p infrastructure for network transport.

use serde::{Deserialize, Serialize};
use super::{BlockProposal, Vote, CommitCertificate};

/// Protocol messages exchanged between validators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BftMessage {
    /// Block proposal from leader
    Propose(BlockProposal),
    /// Vote on a proposal
    Vote(Vote),
    /// Commit certificate (2f+1 votes collected)
    Commit(CommitCertificate),
    /// Request for missing block at height
    SyncRequest { height: u64 },
    /// Response with block data
    SyncResponse {
        height: u64,
        proposal: BlockProposal,
        certificate: CommitCertificate,
    },
}

impl BftMessage {
    /// Serialize message to bytes for network transport
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| e.to_string())
    }

    /// Deserialize message from bytes
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes).map_err(|e| e.to_string())
    }

    /// Get the height this message relates to
    pub fn height(&self) -> u64 {
        match self {
            BftMessage::Propose(p) => p.height,
            BftMessage::Vote(v) => v.height,
            BftMessage::Commit(c) => c.height,
            BftMessage::SyncRequest { height } => *height,
            BftMessage::SyncResponse { height, .. } => *height,
        }
    }
}

/// Protocol version for forward compatibility
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum message size (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1_048_576;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_roundtrip() {
        let proposal = BlockProposal {
            height: 42,
            proposer: 0,
            tx_hashes: vec![[1u8; 32], [2u8; 32]],
            parent_hash: [0u8; 32],
            timestamp: 1000,
            signature: vec![1, 2, 3],
        };

        let msg = BftMessage::Propose(proposal);
        let encoded = msg.encode().unwrap();
        let decoded = BftMessage::decode(&encoded).unwrap();

        assert_eq!(msg.height(), decoded.height());
    }

    #[test]
    fn test_vote_message() {
        let vote = Vote {
            height: 10,
            voter: 2,
            block_hash: [42u8; 32],
            signature: vec![4, 5, 6],
        };

        let msg = BftMessage::Vote(vote);
        let encoded = msg.encode().unwrap();
        let decoded = BftMessage::decode(&encoded).unwrap();
        assert_eq!(decoded.height(), 10);
    }
}
