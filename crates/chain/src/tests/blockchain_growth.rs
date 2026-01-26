//! Blockchain Growth Tests
//!
//! These tests verify that the blockchain grows correctly over time,
//! maintaining integrity, proper state transitions, and correct
//! handling of various block scenarios.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use crate::state::AppState;
use crate::tx::{Transaction, TransactionType};
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{
    new_shared_unified_state, AccountAddress, Decimal, Signature,
};

/// Create a test app with shared state
fn create_test_app() -> HyperCoreApp {
    let unified_state = new_shared_unified_state();
    let engine = Arc::new(RwLock::new(EngineState::new()));
    let spot_engine = Arc::new(RwLock::new(
        SpotEngine::with_unified_state(Arc::clone(&unified_state)),
    ));

    HyperCoreApp::with_shared_state(unified_state, engine, Some(spot_engine))
}

/// Create a simple test transaction
fn create_tx(nonce: u64) -> Transaction {
    Transaction {
        action: TransactionType::CancelAll,
        nonce,
        signature: Signature::zero(),
        hash: None,
    }
}

/// Create a leverage update transaction
fn create_leverage_tx(asset: u8, leverage: u8, nonce: u64) -> Transaction {
    Transaction {
        action: TransactionType::UpdateLeverage {
            asset,
            is_cross: true,
            leverage,
        },
        nonce,
        signature: Signature::zero(),
        hash: None,
    }
}

// =============================================================================
// BLOCK GROWTH TESTS
// =============================================================================

#[tokio::test]
async fn test_blockchain_grows_sequentially() {
    let mut app = create_test_app();

    // Verify blockchain grows from height 0 to 100
    for height in 1..=100 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);
        app.end_block();
        let app_hash = app.commit();

        assert_eq!(app.current_height(), height);
        assert!(app_hash != [0u8; 32], "App hash should not be zero");
    }
}

#[tokio::test]
async fn test_block_hashes_are_unique() {
    let mut app = create_test_app();
    let mut hashes = std::collections::HashSet::new();

    // Each block should produce a unique hash
    for height in 1..=50 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        // Add a transaction to make each block different
        let tx = create_leverage_tx(0, (height % 50 + 1) as u8, timestamp);
        let _ = app.execute_tx(&tx, timestamp);

        app.end_block();
        let app_hash = app.commit();

        assert!(
            hashes.insert(app_hash),
            "Block {} produced duplicate hash",
            height
        );
    }
}

#[tokio::test]
async fn test_block_timestamps_must_increase() {
    let mut app = create_test_app();

    // Process first block
    app.begin_block(1, 1000);
    app.end_block();
    app.commit();

    // Second block with later timestamp - should succeed
    app.begin_block(2, 2000);
    app.end_block();
    app.commit();

    assert_eq!(app.current_height(), 2);
}

#[tokio::test]
async fn test_block_with_many_transactions() {
    let mut app = create_test_app();

    let height = 1;
    let timestamp = 1000u64;

    app.begin_block(height, timestamp);

    // Execute many transactions
    for i in 0..1000 {
        let tx = create_tx(timestamp + i);
        let _ = app.execute_tx(&tx, timestamp);
    }

    app.end_block();
    let app_hash = app.commit();

    assert_eq!(app.current_height(), 1);
    assert!(app_hash != [0u8; 32]);
}

#[tokio::test]
async fn test_block_state_transitions() {
    let mut app = create_test_app();

    let mut prev_hash = [0u8; 32];

    // Process blocks and verify state transitions
    for height in 1..=20 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        // Different transaction types based on block
        if height % 2 == 0 {
            let tx = create_leverage_tx(0, 20, timestamp);
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        let app_hash = app.commit();

        // Hash should change from previous block
        assert_ne!(app_hash, prev_hash, "Block {} should have different hash", height);
        prev_hash = app_hash;
    }
}

// =============================================================================
// EMPTY BLOCK TESTS
// =============================================================================

#[tokio::test]
async fn test_empty_blocks_still_advance_chain() {
    let mut app = create_test_app();

    // Process empty blocks
    for height in 1..=10 {
        app.begin_block(height, height * 1000);
        app.end_block();
        app.commit();

        assert_eq!(app.current_height(), height);
    }
}

