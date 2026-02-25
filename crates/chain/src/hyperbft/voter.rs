//! Vote collection and quorum tracking (Phase D2)

use std::collections::{HashMap, HashSet};
use super::{HyperBftConfig, Vote, CommitCertificate};

/// Tracks votes for block proposals and determines quorum
pub struct VoteCollector {
    config: HyperBftConfig,
    /// Votes indexed by (height, block_hash) -> set of voter indices
    votes: HashMap<(u64, [u8; 32]), Vec<Vote>>,
    /// Track which validators have voted per height to prevent double-voting
    voted: HashMap<u64, HashSet<usize>>,
}

impl VoteCollector {
    pub fn new(config: HyperBftConfig) -> Self {
        Self {
            config,
            votes: HashMap::new(),
            voted: HashMap::new(),
        }
    }

    /// Add a vote, returns CommitCertificate if quorum is reached
    pub fn add_vote(&mut self, vote: Vote) -> Option<CommitCertificate> {
        // Prevent double voting
        let height_voted = self.voted.entry(vote.height).or_default();
        if height_voted.contains(&vote.voter) {
            tracing::warn!(
                "Double vote detected: validator {} at height {}",
                vote.voter, vote.height
            );
            return None;
        }
        height_voted.insert(vote.voter);

        // Add vote
        let key = (vote.height, vote.block_hash);
        let votes = self.votes.entry(key).or_default();
        votes.push(vote.clone());

        // Check quorum
        if votes.len() >= self.config.quorum_size() {
            Some(CommitCertificate {
                height: vote.height,
                block_hash: vote.block_hash,
                votes: votes.clone(),
            })
        } else {
            None
        }
    }

    /// Get vote count for a specific block
    pub fn vote_count(&self, height: u64, block_hash: &[u8; 32]) -> usize {
        self.votes.get(&(height, *block_hash)).map_or(0, |v| v.len())
    }

    /// Clean up old heights (keep only recent)
    pub fn prune(&mut self, min_height: u64) {
        self.votes.retain(|(h, _), _| *h >= min_height);
        self.voted.retain(|h, _| *h >= min_height);
    }

    /// Get quorum size
    pub fn quorum_size(&self) -> usize {
        self.config.quorum_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vote(height: u64, voter: usize, block_hash: [u8; 32]) -> Vote {
        Vote {
            height,
            voter,
            block_hash,
            signature: vec![],
        }
    }

    #[test]
    fn test_quorum_reached() {
        let config = HyperBftConfig {
            num_validators: 4,
            fault_tolerance: 1,
            ..Default::default()
        };
        let mut collector = VoteCollector::new(config);
        let block_hash = [1u8; 32];

        // Need 2f+1 = 3 votes
        assert!(collector.add_vote(make_vote(1, 0, block_hash)).is_none());
        assert!(collector.add_vote(make_vote(1, 1, block_hash)).is_none());

        // Third vote should trigger quorum
        let cert = collector.add_vote(make_vote(1, 2, block_hash));
        assert!(cert.is_some());
        let cert = cert.unwrap();
        assert!(cert.has_quorum(3));
        assert_eq!(cert.votes.len(), 3);
    }

    #[test]
    fn test_double_vote_rejected() {
        let config = HyperBftConfig {
            num_validators: 4,
            fault_tolerance: 1,
            ..Default::default()
        };
        let mut collector = VoteCollector::new(config);
        let block_hash = [1u8; 32];

        assert!(collector.add_vote(make_vote(1, 0, block_hash)).is_none());
        // Same validator votes again - should be ignored
        assert!(collector.add_vote(make_vote(1, 0, block_hash)).is_none());
        assert_eq!(collector.vote_count(1, &block_hash), 1);
    }

    #[test]
    fn test_prune() {
        let config = HyperBftConfig::default();
        let mut collector = VoteCollector::new(config);
        let block_hash = [1u8; 32];

        collector.add_vote(make_vote(1, 0, block_hash));
        collector.add_vote(make_vote(5, 0, block_hash));
        collector.add_vote(make_vote(10, 0, block_hash));

        collector.prune(5);
        assert_eq!(collector.vote_count(1, &block_hash), 0);
        assert_eq!(collector.vote_count(5, &block_hash), 1);
        assert_eq!(collector.vote_count(10, &block_hash), 1);
    }
}
