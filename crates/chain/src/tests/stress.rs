//! Stress Tests
//!
//! These tests verify system behavior under high load including:
//! - High transaction throughput
//! - Large block sizes
//! - Memory pressure
//! - Sustained operation

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use crate::tx::{Transaction, TransactionType};
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{new_shared_unified_state, Decimal, Signature};

fn create_app() -> HyperCoreApp {
    let unified_state = new_shared_unified_state();
    let engine = Arc::new(RwLock::new(EngineState::new()));
    let spot_engine = Arc::new(RwLock::new(
        SpotEngine::with_unified_state(Arc::clone(&unified_state)),
    ));
    HyperCoreApp::with_shared_state(unified_state, engine, Some(spot_engine))
}

fn create_tx(nonce: u64) -> Transaction {
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
// HIGH THROUGHPUT TESTS
// =============================================================================

#[tokio::test]
async fn test_1000_txs_per_block() {
    let mut app = create_app();
    let start = Instant::now();

    app.begin_block(1, 1000);

    for i in 0..1000 {
        let tx = create_tx(1000 + i);
        let _ = app.execute_tx(&tx, 1000);
    }

    app.end_block();
    let hash = app.commit();

    let elapsed = start.elapsed();
    assert_ne!(hash, [0u8; 32], "Committed block should produce non-zero hash");
    println!("1000 txs processed in {:?}", elapsed);
}

#[tokio::test]
async fn test_5000_txs_per_block() {
    let mut app = create_app();
    let start = Instant::now();

    app.begin_block(1, 1000);

    for i in 0..5000 {
        let tx = create_tx(1000 + i);
        let _ = app.execute_tx(&tx, 1000);
    }

    app.end_block();
    let hash = app.commit();

    let elapsed = start.elapsed();
    assert_ne!(hash, [0u8; 32], "Committed block should produce non-zero hash");
    println!("5000 txs processed in {:?}", elapsed);
}

#[tokio::test]
async fn test_10000_txs_per_block() {
    let mut app = create_app();
    let start = Instant::now();

    app.begin_block(1, 1000);

    for i in 0..10000 {
        let tx = create_tx(1000 + i);
        let _ = app.execute_tx(&tx, 1000);
    }

    app.end_block();
    let hash = app.commit();

    let elapsed = start.elapsed();
    assert_ne!(hash, [0u8; 32], "Committed block should produce non-zero hash");
    println!("10000 txs processed in {:?}", elapsed);
}

// =============================================================================
// SUSTAINED LOAD TESTS
// =============================================================================

#[tokio::test]
async fn test_sustained_100_blocks_100_txs() {
    let mut app = create_app();
    let start = Instant::now();

    for height in 1..=100 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        for i in 0..100 {
            let tx = create_tx(timestamp + i);
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        app.commit();
    }

    let elapsed = start.elapsed();
    assert_eq!(app.current_height(), 100);
    println!("100 blocks x 100 txs = 10,000 total txs in {:?}", elapsed);
}

#[tokio::test]
async fn test_sustained_500_blocks() {
    let mut app = create_app();
    let start = Instant::now();

    for height in 1..=500 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        // Varying transaction count
        let tx_count = (height % 50 + 1) as u64;
        for i in 0..tx_count {
            let tx = create_tx(timestamp + i);
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        app.commit();
    }

    let elapsed = start.elapsed();
    assert_eq!(app.current_height(), 500);
    println!("500 blocks in {:?}", elapsed);
}

// =============================================================================
// MEMORY PRESSURE TESTS
// =============================================================================

#[tokio::test]
async fn test_large_state_accumulation() {
    let mut app = create_app();

    // Accumulate state over many blocks
    for height in 1..=200 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        // Each block adds some state
        for i in 0..50 {
            let tx = create_leverage_tx(timestamp + i, ((i % 49) + 1) as u8);
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        app.commit();
    }

    // Should still function correctly
    let final_height = app.current_height();
    assert_eq!(final_height, 200);
}

#[tokio::test]
async fn test_many_unique_nonces() {
    let mut app = create_app();

    app.begin_block(1, 1000);

    // Create many unique nonces
    for i in 0..10000 {
        let tx = Transaction {
            action: TransactionType::CancelAll,
            nonce: i + 1000,
            signature: Signature::zero(),
            hash: None,
        };
        let _ = app.execute_tx(&tx, 1000);
    }

    app.end_block();
    app.commit();

    // Nonce table should handle many entries
    let nonces = app.get_nonces();
    // The nonce count depends on implementation
}

// =============================================================================
// CONCURRENT STRESS TESTS
// =============================================================================

#[tokio::test]
async fn test_parallel_app_stress() {
    use std::thread;

    let handles: Vec<_> = (0..4)
        .map(|thread_id| {
            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let mut app = create_app();
                    let start = Instant::now();

                    for height in 1..=100 {
                        let timestamp = height * 1000;

                        app.begin_block(height, timestamp);

                        for i in 0..100 {
                            let tx = create_tx(timestamp + i);
                            let _ = app.execute_tx(&tx, timestamp);
                        }

                        app.end_block();
                        app.commit();
                    }

                    (thread_id, start.elapsed())
                })
            })
        })
        .collect();

    for handle in handles {
        let (id, elapsed) = handle.join().unwrap();
        println!("Thread {} completed 100 blocks in {:?}", id, elapsed);
    }
}

