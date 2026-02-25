//! Attestation Collector for State Verification
//!
//! This module collects state attestations from validators and detects divergence.
//! When enough attestations are collected (quorum), it compares the app_hashes to
//! detect if any validators have diverged from the majority.
//!
//! ## Divergence Detection
//!
//! Divergence is detected when:
//! 1. We have attestations from 2/3+ of voting power (quorum)
//! 2. There are multiple different app_hashes for the same height
//!
//! When divergence is detected, the collector sends a `DivergenceAlert` to the handler.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use super::attestation::StateAttestation;

/// Configuration for the attestation collector
#[derive(Debug, Clone)]
pub struct AttestationConfig {
    /// Number of blocks to keep attestations for (default: 100)
    pub retention_blocks: u64,
    /// Timeout for collecting attestations in milliseconds (default: 5000)
    pub collection_timeout_ms: u64,
    /// Quorum threshold as a fraction (default: 0.67 for 2/3)
    pub quorum_threshold: f64,
    /// Whether to enable divergence detection (default: true)
    pub enabled: bool,
}

impl Default for AttestationConfig {
    fn default() -> Self {
        Self {
            retention_blocks: 100,
            collection_timeout_ms: 5000,
            quorum_threshold: 0.67,
            enabled: true,
        }
    }
}

/// Alert sent when state divergence is detected
#[derive(Debug, Clone)]
pub struct DivergenceAlert {
    /// Block height where divergence was detected
    pub height: u64,
    /// Our computed app_hash
    pub our_hash: [u8; 32],
    /// The majority app_hash (if determinable)
    pub majority_hash: Option<[u8; 32]>,
    /// All attestations received for this height
    pub attestations: Vec<StateAttestation>,
    /// Whether we are in the minority
    pub we_are_minority: bool,
    /// Voting power breakdown by hash
    pub hash_votes: BTreeMap<[u8; 32], u64>,
}

/// Attestation collector that gathers and analyzes state attestations
pub struct AttestationCollector {
    /// Attestations received per height
    /// Using BTreeMap for deterministic iteration order
    attestations: Arc<RwLock<BTreeMap<u64, Vec<StateAttestation>>>>,
    /// Known validators and their voting power
    validators: Arc<RwLock<HashMap<[u8; 32], u64>>>,
    /// Our validator's public key (if we are a validator)
    #[allow(dead_code)]
    our_pubkey: Option<[u8; 32]>,
    /// Our last computed app_hash per height
    our_hashes: Arc<RwLock<BTreeMap<u64, [u8; 32]>>>,
    /// Channel to send divergence alerts
    alert_tx: mpsc::Sender<DivergenceAlert>,
    /// Configuration
    config: AttestationConfig,
}

impl AttestationCollector {
    /// Create a new attestation collector
    ///
    /// # Arguments
    /// * `alert_tx` - Channel to send divergence alerts
    /// * `config` - Configuration options
    /// * `our_pubkey` - Our validator's public key (None if not a validator)
    pub fn new(
        alert_tx: mpsc::Sender<DivergenceAlert>,
        config: AttestationConfig,
        our_pubkey: Option<[u8; 32]>,
    ) -> Self {
        Self {
            attestations: Arc::new(RwLock::new(BTreeMap::new())),
            validators: Arc::new(RwLock::new(HashMap::new())),
            our_pubkey,
            our_hashes: Arc::new(RwLock::new(BTreeMap::new())),
            alert_tx,
            config,
        }
    }

    /// Register a validator with their voting power
    pub async fn add_validator(&self, pubkey: [u8; 32], voting_power: u64) {
        let mut validators = self.validators.write().await;
        validators.insert(pubkey, voting_power);
        tracing::debug!(
            "Added validator {:?} with voting power {}",
            hex::encode(&pubkey[..8]),
            voting_power
        );
    }

    /// Remove a validator
    pub async fn remove_validator(&self, pubkey: &[u8; 32]) {
        let mut validators = self.validators.write().await;
        validators.remove(pubkey);
    }

