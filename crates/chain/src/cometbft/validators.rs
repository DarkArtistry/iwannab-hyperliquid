//! Validator Set Management
//!
//! This module handles validator set tracking for CometBFT consensus.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// A validator in the set
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validator {
    /// Public key (Ed25519 or Secp256k1)
    pub pub_key: Vec<u8>,
    /// Voting power
    pub power: i64,
    /// Optional name/moniker
    pub name: Option<String>,
}

impl Validator {
    /// Create a new validator
    pub fn new(pub_key: Vec<u8>, power: i64) -> Self {
        Self {
            pub_key,
            power,
            name: None,
        }
    }

    /// Create a validator with a name
    pub fn with_name(pub_key: Vec<u8>, power: i64, name: String) -> Self {
        Self {
            pub_key,
            power,
            name: Some(name),
        }
    }

    /// Get the address (first 20 bytes of the SHA256 hash of the public key)
    pub fn address(&self) -> [u8; 20] {
        use sha2::{Sha256, Digest};
        let hash = Sha256::digest(&self.pub_key);
        let mut address = [0u8; 20];
        address.copy_from_slice(&hash[..20]);
        address
    }
}

/// Validator set management
#[derive(Debug, Clone, Default)]
pub struct ValidatorSet {
    /// Validators indexed by their address
    validators: HashMap<[u8; 20], Validator>,
    /// Total voting power
    total_power: i64,
}

impl ValidatorSet {
    /// Create a new empty validator set
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
            total_power: 0,
        }
    }

    /// Add a validator to the set
    pub fn add_validator(&mut self, pub_key: Vec<u8>, power: i64) {
        let validator = Validator::new(pub_key, power);
        let address = validator.address();

        // Update total power
        if let Some(existing) = self.validators.get(&address) {
            self.total_power -= existing.power;
        }
        self.total_power += power;

        if power > 0 {
            self.validators.insert(address, validator);
        } else {
            // Power of 0 means remove the validator
            self.validators.remove(&address);
        }
    }

    /// Update a validator's power
    pub fn update_power(&mut self, address: &[u8; 20], new_power: i64) -> bool {
        if let Some(validator) = self.validators.get_mut(address) {
            self.total_power -= validator.power;
            self.total_power += new_power;
            validator.power = new_power;

            if new_power == 0 {
                self.validators.remove(address);
            }
            true
        } else {
            false
        }
    }

    /// Remove a validator from the set
    pub fn remove_validator(&mut self, address: &[u8; 20]) -> Option<Validator> {
        if let Some(validator) = self.validators.remove(address) {
            self.total_power -= validator.power;
            Some(validator)
        } else {
            None
        }
    }

    /// Get a validator by address
    pub fn get(&self, address: &[u8; 20]) -> Option<&Validator> {
        self.validators.get(address)
    }

    /// Get the total voting power
    pub fn total_power(&self) -> i64 {
        self.total_power
    }

    /// Get the number of validators
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Check if the validator set is empty
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }

    /// Get all validators
    pub fn all_validators(&self) -> Vec<&Validator> {
        self.validators.values().collect()
    }

    /// Apply validator updates from end_block
    pub fn apply_updates(&mut self, updates: Vec<(Vec<u8>, i64)>) {
        for (pub_key, power) in updates {
            self.add_validator(pub_key, power);
        }
    }

    /// Check if an address is a validator
    pub fn is_validator(&self, address: &[u8; 20]) -> bool {
        self.validators.contains_key(address)
    }

    /// Get the proposer for a given height (simple round-robin)
    /// In production, this would use weighted round-robin based on voting power
    pub fn get_proposer(&self, height: u64) -> Option<&Validator> {
        if self.validators.is_empty() {
            return None;
        }

        let mut validators: Vec<_> = self.validators.values().collect();
        validators.sort_by_key(|v| &v.pub_key);

        let index = (height as usize) % validators.len();
        Some(validators[index])
    }

    /// Calculate the threshold for a supermajority (2/3+)
    pub fn supermajority_threshold(&self) -> i64 {
        (self.total_power * 2 + 2) / 3
    }

    /// Check if the given power constitutes a supermajority
    pub fn is_supermajority(&self, power: i64) -> bool {
        power >= self.supermajority_threshold()
    }
}

/// Validator update for CometBFT
#[derive(Debug, Clone)]
pub struct ValidatorUpdateSet {
    /// Updates to apply
    pub updates: Vec<ValidatorUpdate>,
}

/// Single validator update
#[derive(Debug, Clone)]
pub struct ValidatorUpdate {
    pub pub_key: Vec<u8>,
    pub power: i64,
}

impl ValidatorUpdateSet {
    pub fn new() -> Self {
        Self { updates: Vec::new() }
    }

    pub fn add_update(&mut self, pub_key: Vec<u8>, power: i64) {
        self.updates.push(ValidatorUpdate { pub_key, power });
    }
}

impl Default for ValidatorUpdateSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pubkey(id: u8) -> Vec<u8> {
        vec![id; 32]
    }

    #[test]
    fn test_validator_set_basic() {
        let mut set = ValidatorSet::new();
        assert!(set.is_empty());
        assert_eq!(set.total_power(), 0);

        set.add_validator(make_pubkey(1), 100);
        assert_eq!(set.len(), 1);
        assert_eq!(set.total_power(), 100);
    }

    #[test]
    fn test_validator_set_multiple() {
        let mut set = ValidatorSet::new();

        set.add_validator(make_pubkey(1), 100);
        set.add_validator(make_pubkey(2), 200);
        set.add_validator(make_pubkey(3), 300);

        assert_eq!(set.len(), 3);
        assert_eq!(set.total_power(), 600);
    }

    #[test]
    fn test_validator_removal() {
        let mut set = ValidatorSet::new();

        set.add_validator(make_pubkey(1), 100);
        let validator = Validator::new(make_pubkey(1), 100);
        let address = validator.address();

        // Remove by setting power to 0
        set.add_validator(make_pubkey(1), 0);
        assert!(set.is_empty());
        assert_eq!(set.total_power(), 0);
    }

    #[test]
    fn test_supermajority() {
        let mut set = ValidatorSet::new();

        set.add_validator(make_pubkey(1), 100);
        set.add_validator(make_pubkey(2), 100);
        set.add_validator(make_pubkey(3), 100);

        // Total power: 300, threshold: (300 * 2 + 2) / 3 = 200
        // Supermajority requires >= threshold
        assert!(!set.is_supermajority(199));
        assert!(set.is_supermajority(200));
        assert!(set.is_supermajority(300));
    }

    #[test]
    fn test_proposer_selection() {
        let mut set = ValidatorSet::new();

        set.add_validator(make_pubkey(1), 100);
        set.add_validator(make_pubkey(2), 200);

        // Should get different proposers for different heights
        let proposer_0 = set.get_proposer(0).unwrap();
        let proposer_1 = set.get_proposer(1).unwrap();

        assert_ne!(proposer_0.pub_key, proposer_1.pub_key);
    }
}
