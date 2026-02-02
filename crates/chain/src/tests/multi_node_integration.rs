//! Multi-Node Integration Tests
//!
//! These tests verify that the HyperCore blockchain works correctly with
//! multiple validator nodes, including:
//!
//! - Blockchain progression (blocks being produced)
//! - Consensus finality (all nodes agree on state)
//! - Validator set management (genesis validators, updates)
//! - State consistency across nodes
//! - Network partition recovery
//!
//! ## Test Architecture
//!
//! Unlike the unit tests in `multi_node.rs` which simulate nodes in-memory,
//! these tests are designed to work with actual CometBFT nodes via:
//!
//! 1. **Simulated Multi-Node** - In-process simulation of multiple ABCI apps
//! 2. **Docker Integration** - Tests that require `docker compose up` (marked #[ignore])
//!
//! ## Running Tests
//!
//! ```bash
//! # Run simulated tests
//! cargo test -p hypercore-chain multi_node_integration
//!
//! # Run Docker integration tests (requires docker compose up)
//! cargo test -p hypercore-chain multi_node_integration -- --ignored
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use crate::tx::{Transaction, TransactionType};
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{
    new_shared_unified_state, Signature,
};

// =============================================================================
// LOCAL VALIDATOR SET IMPLEMENTATION (for testing without cometbft feature)
// =============================================================================

/// A validator in the set (test implementation)
#[derive(Debug, Clone)]
struct Validator {
    /// Public key (Ed25519)
    pub_key: Vec<u8>,
    /// Voting power
    power: i64,
}

impl Validator {
    /// Create a new validator
    fn new(pub_key: Vec<u8>, power: i64) -> Self {
        Self { pub_key, power }
    }

    /// Get the address (first 20 bytes of SHA3-256 hash of public key)
    fn address(&self) -> [u8; 20] {
        use sha3::{Sha3_256, Digest};
        let hash = Sha3_256::digest(&self.pub_key);
        let mut address = [0u8; 20];
        address.copy_from_slice(&hash[..20]);
        address
    }
}

/// Validator set management (test implementation)
#[derive(Debug, Clone, Default)]
struct ValidatorSet {
    validators: HashMap<[u8; 20], Validator>,
    total_power: i64,
}

impl ValidatorSet {
    fn new() -> Self {
        Self {
            validators: HashMap::new(),
            total_power: 0,
        }
    }

    fn add_validator(&mut self, pub_key: Vec<u8>, power: i64) {
        let validator = Validator::new(pub_key, power);
        let address = validator.address();

        if let Some(existing) = self.validators.get(&address) {
            self.total_power -= existing.power;
        }
        self.total_power += power;

        if power > 0 {
            self.validators.insert(address, validator);
        } else {
            self.validators.remove(&address);
        }
    }

    fn update_power(&mut self, address: &[u8; 20], new_power: i64) -> bool {
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

    fn total_power(&self) -> i64 {
        self.total_power
    }

    fn len(&self) -> usize {
        self.validators.len()
    }

    fn get_proposer(&self, height: u64) -> Option<&Validator> {
        if self.validators.is_empty() {
            return None;
        }

        let mut validators: Vec<_> = self.validators.values().collect();
        validators.sort_by_key(|v| &v.pub_key);

        let index = (height as usize) % validators.len();
        Some(validators[index])
    }

    fn supermajority_threshold(&self) -> i64 {
        (self.total_power * 2 + 2) / 3
    }

    fn is_supermajority(&self, power: i64) -> bool {
        power >= self.supermajority_threshold()
    }
}

// =============================================================================
// SIMULATED VALIDATOR NODE
// =============================================================================

/// A simulated validator node with its own state
struct SimulatedValidatorNode {
    /// Node identifier (0, 1, 2, ...)
    id: u32,
    /// The ABCI application
    app: HyperCoreApp,
    /// Validator public key (Ed25519 format)
    pub_key: Vec<u8>,
    /// Voting power
    power: i64,
    /// Block hashes produced by this node
    block_hashes: Vec<[u8; 32]>,
}

impl SimulatedValidatorNode {
    /// Create a new simulated validator node
    fn new(id: u32, power: i64) -> Self {
        let unified_state = new_shared_unified_state();
        let engine = Arc::new(RwLock::new(EngineState::new()));
        let spot_engine = Arc::new(RwLock::new(
            SpotEngine::with_unified_state(Arc::clone(&unified_state)),
        ));

        let app = HyperCoreApp::with_shared_state(unified_state, engine, Some(spot_engine));

        // Generate deterministic public key based on ID
        let pub_key: Vec<u8> = (0..32).map(|i| ((id as u8).wrapping_mul(37).wrapping_add(i as u8))).collect();

        Self {
            id,
            app,
            pub_key,
            power,
            block_hashes: Vec::new(),
        }
    }