    /// Update validator's voting power
    pub async fn update_validator_power(&self, pubkey: [u8; 32], voting_power: u64) {
        let mut validators = self.validators.write().await;
        if voting_power == 0 {
            validators.remove(&pubkey);
        } else {
            validators.insert(pubkey, voting_power);
        }
    }

    /// Get total voting power of all validators
    pub async fn total_voting_power(&self) -> u64 {
        let validators = self.validators.read().await;
        validators.values().sum()
    }

    /// Record our own computed app_hash for a height
    pub async fn record_our_hash(&self, height: u64, app_hash: [u8; 32]) {
        let mut our_hashes = self.our_hashes.write().await;
        our_hashes.insert(height, app_hash);

        // Cleanup old entries
        let min_height = height.saturating_sub(self.config.retention_blocks);
        our_hashes.retain(|h, _| *h >= min_height);

        tracing::debug!(
            "Recorded our app_hash for height {}: {:?}",
            height,
            hex::encode(&app_hash[..8])
        );
    }

    /// Process a received attestation
    ///
    /// Validates the attestation and stores it. If quorum is reached,
    /// checks for divergence and sends an alert if detected.
    pub async fn process_attestation(&self, attestation: StateAttestation) -> Result<(), AttestationError> {
        if !self.config.enabled {
            return Ok(());
        }

        // 1. Verify signature
        if !attestation.verify() {
            tracing::warn!(
                "Invalid attestation signature from validator {:?}",
                attestation.validator_hex()
            );
            return Err(AttestationError::InvalidSignature);
        }

        // 2. Check if validator is known
        let validators = self.validators.read().await;
        if !validators.contains_key(&attestation.validator_pubkey) {
            tracing::debug!(
                "Attestation from unknown validator {:?}",
                attestation.validator_hex()
            );
            return Err(AttestationError::UnknownValidator);
        }
        drop(validators);

        // 3. Check if attestation is for a recent height
        let our_hashes = self.our_hashes.read().await;
        let our_latest_height = our_hashes.keys().max().copied().unwrap_or(0);
        drop(our_hashes);

        if attestation.height + self.config.retention_blocks < our_latest_height {
            tracing::debug!(
                "Attestation for old height {} (current: {})",
                attestation.height,
                our_latest_height
            );
            return Err(AttestationError::HeightTooOld);
        }

        // 4. Store attestation
        let mut attestations = self.attestations.write().await;
        let height_attestations = attestations.entry(attestation.height).or_insert_with(Vec::new);

        // Check for duplicate (same validator, same height)
        if height_attestations.iter().any(|a| a.validator_pubkey == attestation.validator_pubkey) {
            tracing::debug!(
                "Duplicate attestation from {:?} for height {}",
                attestation.validator_hex(),
                attestation.height
            );
            return Err(AttestationError::DuplicateAttestation);
        }

        let height = attestation.height;
        height_attestations.push(attestation);

        // Cleanup old attestations
        let min_height = height.saturating_sub(self.config.retention_blocks);
        attestations.retain(|h, _| *h >= min_height);

        drop(attestations);

        // 5. Check for divergence
        self.check_divergence(height).await;

        Ok(())
    }

