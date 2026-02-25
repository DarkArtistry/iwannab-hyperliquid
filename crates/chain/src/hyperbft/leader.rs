//! Leader election and rotation (Phase D2)

use super::HyperBftConfig;

/// Leader election using round-robin rotation
pub struct LeaderElection {
    config: HyperBftConfig,
}

impl LeaderElection {
    pub fn new(config: HyperBftConfig) -> Self {
        Self { config }
    }

    /// Get the leader for a given block height (round-robin)
    pub fn leader_for_height(&self, height: u64) -> usize {
        (height as usize) % self.config.num_validators
    }

    /// Check if this validator is the leader for a given height
    pub fn is_leader(&self, height: u64) -> bool {
        self.leader_for_height(height) == self.config.validator_index
    }

    /// Get the next leader after the current height
    pub fn next_leader(&self, height: u64) -> usize {
        self.leader_for_height(height + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_rotation() {
        let config = HyperBftConfig {
            num_validators: 4,
            validator_index: 0,
            ..Default::default()
        };
        let election = LeaderElection::new(config);

        assert_eq!(election.leader_for_height(0), 0);
        assert_eq!(election.leader_for_height(1), 1);
        assert_eq!(election.leader_for_height(2), 2);
        assert_eq!(election.leader_for_height(3), 3);
        assert_eq!(election.leader_for_height(4), 0); // Wraps around
    }

    #[test]
    fn test_is_leader() {
        let config = HyperBftConfig {
            num_validators: 4,
            validator_index: 2,
            ..Default::default()
        };
        let election = LeaderElection::new(config);

        assert!(!election.is_leader(0));
        assert!(!election.is_leader(1));
        assert!(election.is_leader(2));
        assert!(!election.is_leader(3));
        assert!(!election.is_leader(4));
        assert!(election.is_leader(6));
    }
}