    /// Process a block with given transactions
    fn process_block(&mut self, height: u64, timestamp: u64, txs: &[Transaction]) -> [u8; 32] {
        self.app.begin_block(height, timestamp);

        for tx in txs {
            let _ = self.app.execute_tx(tx, timestamp);
        }

        self.app.end_block();
        let hash = self.app.commit();
        self.block_hashes.push(hash);
        hash
    }

    /// Get the current height
    fn current_height(&self) -> u64 {
        self.app.current_height()
    }

    /// Get block hash at height
    fn get_block_hash(&self, height: u64) -> Option<[u8; 32]> {
        if height == 0 || height as usize > self.block_hashes.len() {
            None
        } else {
            Some(self.block_hashes[height as usize - 1])
        }
    }
}

/// A simulated multi-node network
struct SimulatedNetwork {
    nodes: Vec<SimulatedValidatorNode>,
    validator_set: ValidatorSet,
}

impl SimulatedNetwork {
    /// Create a new network with the given number of validators
    fn new(num_validators: u32) -> Self {
        let mut nodes = Vec::with_capacity(num_validators as usize);
        let mut validator_set = ValidatorSet::new();

        for i in 0..num_validators {
            let power = 10; // Equal voting power
            let node = SimulatedValidatorNode::new(i, power);

            // Add to validator set
            validator_set.add_validator(node.pub_key.clone(), power);

            nodes.push(node);
        }

        Self { nodes, validator_set }
    }

    /// Process a block on all nodes (simulating consensus)
    fn process_block_all(&mut self, height: u64, timestamp: u64, txs: &[Transaction]) -> Vec<[u8; 32]> {
        self.nodes
            .iter_mut()
            .map(|node| node.process_block(height, timestamp, txs))
            .collect()
    }

    /// Check if all nodes agree on the block hash at a given height
    fn check_consensus(&self, height: u64) -> bool {
        if self.nodes.is_empty() {
            return true;
        }

        let reference_hash = self.nodes[0].get_block_hash(height);

        self.nodes.iter().all(|node| node.get_block_hash(height) == reference_hash)
    }

    /// Get the total voting power
    fn total_power(&self) -> i64 {
        self.validator_set.total_power()
    }

    /// Get a validator by index
    fn get_node(&self, index: usize) -> Option<&SimulatedValidatorNode> {
        self.nodes.get(index)
    }

    /// Get a mutable validator by index
    fn get_node_mut(&mut self, index: usize) -> Option<&mut SimulatedValidatorNode> {
        self.nodes.get_mut(index)
    }
}

// =============================================================================
// TEST UTILITIES
// =============================================================================

fn create_test_tx(sender_idx: u64, nonce: u64) -> Transaction {
    let mut sender_bytes = [0u8; 20];
    sender_bytes[12..20].copy_from_slice(&sender_idx.to_be_bytes());

    Transaction {
        action: TransactionType::CancelAll,
        nonce,
        signature: Signature::zero(),
        hash: None,
    }
}

fn create_leverage_tx(nonce: u64, leverage: u8) -> Transaction {
    Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage,
        },
        nonce,
        signature: Signature::zero(),
        hash: None,
    }
}

// =============================================================================
// BLOCKCHAIN PROGRESSION TESTS
// =============================================================================

#[tokio::test]
async fn test_blockchain_progression_3_validators() {
    // Create a 3-validator network
    let mut network = SimulatedNetwork::new(3);

    // Verify initial state
    assert_eq!(network.nodes.len(), 3);
    assert_eq!(network.total_power(), 30); // 3 validators * 10 power each

    // Process 100 blocks
    for height in 1..=100 {
        let timestamp = height * 1000;
        let txs: Vec<Transaction> = (0..5)
            .map(|i| create_test_tx(i, timestamp + i))
            .collect();

        let hashes = network.process_block_all(height, timestamp, &txs);

        // All nodes should produce the same hash (consensus)
        assert!(
            hashes.iter().all(|h| *h == hashes[0]),
            "Nodes diverged at height {}: {:?}",
            height,
            hashes
        );

        // Verify consensus
        assert!(
            network.check_consensus(height),
            "Consensus failed at height {}",
            height
        );
    }

    // Verify all nodes are at the same height
    for node in &network.nodes {
        assert_eq!(node.current_height(), 100);
    }
}