    /// Check for divergence at a given height
    async fn check_divergence(&self, height: u64) {
        let attestations = self.attestations.read().await;
        let validators = self.validators.read().await;
        let our_hashes = self.our_hashes.read().await;

        let height_attestations = match attestations.get(&height) {
            Some(a) => a,
            None => return,
        };

        // Calculate voting power by hash
        let mut hash_votes: BTreeMap<[u8; 32], u64> = BTreeMap::new();
        let mut total_voted: u64 = 0;

        for att in height_attestations {
            if let Some(&power) = validators.get(&att.validator_pubkey) {
                *hash_votes.entry(att.app_hash).or_insert(0) += power;
                total_voted += power;
            }
        }

        // Check if we have quorum
        let total_power: u64 = validators.values().sum();
        if total_power == 0 {
            return;
        }

        let quorum_ratio = total_voted as f64 / total_power as f64;
        if quorum_ratio < self.config.quorum_threshold {
            tracing::debug!(
                "Not enough votes for height {} ({:.1}% < {:.1}%)",
                height,
                quorum_ratio * 100.0,
                self.config.quorum_threshold * 100.0
            );
            return;
        }

        // Find majority hash
        let majority_hash = hash_votes
            .iter()
            .max_by_key(|(_, &power)| power)
            .map(|(hash, _)| *hash);

        // Check if there's divergence (multiple different hashes)
        if hash_votes.len() <= 1 {
            tracing::debug!(
                "No divergence at height {} - all {} attestations agree",
                height,
                height_attestations.len()
            );
            return;
        }

        // DIVERGENCE DETECTED!
        tracing::error!(
            "STATE DIVERGENCE DETECTED at height {}! {} different hashes from {} attestations",
            height,
            hash_votes.len(),
            height_attestations.len()
        );

        // Log the hash distribution
        for (hash, power) in &hash_votes {
            let percentage = (*power as f64 / total_voted as f64) * 100.0;
            tracing::error!(
                "  Hash {:?}: {:.1}% voting power",
                hex::encode(&hash[..8]),
                percentage
            );
        }

        // Determine if we are in the minority
        let our_hash = our_hashes.get(&height).copied();
        let we_are_minority = match (our_hash, majority_hash) {
            (Some(our), Some(majority)) => our != majority,
            _ => false,
        };

        if we_are_minority {
            tracing::error!(
                "WE ARE IN THE MINORITY! Our hash: {:?}, majority: {:?}",
                our_hash.map(|h| hex::encode(&h[..8])),
                majority_hash.map(|h| hex::encode(&h[..8]))
            );
        }

        // Send divergence alert
        let alert = DivergenceAlert {
            height,
            our_hash: our_hash.unwrap_or([0u8; 32]),
            majority_hash,
            attestations: height_attestations.clone(),
            we_are_minority,
            hash_votes,
        };

        if let Err(e) = self.alert_tx.send(alert).await {
            tracing::error!("Failed to send divergence alert: {}", e);
        }
    }

    /// Get the number of attestations received for a height
    pub async fn attestation_count(&self, height: u64) -> usize {
        let attestations = self.attestations.read().await;
        attestations.get(&height).map(|a| a.len()).unwrap_or(0)
    }

    /// Get all attestations for a height
    pub async fn get_attestations(&self, height: u64) -> Vec<StateAttestation> {
        let attestations = self.attestations.read().await;
        attestations.get(&height).cloned().unwrap_or_default()
    }

    /// Get statistics about attestation collection
    pub async fn stats(&self) -> AttestationStats {
        let attestations = self.attestations.read().await;
        let validators = self.validators.read().await;

        let total_attestations: usize = attestations.values().map(|a| a.len()).sum();
        let heights_tracked = attestations.len();
        let validators_count = validators.len();
        let total_voting_power: u64 = validators.values().sum();

        AttestationStats {
            total_attestations,
            heights_tracked,
            validators_count,
            total_voting_power,
        }
    }
}

/// Attestation collection statistics
#[derive(Debug, Clone)]
pub struct AttestationStats {
    /// Total number of attestations stored
    pub total_attestations: usize,
    /// Number of heights being tracked
    pub heights_tracked: usize,
    /// Number of registered validators
    pub validators_count: usize,
    /// Total voting power of all validators
    pub total_voting_power: u64,
}