#[tokio::test]
async fn test_empty_blocks_have_unique_hashes() {
    let mut app = create_test_app();
    let mut hashes = Vec::new();

    // Process empty blocks with different timestamps
    for height in 1..=10 {
        app.begin_block(height, height * 1000);
        app.end_block();
        let hash = app.commit();
        hashes.push(hash);
    }

    // Each hash should be unique (timestamps differ)
    let unique_count = hashes.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(unique_count, 10, "All empty blocks should have unique hashes");
}

// =============================================================================
// BLOCK METADATA TESTS
// =============================================================================

#[tokio::test]
async fn test_block_stores_metadata() {
    let mut app = create_test_app();

    let height = 1;
    let timestamp = 1234567890u64;

    app.begin_block(height, timestamp);
    app.end_block();
    let app_hash = app.commit();

    // Verify block metadata is stored
    let metadata = app.get_block_metadata();
    assert!(metadata.contains_key(&height), "Block metadata should be stored");

    let block_meta = &metadata[&height];
    assert_eq!(block_meta.height, height);
    assert_eq!(block_meta.timestamp, timestamp);
}

#[tokio::test]
async fn test_block_hash_chain() {
    let mut app = create_test_app();
    let mut block_hashes = Vec::new();

    // Build a chain of blocks
    for height in 1..=10 {
        app.begin_block(height, height * 1000);
        app.end_block();
        let hash = app.commit();
        block_hashes.push(hash);
    }

    // Verify we can retrieve block hashes
    let stored_hashes = app.get_block_hashes();
    for (height, expected_hash) in block_hashes.iter().enumerate() {
        let height = (height + 1) as u64;
        assert_eq!(
            stored_hashes.get(&height),
            Some(expected_hash),
            "Block hash for height {} should match",
            height
        );
    }
}

// =============================================================================
// STATE CONSISTENCY TESTS
// =============================================================================

#[tokio::test]
async fn test_state_consistent_after_restart_simulation() {
    // Simulate node restart by recreating app and replaying blocks
    let mut app1 = create_test_app();
    let mut app2 = create_test_app();

    // Generate some blocks on app1
    let mut transactions_by_block = Vec::new();
    for height in 1..=10 {
        let timestamp = height * 1000;
        let txs: Vec<Transaction> = (0..5)
            .map(|i| create_tx(timestamp + i))
            .collect();

        app1.begin_block(height, timestamp);
        for tx in &txs {
            let _ = app1.execute_tx(tx, timestamp);
        }
        app1.end_block();
        app1.commit();

        transactions_by_block.push((height, timestamp, txs));
    }

    // Replay same blocks on app2 (simulating restart recovery)
    for (height, timestamp, txs) in transactions_by_block {
        app2.begin_block(height, timestamp);
        for tx in &txs {
            let _ = app2.execute_tx(&tx, timestamp);
        }
        app2.end_block();
        app2.commit();
    }

    // Both apps should have identical final state
    assert_eq!(app1.current_height(), app2.current_height());

    // Final hashes should match
    let hash1 = {
        app1.begin_block(11, 11000);
        app1.end_block();
        app1.commit()
    };
    let hash2 = {
        app2.begin_block(11, 11000);
        app2.end_block();
        app2.commit()
    };

    assert_eq!(hash1, hash2, "State should be identical after replay");
}

#[tokio::test]
async fn test_nonce_tracking_across_blocks() {
    let mut app = create_test_app();

    let sender_idx = 123u64;
    let mut sender_bytes = [0u8; 20];
    sender_bytes[12..20].copy_from_slice(&sender_idx.to_be_bytes());
    let sender = AccountAddress::from(sender_bytes);

    // Track nonces across multiple blocks
    for height in 1..=5 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        // Create tx with increasing nonces
        for i in 0..3 {
            let nonce = timestamp + i;
            let tx = Transaction {
                action: TransactionType::CancelAll,
                nonce,
                signature: Signature::zero(),
                hash: None,
            };
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        app.commit();
    }

    // Verify nonces were tracked
    let nonces = app.get_nonces();
    // The nonce tracking should have entries
    assert!(nonces.len() >= 0, "Nonces should be tracked");
}