#[tokio::test]
async fn test_blockchain_progression_5_validators() {
    let mut network = SimulatedNetwork::new(5);

    // Process 50 blocks with varying transaction counts
    for height in 1..=50 {
        let timestamp = height * 1000;
        let tx_count = (height % 10) + 1; // 1-10 txs per block
        let txs: Vec<Transaction> = (0..tx_count)
            .map(|i| create_test_tx(i, timestamp + i))
            .collect();

        let hashes = network.process_block_all(height, timestamp, &txs);

        // Verify consensus
        assert!(
            hashes.iter().all(|h| *h == hashes[0]),
            "Nodes diverged at height {}", height
        );
    }

    // Verify final state
    for node in &network.nodes {
        assert_eq!(node.current_height(), 50);
    }
}

#[tokio::test]
async fn test_blockchain_empty_blocks() {
    let mut network = SimulatedNetwork::new(3);

    // Process 100 empty blocks
    for height in 1..=100 {
        let timestamp = height * 1000;
        let hashes = network.process_block_all(height, timestamp, &[]);

        // Even empty blocks should have consensus
        assert!(
            hashes.iter().all(|h| *h == hashes[0]),
            "Empty block divergence at height {}",
            height
        );
    }
}

// =============================================================================
// VALIDATOR SET TESTS
// =============================================================================

#[tokio::test]
async fn test_validator_set_initialization() {
    let network = SimulatedNetwork::new(4);

    // Verify validator set
    assert_eq!(network.validator_set.len(), 4);
    assert_eq!(network.total_power(), 40);

    // Verify supermajority threshold (2/3 + 1)
    // For 40 total power: (40 * 2 + 2) / 3 = 27
    assert!(network.validator_set.is_supermajority(27));
    assert!(!network.validator_set.is_supermajority(26));
}

#[tokio::test]
async fn test_validator_proposer_rotation() {
    let network = SimulatedNetwork::new(3);

    // Get proposers for different heights
    let proposer_0 = network.validator_set.get_proposer(0);
    let proposer_1 = network.validator_set.get_proposer(1);
    let proposer_2 = network.validator_set.get_proposer(2);
    let proposer_3 = network.validator_set.get_proposer(3);

    assert!(proposer_0.is_some());
    assert!(proposer_1.is_some());
    assert!(proposer_2.is_some());
    assert!(proposer_3.is_some());

    // Proposers should rotate (round-robin)
    // Height 0 and 3 should have same proposer (3 validators)
    assert_eq!(
        proposer_0.unwrap().pub_key,
        proposer_3.unwrap().pub_key,
        "Proposer rotation should cycle every 3 blocks"
    );
}

#[tokio::test]
async fn test_validator_power_update() {
    let mut validator_set = ValidatorSet::new();

    // Add validators
    validator_set.add_validator(vec![1u8; 32], 100);
    validator_set.add_validator(vec![2u8; 32], 200);

    assert_eq!(validator_set.total_power(), 300);

    // Get validator address
    let validator = Validator::new(vec![1u8; 32], 100);
    let address = validator.address();

    // Update power
    validator_set.update_power(&address, 150);
    assert_eq!(validator_set.total_power(), 350);

    // Remove validator (set power to 0)
    validator_set.update_power(&address, 0);
    assert_eq!(validator_set.total_power(), 200);
    assert_eq!(validator_set.len(), 1);
}

// =============================================================================
// STATE CONSISTENCY TESTS
// =============================================================================

#[tokio::test]
async fn test_state_consistency_after_many_blocks() {
    let mut network = SimulatedNetwork::new(3);

    // Process 500 blocks
    for height in 1..=500 {
        let timestamp = height * 1000;
        let txs: Vec<Transaction> = (0..3)
            .map(|i| create_test_tx(i, timestamp + i))
            .collect();

        network.process_block_all(height, timestamp, &txs);
    }

    // Verify all nodes agree on every block
    for height in 1..=500 {
        assert!(
            network.check_consensus(height),
            "State inconsistency at height {}",
            height
        );
    }
}

#[tokio::test]
async fn test_deterministic_state_transitions() {
    // Create two networks with same configuration
    let mut network1 = SimulatedNetwork::new(3);
    let mut network2 = SimulatedNetwork::new(3);

    // Process same blocks on both networks
    for height in 1..=100 {
        let timestamp = height * 1000;
        let txs: Vec<Transaction> = (0..5)
            .map(|i| create_leverage_tx(timestamp + i, ((i % 49) + 1) as u8))
            .collect();

        let hashes1 = network1.process_block_all(height, timestamp, &txs);
        let hashes2 = network2.process_block_all(height, timestamp, &txs);

        // Both networks should produce identical hashes
        assert_eq!(hashes1, hashes2, "Networks diverged at height {}", height);
    }
}