/// Errors that can occur during attestation processing
#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("Invalid attestation signature")]
    InvalidSignature,
    #[error("Attestation from unknown validator")]
    UnknownValidator,
    #[error("Attestation height too old")]
    HeightTooOld,
    #[error("Duplicate attestation")]
    DuplicateAttestation,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::AttestationKeyPair;

    async fn setup_collector() -> (AttestationCollector, mpsc::Receiver<DivergenceAlert>) {
        let (tx, rx) = mpsc::channel(10);
        let key_pair = AttestationKeyPair::generate();
        let collector = AttestationCollector::new(
            tx,
            AttestationConfig::default(),
            Some(key_pair.public_key()),
        );
        (collector, rx)
    }

    #[tokio::test]
    async fn test_add_validator() {
        let (collector, _rx) = setup_collector().await;
        let key = [0x42u8; 32];

        collector.add_validator(key, 100).await;
        assert_eq!(collector.total_voting_power().await, 100);

        collector.add_validator([0x43u8; 32], 50).await;
        assert_eq!(collector.total_voting_power().await, 150);
    }

    #[tokio::test]
    async fn test_remove_validator() {
        let (collector, _rx) = setup_collector().await;
        let key = [0x42u8; 32];

        collector.add_validator(key, 100).await;
        assert_eq!(collector.total_voting_power().await, 100);

        collector.remove_validator(&key).await;
        assert_eq!(collector.total_voting_power().await, 0);
    }

    #[tokio::test]
    async fn test_process_valid_attestation() {
        let (collector, _rx) = setup_collector().await;
        let key_pair = AttestationKeyPair::generate();

        // Register the validator
        collector.add_validator(key_pair.public_key(), 100).await;

        // Create and process attestation
        let attestation = key_pair.sign_attestation(1, [0x42u8; 32]);
        let result = collector.process_attestation(attestation).await;

        assert!(result.is_ok());
        assert_eq!(collector.attestation_count(1).await, 1);
    }

    #[tokio::test]
    async fn test_reject_invalid_signature() {
        let (collector, _rx) = setup_collector().await;
        let key_pair = AttestationKeyPair::generate();

        // Register the validator
        collector.add_validator(key_pair.public_key(), 100).await;

        // Create attestation with tampered signature
        let mut attestation = key_pair.sign_attestation(1, [0x42u8; 32]);
        attestation.signature[0] ^= 0xFF;

        let result = collector.process_attestation(attestation).await;
        assert!(matches!(result, Err(AttestationError::InvalidSignature)));
    }

    #[tokio::test]
    async fn test_reject_unknown_validator() {
        let (collector, _rx) = setup_collector().await;
        let key_pair = AttestationKeyPair::generate();

        // Don't register the validator
        let attestation = key_pair.sign_attestation(1, [0x42u8; 32]);
        let result = collector.process_attestation(attestation).await;

        assert!(matches!(result, Err(AttestationError::UnknownValidator)));
    }

    #[tokio::test]
    async fn test_reject_duplicate_attestation() {
        let (collector, _rx) = setup_collector().await;
        let key_pair = AttestationKeyPair::generate();

        collector.add_validator(key_pair.public_key(), 100).await;

        let attestation1 = key_pair.sign_attestation(1, [0x42u8; 32]);
        let attestation2 = key_pair.sign_attestation(1, [0x42u8; 32]);

        collector.process_attestation(attestation1).await.unwrap();
        let result = collector.process_attestation(attestation2).await;

        assert!(matches!(result, Err(AttestationError::DuplicateAttestation)));
    }

    #[tokio::test]
    async fn test_divergence_detection() {
        let (tx, mut rx) = mpsc::channel(10);
        let our_key = AttestationKeyPair::generate();
        let collector = AttestationCollector::new(
            tx,
            AttestationConfig {
                quorum_threshold: 0.5, // Lower threshold for testing
                ..Default::default()
            },
            Some(our_key.public_key()),
        );

        // Create 3 validators
        let key1 = AttestationKeyPair::generate();
        let key2 = AttestationKeyPair::generate();
        let key3 = AttestationKeyPair::generate();

        collector.add_validator(key1.public_key(), 100).await;
        collector.add_validator(key2.public_key(), 100).await;
        collector.add_validator(key3.public_key(), 100).await;

        // Record our hash
        let our_hash = [0x42u8; 32];
        collector.record_our_hash(1, our_hash).await;

        // Two validators agree on one hash
        let hash_a = [0x42u8; 32];
        collector.process_attestation(key1.sign_attestation(1, hash_a)).await.unwrap();
        collector.process_attestation(key2.sign_attestation(1, hash_a)).await.unwrap();

        // Third validator has different hash - DIVERGENCE!
        let hash_b = [0x43u8; 32];
        collector.process_attestation(key3.sign_attestation(1, hash_b)).await.unwrap();

        // Should receive divergence alert
        let alert = rx.recv().await.expect("Should receive alert");
        assert_eq!(alert.height, 1);
        assert_eq!(alert.hash_votes.len(), 2);
        assert!(!alert.we_are_minority); // We match the majority
    }

    #[tokio::test]
    async fn test_we_are_minority_detection() {
        let (tx, mut rx) = mpsc::channel(10);
        let our_key = AttestationKeyPair::generate();
        let collector = AttestationCollector::new(
            tx,
            AttestationConfig {
                quorum_threshold: 0.5,
                ..Default::default()
            },
            Some(our_key.public_key()),
        );

        let key1 = AttestationKeyPair::generate();
        let key2 = AttestationKeyPair::generate();
        let key3 = AttestationKeyPair::generate();

        collector.add_validator(key1.public_key(), 100).await;
        collector.add_validator(key2.public_key(), 100).await;
        collector.add_validator(key3.public_key(), 100).await;

        // Our hash is different from majority
        let our_hash = [0x99u8; 32];
        collector.record_our_hash(1, our_hash).await;

        // Two validators agree on different hash
        let majority_hash = [0x42u8; 32];
        collector.process_attestation(key1.sign_attestation(1, majority_hash)).await.unwrap();
        collector.process_attestation(key2.sign_attestation(1, majority_hash)).await.unwrap();

        // Third has our hash
        collector.process_attestation(key3.sign_attestation(1, our_hash)).await.unwrap();

        let alert = rx.recv().await.expect("Should receive alert");
        assert!(alert.we_are_minority); // We are in minority!
    }

    #[tokio::test]
    async fn test_no_divergence_when_all_agree() {
        let (tx, mut rx) = mpsc::channel(10);
        let collector = AttestationCollector::new(
            tx,
            AttestationConfig {
                quorum_threshold: 0.5,
                ..Default::default()
            },
            None,
        );

        let key1 = AttestationKeyPair::generate();
        let key2 = AttestationKeyPair::generate();

        collector.add_validator(key1.public_key(), 100).await;
        collector.add_validator(key2.public_key(), 100).await;

        // All agree on same hash
        let hash = [0x42u8; 32];
        collector.process_attestation(key1.sign_attestation(1, hash)).await.unwrap();
        collector.process_attestation(key2.sign_attestation(1, hash)).await.unwrap();

        // No alert should be sent
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_stats() {
        let (collector, _rx) = setup_collector().await;

        let key1 = AttestationKeyPair::generate();
        let key2 = AttestationKeyPair::generate();

        collector.add_validator(key1.public_key(), 100).await;
        collector.add_validator(key2.public_key(), 200).await;

        collector.process_attestation(key1.sign_attestation(1, [0x42u8; 32])).await.unwrap();
        collector.process_attestation(key2.sign_attestation(1, [0x42u8; 32])).await.unwrap();
        collector.process_attestation(key1.sign_attestation(2, [0x42u8; 32])).await.unwrap();

        let stats = collector.stats().await;
        assert_eq!(stats.validators_count, 2);
        assert_eq!(stats.total_voting_power, 300);
        assert_eq!(stats.total_attestations, 3);
        assert_eq!(stats.heights_tracked, 2);
    }

    #[tokio::test]
    async fn test_disabled_collector() {
        let (tx, _rx) = mpsc::channel(10);
        let collector = AttestationCollector::new(
            tx,
            AttestationConfig {
                enabled: false,
                ..Default::default()
            },
            None,
        );

        let key = AttestationKeyPair::generate();
        let attestation = key.sign_attestation(1, [0x42u8; 32]);

        // Should return Ok but not store
        let result = collector.process_attestation(attestation).await;
        assert!(result.is_ok());
        assert_eq!(collector.attestation_count(1).await, 0);
    }
}