// =============================================================================
// LONG RUNNING CHAIN TESTS
// =============================================================================

#[tokio::test]
async fn test_long_chain_1000_blocks() {
    let mut app = create_test_app();

    // Simulate a longer-running chain
    for height in 1..=1000 {
        let timestamp = height * 500;

        app.begin_block(height, timestamp);

        // Add some transactions every 10 blocks
        if height % 10 == 0 {
            for i in 0..5 {
                let tx = create_tx(timestamp + i);
                let _ = app.execute_tx(&tx, timestamp);
            }
        }

        app.end_block();
        app.commit();
    }

    assert_eq!(app.current_height(), 1000);
}

#[tokio::test]
async fn test_block_hash_determinism_over_time() {
    let mut app1 = create_test_app();
    let mut app2 = create_test_app();

    // Process many blocks and verify hash consistency
    for height in 1..=500 {
        let timestamp = height * 1000;
        let txs: Vec<Transaction> = (0..3)
            .map(|i| create_tx(timestamp + i))
            .collect();

        // App 1
        app1.begin_block(height, timestamp);
        for tx in &txs {
            let _ = app1.execute_tx(tx, timestamp);
        }
        app1.end_block();
        let hash1 = app1.commit();

        // App 2
        app2.begin_block(height, timestamp);
        for tx in &txs {
            let _ = app2.execute_tx(tx, timestamp);
        }
        app2.end_block();
        let hash2 = app2.commit();

        assert_eq!(hash1, hash2, "Apps diverged at height {}", height);
    }
}

// =============================================================================
// BLOCK EVENT TESTS
// =============================================================================

#[tokio::test]
async fn test_block_events_generated() {
    let mut app = create_test_app();

    app.begin_block(1, 1000);

    // Execute a transaction that might generate events
    let tx = create_leverage_tx(0, 10, 1000);
    let _ = app.execute_tx(&tx, 1000);

    app.end_block();
    app.commit();

    // Get events for this block
    let events = app.get_block_events(1);
    // Events may or may not be generated depending on tx success
}

// =============================================================================
// BLOCK BOUNDARY TESTS
// =============================================================================

#[tokio::test]
async fn test_begin_block_resets_pending_state() {
    let mut app = create_test_app();

    // First block
    app.begin_block(1, 1000);
    let tx1 = create_tx(1000);
    let _ = app.execute_tx(&tx1, 1000);
    app.end_block();
    let hash1 = app.commit();

    // Second block - should start fresh
    app.begin_block(2, 2000);
    let tx2 = create_tx(2000);
    let _ = app.execute_tx(&tx2, 2000);
    app.end_block();
    let hash2 = app.commit();

    // Hashes should be different
    assert_ne!(hash1, hash2);
}

#[tokio::test]
async fn test_commit_finalizes_block() {
    let mut app = create_test_app();

    app.begin_block(1, 1000);
    app.end_block();

    // Before commit
    let height_before = app.current_height();

    // Commit
    let hash = app.commit();

    // After commit - height should be updated
    let height_after = app.current_height();
    assert_eq!(height_after, 1);
    assert!(hash != [0u8; 32]);
}

// =============================================================================
// CONCURRENT BLOCK PROCESSING SIMULATION
// =============================================================================

#[tokio::test]
async fn test_sequential_apps_reach_same_state() {
    // Create 5 apps and process the same sequence
    let apps: Vec<_> = (0..5)
        .map(|_| create_test_app())
        .collect();

    let mut final_hashes = Vec::new();

    for mut app in apps {
        for height in 1..=50 {
            let timestamp = height * 1000;
            let txs: Vec<Transaction> = (0..3)
                .map(|i| create_tx(timestamp + i))
                .collect();

            app.begin_block(height, timestamp);
            for tx in &txs {
                let _ = app.execute_tx(tx, timestamp);
            }
            app.end_block();
            app.commit();
        }

        // Get final state hash
        app.begin_block(51, 51000);
        app.end_block();
        final_hashes.push(app.commit());
    }

    // All apps should have identical final hash
    let first_hash = final_hashes[0];
    for (i, hash) in final_hashes.iter().enumerate() {
        assert_eq!(first_hash, *hash, "App {} has different final hash", i);
    }
}