// =============================================================================
// SNAPSHOT/RESTORE STRESS TESTS
// =============================================================================

#[tokio::test]
async fn test_snapshot_large_state() {
    let mut app = create_app();

    // Build up large state
    for height in 1..=100 {
        let timestamp = height * 1000;

        app.begin_block(height, timestamp);

        for i in 0..100 {
            let tx = create_tx(timestamp + i);
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        app.commit();
    }

    let start = Instant::now();
    let snapshot = app.state.snapshot().await;
    let snapshot_time = start.elapsed();

    println!("Snapshot of 100 blocks in {:?}", snapshot_time);
    assert_eq!(snapshot.height, 100);
}

#[tokio::test]
async fn test_restore_large_state() {
    let mut app1 = create_app();

    // Build state
    for height in 1..=100 {
        let timestamp = height * 1000;
        app1.begin_block(height, timestamp);
        for i in 0..50 {
            let tx = create_tx(timestamp + i);
            let _ = app1.execute_tx(&tx, timestamp);
        }
        app1.end_block();
        app1.commit();
    }

    let snapshot = app1.state.snapshot().await;

    let mut app2 = create_app();
    let start = Instant::now();
    app2.state.restore(snapshot).await;
    let restore_time = start.elapsed();

    println!("Restore of 100 blocks in {:?}", restore_time);
    assert_eq!(app2.current_height(), 100);
}

// =============================================================================
// ATTESTATION STRESS TESTS
// =============================================================================

#[tokio::test]
async fn test_attestation_collector_high_volume() {
    use crate::attestation::AttestationKeyPair;
    use crate::attestation_collector::{AttestationCollector, AttestationConfig};

    let (alert_tx, _) = tokio::sync::mpsc::channel(1000);
    let collector = Arc::new(AttestationCollector::new(
        alert_tx,
        AttestationConfig::default(),
        None,
    ));

    // Add many validators
    let keys: Vec<AttestationKeyPair> = (0..100)
        .map(|_| AttestationKeyPair::generate())
        .collect();

    for key in &keys {
        collector.add_validator(key.public_key(), 100).await;
    }

    let start = Instant::now();

    // Process many attestations
    for height in 1..=100 {
        let app_hash = [height as u8; 32];
        for key in &keys {
            let attestation = key.sign_attestation(height, app_hash);
            let _ = collector.process_attestation(attestation).await;
        }
    }

    let elapsed = start.elapsed();
    println!("Processed 10,000 attestations in {:?}", elapsed);

    let stats = collector.stats().await;
    assert_eq!(stats.total_attestations, 10_000);
}

// =============================================================================
// MERKLE TREE STRESS TESTS
// =============================================================================

#[tokio::test]
async fn test_merkle_tree_large_dataset() {
    use crate::merkle::MerkleTree;

    let start = Instant::now();

    // Create large dataset
    let entries: Vec<[u8; 32]> = (0u64..10000)
        .map(|i| {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_be_bytes());
            hash
        })
        .collect();

    let tree = MerkleTree::from_leaves(entries);
    let build_time = start.elapsed();

    println!("Built Merkle tree with 10,000 leaves in {:?}", build_time);

    // Generate proofs
    let start = Instant::now();
    for i in 0..100 {
        let _proof = tree.prove(i);
    }
    let proof_time = start.elapsed();

    println!("Generated 100 proofs in {:?}", proof_time);
}

// =============================================================================
// BENCHMARK HELPERS
// =============================================================================

#[tokio::test]
async fn test_tx_throughput_measurement() {
    let mut app = create_app();

    let iterations = 5;
    let txs_per_block = 1000;
    let mut total_time = Duration::ZERO;

    for iter in 0..iterations {
        let start = Instant::now();

        app.begin_block(iter + 1, (iter + 1) * 1000);

        for i in 0..txs_per_block {
            let tx = create_tx(((iter + 1) * 1000) + i);
            let _ = app.execute_tx(&tx, (iter + 1) * 1000);
        }

        app.end_block();
        app.commit();

        total_time += start.elapsed();
    }

    let avg_time = total_time / iterations as u32;
    let throughput = txs_per_block as f64 / avg_time.as_secs_f64();

    println!(
        "Average block time: {:?}, Throughput: {:.0} tx/s",
        avg_time, throughput
    );
}

// =============================================================================
// EDGE CASE STRESS TESTS
// =============================================================================

#[tokio::test]
async fn test_empty_blocks_burst() {
    let mut app = create_app();
    let start = Instant::now();

    // Process many empty blocks rapidly
    for height in 1..=1000 {
        app.begin_block(height, height * 100);
        app.end_block();
        app.commit();
    }

    let elapsed = start.elapsed();
    assert_eq!(app.current_height(), 1000);
    println!("1000 empty blocks in {:?}", elapsed);
}

#[tokio::test]
async fn test_alternating_load() {
    let mut app = create_app();

    for height in 1..=100 {
        let timestamp = height * 1000;
        app.begin_block(height, timestamp);

        // Alternate between heavy and light blocks
        let tx_count = if height % 2 == 0 { 500 } else { 10 };

        for i in 0..tx_count {
            let tx = create_tx(timestamp + i);
            let _ = app.execute_tx(&tx, timestamp);
        }

        app.end_block();
        app.commit();
    }

    assert_eq!(app.current_height(), 100);
}