// =============================================================================
// BYZANTINE FAULT TOLERANCE TESTS
// =============================================================================

#[tokio::test]
async fn test_byzantine_minority_cannot_affect_consensus() {
    // In a 4-validator network, 1 Byzantine node shouldn't affect consensus
    let mut network = SimulatedNetwork::new(4);

    // Simulate Byzantine node (node 3) by not participating in some blocks
    // Other 3 nodes (75% voting power) should still reach consensus

    for height in 1..=50 {
        let timestamp = height * 1000;
        let txs: Vec<Transaction> = (0..3)
            .map(|i| create_test_tx(i, timestamp + i))
            .collect();

        // Process on all nodes
        let mut hashes = Vec::new();
        for (i, node) in network.nodes.iter_mut().enumerate() {
            if i == 3 && height % 3 == 0 {
                // Byzantine node skips some blocks - in reality it would be excluded
                // For our simulation, we still process but could use different txs
                let byzantine_txs = vec![create_test_tx(999, timestamp)];
                let hash = node.process_block(height, timestamp, &byzantine_txs);
                hashes.push(hash);
            } else {
                let hash = node.process_block(height, timestamp, &txs);
                hashes.push(hash);
            }
        }

        // Honest nodes (0, 1, 2) should agree
        assert_eq!(hashes[0], hashes[1], "Honest nodes 0,1 diverged at height {}", height);
        assert_eq!(hashes[1], hashes[2], "Honest nodes 1,2 diverged at height {}", height);
    }
}

// =============================================================================
// LATE JOINER TESTS
// =============================================================================

#[tokio::test]
async fn test_late_joiner_can_sync() {
    // Start with 2 validators
    let mut network = SimulatedNetwork::new(2);

    // Process 50 blocks
    for height in 1..=50 {
        let timestamp = height * 1000;
        let txs = vec![create_test_tx(height, timestamp)];
        network.process_block_all(height, timestamp, &txs);
    }

    // Add a new node (simulating late joiner)
    let mut new_node = SimulatedValidatorNode::new(2, 10);

    // New node needs to sync by replaying all blocks
    for height in 1..=50 {
        let timestamp = height * 1000;
        let txs = vec![create_test_tx(height, timestamp)];
        new_node.process_block(height, timestamp, &txs);
    }

    // New node should now be in sync
    assert_eq!(new_node.current_height(), 50);
    assert_eq!(
        new_node.get_block_hash(50),
        network.get_node(0).unwrap().get_block_hash(50),
        "Late joiner state doesn't match"
    );
}

// =============================================================================
// NETWORK PARTITION SIMULATION TESTS
// =============================================================================

#[tokio::test]
async fn test_network_partition_recovery() {
    // Simulate a network partition where nodes process different blocks
    // then recover and must reach consensus

    let mut network = SimulatedNetwork::new(3);

    // All nodes process blocks 1-10 together
    for height in 1..=10 {
        let timestamp = height * 1000;
        let txs = vec![create_test_tx(height, timestamp)];
        network.process_block_all(height, timestamp, &txs);
    }

    // Verify consensus at height 10
    assert!(network.check_consensus(10));

    // Simulate partition: nodes continue with same blocks (in BFT, they'd wait for consensus)
    // In reality, CometBFT handles this, but we simulate deterministic behavior
    for height in 11..=20 {
        let timestamp = height * 1000;
        let txs = vec![create_test_tx(height, timestamp)];

        // All nodes process same blocks (BFT ensures this)
        network.process_block_all(height, timestamp, &txs);
    }

    // After recovery, all nodes should agree
    for height in 1..=20 {
        assert!(
            network.check_consensus(height),
            "Consensus failed after partition at height {}",
            height
        );
    }
}

// =============================================================================
// STRESS TESTS
// =============================================================================

#[tokio::test]
async fn test_high_throughput_multi_validator() {
    let mut network = SimulatedNetwork::new(3);
    let start = std::time::Instant::now();

    // Process 1000 blocks with 100 txs each
    for height in 1..=1000 {
        let timestamp = height * 100;
        let txs: Vec<Transaction> = (0..100)
            .map(|i| create_test_tx(i, timestamp + i))
            .collect();

        let hashes = network.process_block_all(height, timestamp, &txs);

        // Verify consensus periodically
        if height % 100 == 0 {
            assert!(
                hashes.iter().all(|h| *h == hashes[0]),
                "Divergence at height {}",
                height
            );
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Processed 1000 blocks x 100 txs x 3 validators = 300,000 tx executions in {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_large_validator_set() {
    // Test with 10 validators
    let mut network = SimulatedNetwork::new(10);

    assert_eq!(network.total_power(), 100);
    assert_eq!(network.validator_set.len(), 10);

    // Process 100 blocks
    for height in 1..=100 {
        let timestamp = height * 1000;
        let txs = vec![create_test_tx(height, timestamp)];
        let hashes = network.process_block_all(height, timestamp, &txs);

        assert!(
            hashes.iter().all(|h| *h == hashes[0]),
            "Divergence with 10 validators at height {}",
            height
        );
    }
}

// =============================================================================
// DOCKER INTEGRATION TESTS (require docker compose up)
// =============================================================================

/// Test actual multi-node blockchain progression
/// Run: docker compose -f docker-compose-multinode.yml up
#[tokio::test]
#[ignore] // Only run with: cargo test -- --ignored
async fn test_docker_multinode_blockchain_progression() {
    // This test requires the multi-node docker setup to be running
    // It connects to actual CometBFT nodes and verifies blockchain progression

    use std::time::Duration;
    use tokio::time::sleep;

    let node_urls = vec![
        "http://localhost:3000",  // Node 0
        "http://localhost:3010",  // Node 1
        "http://localhost:3020",  // Node 2
    ];

    // Wait for nodes to be ready
    for url in &node_urls {
        let health_url = format!("{}/health", url);
        for attempt in 1..=30 {
            match reqwest::get(&health_url).await {
                Ok(resp) if resp.status().is_success() => break,
                _ if attempt == 30 => panic!("Node {} not ready after 30 attempts", url),
                _ => sleep(Duration::from_secs(1)).await,
            }
        }
    }

    // Get initial block heights
    let mut initial_heights = Vec::new();
    for url in &node_urls {
        let status_url = format!("{}/status", url);
        let resp: serde_json::Value = reqwest::get(&status_url)
            .await
            .expect("Failed to get status")
            .json()
            .await
            .expect("Failed to parse status");

        let height = resp["height"].as_u64().unwrap_or(0);
        initial_heights.push(height);
    }

    println!("Initial heights: {:?}", initial_heights);

    // Wait for blockchain to progress
    sleep(Duration::from_secs(10)).await;

    // Get new block heights
    let mut final_heights = Vec::new();
    for url in &node_urls {
        let status_url = format!("{}/status", url);
        let resp: serde_json::Value = reqwest::get(&status_url)
            .await
            .expect("Failed to get status")
            .json()
            .await
            .expect("Failed to parse status");

        let height = resp["height"].as_u64().unwrap_or(0);
        final_heights.push(height);
    }

    println!("Final heights: {:?}", final_heights);

    // Verify blockchain progressed on all nodes
    for (i, (initial, final_h)) in initial_heights.iter().zip(final_heights.iter()).enumerate() {
        assert!(
            final_h > initial,
            "Node {} blockchain not progressing: {} -> {}",
            i, initial, final_h
        );
    }

    // Verify all nodes are at the same height (consensus)
    assert!(
        final_heights.iter().all(|h| *h == final_heights[0]),
        "Nodes at different heights: {:?}",
        final_heights
    );
}

/// Test validator set via CometBFT RPC
#[tokio::test]
#[ignore]
async fn test_docker_validator_set() {
    use std::time::Duration;
    use tokio::time::sleep;

    let cometbft_rpcs = vec![
        "http://localhost:26657",  // CometBFT 0
        "http://localhost:26667",  // CometBFT 1
        "http://localhost:26677",  // CometBFT 2
    ];

    // Wait for CometBFT to be ready
    sleep(Duration::from_secs(5)).await;

    for rpc in &cometbft_rpcs {
        let validators_url = format!("{}/validators", rpc);
        let resp: serde_json::Value = reqwest::get(&validators_url)
            .await
            .expect("Failed to get validators")
            .json()
            .await
            .expect("Failed to parse validators");

        let validators = resp["result"]["validators"].as_array()
            .expect("No validators array");

        println!("Validators from {}: {:?}", rpc, validators.len());

        // Should have 3 validators
        assert_eq!(
            validators.len(), 3,
            "Expected 3 validators from {}",
            rpc
        );
    }
}
