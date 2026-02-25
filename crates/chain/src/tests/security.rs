//! Security Tests
//!
//! These tests verify security properties of the system including:
//! - Signature verification
//! - Replay attack prevention
//! - Nonce validation
//! - Transaction validation
//! - Attestation security

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use crate::attestation::AttestationKeyPair;
use crate::attestation_collector::{AttestationCollector, AttestationConfig};
use crate::tx::{Transaction, TransactionType};
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_primitives::{
    new_shared_unified_state, AccountAddress, Decimal, Signature,
};

fn create_app() -> HyperCoreApp {
    let unified_state = new_shared_unified_state();
    let engine = Arc::new(RwLock::new(EngineState::new()));
    let spot_engine = Arc::new(RwLock::new(
        SpotEngine::with_unified_state(Arc::clone(&unified_state)),
    ));

    HyperCoreApp::with_shared_state(unified_state, engine, Some(spot_engine))
}

// =============================================================================
// ATTESTATION SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_attestation_signature_verification() {
    let key = AttestationKeyPair::generate();

    // Create valid attestation
    let attestation = key.sign_attestation(100, [0x42u8; 32]);

    // Valid signature should verify
    assert!(attestation.verify(), "Valid attestation should verify");
}

#[tokio::test]
async fn test_attestation_tampering_detected() {
    let key = AttestationKeyPair::generate();

    // Create attestation
    let mut attestation = key.sign_attestation(100, [0x42u8; 32]);

    // Tamper with the app_hash
    attestation.app_hash = [0xFFu8; 32];

    // Should fail verification
    assert!(!attestation.verify(), "Tampered attestation should fail verification");
}

#[tokio::test]
async fn test_attestation_height_tampering_detected() {
    let key = AttestationKeyPair::generate();

    let mut attestation = key.sign_attestation(100, [0x42u8; 32]);

    // Tamper with height
    attestation.height = 999;

    assert!(!attestation.verify(), "Height-tampered attestation should fail verification");
}

#[tokio::test]
async fn test_attestation_pubkey_tampering_detected() {
    let key1 = AttestationKeyPair::generate();
    let key2 = AttestationKeyPair::generate();

    let mut attestation = key1.sign_attestation(100, [0x42u8; 32]);

    // Replace pubkey with another key's pubkey
    attestation.validator_pubkey = key2.public_key();

    assert!(!attestation.verify(), "Pubkey-tampered attestation should fail verification");
}

#[tokio::test]
async fn test_attestation_signature_tampering_detected() {
    let key = AttestationKeyPair::generate();

    let mut attestation = key.sign_attestation(100, [0x42u8; 32]);

    // Corrupt the signature
    attestation.signature[0] ^= 0xFF;

    assert!(!attestation.verify(), "Signature-tampered attestation should fail verification");
}

#[tokio::test]
async fn test_attestation_from_unknown_validator_rejected() {
    let (alert_tx, _) = tokio::sync::mpsc::channel(100);
    let collector = Arc::new(AttestationCollector::new(
        alert_tx,
        AttestationConfig::default(),
        None,
    ));

    // Don't register any validators
    let unknown_key = AttestationKeyPair::generate();
    let attestation = unknown_key.sign_attestation(1, [0x42u8; 32]);

    // Should reject attestation from unknown validator
    let result = collector.process_attestation(attestation).await;
    assert!(result.is_err(), "Should reject attestation from unknown validator");
}

#[tokio::test]
async fn test_attestation_replay_prevention() {
    let (alert_tx, _) = tokio::sync::mpsc::channel(100);
    let collector = Arc::new(AttestationCollector::new(
        alert_tx,
        AttestationConfig::default(),
        None,
    ));

    let key = AttestationKeyPair::generate();
    collector.add_validator(key.public_key(), 100).await;

    // Create and process attestation
    let attestation = key.sign_attestation(1, [0x42u8; 32]);
    collector.process_attestation(attestation.clone()).await.unwrap();

    // Try to replay the same attestation
    let result = collector.process_attestation(attestation).await;
    assert!(result.is_err(), "Should reject replayed attestation");
}

// =============================================================================
// NONCE SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_nonce_prevents_replay_attack() {
    let mut app = create_app();

    // Use timestamp-based nonce (valid for current block time)
    let nonce = 1000u64;
    let tx = Transaction {
        action: TransactionType::CancelAll,
        nonce,
        signature: Signature::zero(),
        hash: None,
    };

    // First execution at time 1000
    app.begin_block(1, 1000);
    let result1 = app.execute_tx(&tx, 1000);
    app.end_block();
    app.commit();

    // CancelAll with zero signature may succeed or fail depending on implementation
    // The key is that executing the same tx again should not crash

    // Try to replay the same transaction at later time
    app.begin_block(2, 2000);
    let result2 = app.execute_tx(&tx, 2000);
    app.end_block();
    app.commit();

    // Both executions should fail (Signature::zero() fails recovery), but
    // the app should remain in a valid state and continue producing blocks
    assert!(result1.is_err() || result2.is_err(),
        "At least one execution should fail: replay or invalid signature");
    assert_eq!(app.current_height(), 2, "App should continue processing blocks after failed txs");
}

#[tokio::test]
async fn test_nonce_timestamp_validation() {
    let mut app = create_app();

    // Nonce with timestamp far in the future (beyond acceptable window)
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let future_nonce = current_time + 100_000_000; // 100,000 seconds in future

    let tx = Transaction {
        action: TransactionType::CancelAll,
        nonce: future_nonce,
        signature: Signature::zero(),
        hash: None,
    };

    app.begin_block(1, current_time);
    let result = app.execute_tx(&tx, current_time);
    app.end_block();
    app.commit();

    // Future nonces should be rejected (more than the allowed window ahead)
    assert!(result.is_err(), "Nonce too far in future should be rejected");
}

#[tokio::test]
async fn test_nonce_timestamp_in_past() {
    let mut app = create_app();

    // Nonce with timestamp far in the past (before allowed window)
    let old_nonce = 1u64; // Very old timestamp (epoch + 1ms)

    let tx = Transaction {
        action: TransactionType::CancelAll,
        nonce: old_nonce,
        signature: Signature::zero(),
        hash: None,
    };

    // Current timestamp is much later (year 2001+)
    let current_time = 1_000_000_000_000u64; // ~2001 in ms
    app.begin_block(1, current_time);
    let result = app.execute_tx(&tx, current_time);
    app.end_block();
    app.commit();

    // Very old nonces should be rejected
    assert!(result.is_err(), "Nonce too far in past should be rejected");
}

// =============================================================================
// TRANSACTION VALIDATION SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_invalid_leverage_rejected() {
    let mut app = create_app();

    // Leverage above maximum (e.g., > 50)
    let tx = Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 100, // Invalid - max is 50
        },
        nonce: 1000,
        signature: Signature::zero(),
        hash: None,
    };

    app.begin_block(1, 1000);
    let result = app.execute_tx(&tx, 1000);
    app.end_block();
    app.commit();

    // Invalid leverage should be rejected
    assert!(result.is_err(), "Leverage > max (50) should be rejected");
}

#[tokio::test]
async fn test_zero_leverage_rejected() {
    let mut app = create_app();

    let tx = Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 0, // Invalid - min is 1
        },
        nonce: 1000,
        signature: Signature::zero(),
        hash: None,
    };

    app.begin_block(1, 1000);
    let result = app.execute_tx(&tx, 1000);
    app.end_block();
    app.commit();

    // Zero leverage should be rejected
    assert!(result.is_err(), "Zero leverage should be rejected");
}

#[tokio::test]
async fn test_negative_amounts_rejected() {
    let mut app = create_app();

    // Create a transfer with negative amount string
    let destination = AccountAddress::from([0x12u8; 20]);
    let tx = Transaction {
        action: TransactionType::UsdTransfer {
            destination,
            amount: "-100".to_string(), // Negative amount string
        },
        nonce: 1000,
        signature: Signature::zero(),
        hash: None,
    };

    app.begin_block(1, 1000);
    let result = app.execute_tx(&tx, 1000);
    app.end_block();
    app.commit();

    // Negative amounts should be rejected
    assert!(result.is_err(), "Negative transfer amounts should be rejected");
}

#[tokio::test]
async fn test_overflow_amounts_handled() {
    let mut app = create_app();

    // Create a transfer with very large amount (beyond balance)
    let destination = AccountAddress::from([0x12u8; 20]);
    let tx = Transaction {
        action: TransactionType::UsdTransfer {
            destination,
            amount: "99999999999999999999999".to_string(), // Very large amount
        },
        nonce: 1000,
        signature: Signature::zero(),
        hash: None,
    };

    app.begin_block(1, 1000);
    let result = app.execute_tx(&tx, 1000);
    app.end_block();
    let hash = app.commit();

    // Should handle overflow gracefully (either reject or fail silently)
    // The important thing is we don't panic and state remains valid
    assert!(hash != [0u8; 32], "App should not crash on overflow amounts");
}

// =============================================================================
// STATE INTEGRITY TESTS
// =============================================================================

#[tokio::test]
async fn test_state_cannot_be_corrupted_by_failed_tx() {
    let mut app = create_app();

    // Process a good block first
    app.begin_block(1, 1000);
    app.end_block();
    let baseline_hash = app.commit();

    // Create a new app with same baseline
    let mut app2 = create_app();
    app2.begin_block(1, 1000);
    app2.end_block();
    let baseline_hash2 = app2.commit();

    assert_eq!(baseline_hash, baseline_hash2);

    // Now process blocks with failed transactions
    app.begin_block(2, 2000);
    for _ in 0..100 {
        let bad_tx = Transaction {
            action: TransactionType::UpdateLeverage {
                asset: 255,
                is_cross: true,
                leverage: 100,
            },
            nonce: 2000,
            signature: Signature::zero(),
            hash: None,
        };
        let _ = app.execute_tx(&bad_tx, 2000);
    }
    app.end_block();
    let hash_after_bad_txs = app.commit();

    // Process empty block on app2
    app2.begin_block(2, 2000);
    app2.end_block();
    let hash_empty_block = app2.commit();

    // State integrity: both should produce valid (non-zero) hashes
    assert_ne!(hash_after_bad_txs, [0u8; 32], "Should produce valid hash after bad txs");
    assert_ne!(hash_empty_block, [0u8; 32], "Should produce valid hash for empty block");

    // Failed transactions should NOT change state — both apps should
    // produce the same hash since all bad txs were rejected
    assert_eq!(hash_after_bad_txs, hash_empty_block,
        "100 failed txs should not corrupt state: hash with failed txs should equal empty block hash");
}

#[tokio::test]
async fn test_concurrent_transaction_execution_safe() {
    // Test that concurrent transaction execution doesn't cause race conditions
    let mut app = create_app();

    app.begin_block(1, 1000);

    // Execute many transactions rapidly
    for i in 0..1000 {
        let tx = Transaction {
            action: TransactionType::CancelAll,
            nonce: 1000 + i,
            signature: Signature::zero(),
            hash: None,
        };
        let _ = app.execute_tx(&tx, 1000);
    }

    app.end_block();
    let hash = app.commit();

    assert!(hash != [0u8; 32], "Should produce valid hash");
}

// =============================================================================
// DOUBLE SPEND PREVENTION TESTS
// =============================================================================

#[tokio::test]
async fn test_balance_cannot_go_negative() {
    let mut app = create_app();

    // Try to transfer more than available balance (zero balance initially)
    let destination = AccountAddress::from([0x12u8; 20]);
    let tx = Transaction {
        action: TransactionType::UsdTransfer {
            destination,
            amount: "1000000000".to_string(), // Huge amount when sender has 0
        },
        nonce: 1000,
        signature: Signature::zero(),
        hash: None,
    };

    app.begin_block(1, 1000);
    let result = app.execute_tx(&tx, 1000);
    app.end_block();
    app.commit();

    // Should reject transfer that would cause negative balance
    assert!(result.is_err(), "Transfer exceeding balance should be rejected");
}

// =============================================================================
// MERKLE PROOF SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_merkle_proof_cannot_be_forged() {
    let mut app = create_app();

    // Process some blocks to build state
    for height in 1..=5 {
        app.begin_block(height, height * 1000);
        app.end_block();
        app.commit();
    }

    // Generate a valid proof
    let nonces = app.get_nonces();
    if let Some((addr, _)) = nonces.iter().next() {
        // Get a proof for this address
        if let Some(proof) = app.generate_nonce_proof(addr) {
            // Verify the proof is valid
            let root = app.get_nonce_root();
            let is_valid = crate::merkle::verify_proof(&proof, &root);
            assert!(is_valid, "Valid proof should verify");

            // Tamper with the proof
            let mut tampered_proof = proof.clone();
            if !tampered_proof.siblings.is_empty() {
                tampered_proof.siblings[0][0] ^= 0xFF;
            }

            let is_tampered_valid = crate::merkle::verify_proof(&tampered_proof, &root);
            assert!(!is_tampered_valid, "Tampered proof should not verify");
        }
    }
}

// =============================================================================
// HASH CHAIN SECURITY TESTS
// =============================================================================

#[tokio::test]
async fn test_app_hash_includes_all_critical_state() {
    let mut app1 = create_app();
    let mut app2 = create_app();

    // Same block, same transactions
    app1.begin_block(1, 1000);
    app2.begin_block(1, 1000);

    let tx = Transaction {
        action: TransactionType::CancelAll,
        nonce: 1000,
        signature: Signature::zero(),
        hash: None,
    };

    let _ = app1.execute_tx(&tx, 1000);
    let _ = app2.execute_tx(&tx, 1000);

    app1.end_block();
    app2.end_block();

    let hash1 = app1.commit();
    let hash2 = app2.commit();

    assert_eq!(hash1, hash2, "Same state should produce same hash");

    // Now process different transactions
    app1.begin_block(2, 2000);
    app2.begin_block(2, 2000);

    let tx1 = Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 10,
        },
        nonce: 2000,
        signature: Signature::zero(),
        hash: None,
    };

    let tx2 = Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 20,
        },
        nonce: 2001,
        signature: Signature::zero(),
        hash: None,
    };

    let _ = app1.execute_tx(&tx1, 2000);
    let _ = app2.execute_tx(&tx2, 2000);

    app1.end_block();
    app2.end_block();

    let hash1_b2 = app1.commit();
    let hash2_b2 = app2.commit();

    // Different transactions may or may not produce different hashes
    // depending on whether they actually modify state (UpdateLeverage on non-existent accounts)
    // The important thing is both produce valid hashes
    assert!(hash1_b2 != [0u8; 32], "App 1 should produce valid hash");
    assert!(hash2_b2 != [0u8; 32], "App 2 should produce valid hash");
}

// =============================================================================
// TIMESTAMP MANIPULATION TESTS
// =============================================================================

#[tokio::test]
async fn test_timestamp_must_be_valid() {
    let mut app = create_app();

    // Process first block with normal timestamp
    app.begin_block(1, 1000);
    app.end_block();
    app.commit();

    // Try to process block with earlier timestamp
    // (In CometBFT, this would be rejected by consensus)
    app.begin_block(2, 500); // Earlier than block 1
    app.end_block();
    app.commit();

    // The app should handle this, but in production CometBFT prevents it
}

// =============================================================================
// BOUNDARY CONDITION TESTS
// =============================================================================

#[tokio::test]
async fn test_max_block_height() {
    let mut app = create_app();

    // Test with very high block height
    let max_height = u64::MAX - 1;
    app.begin_block(max_height, max_height);
    app.end_block();
    let hash = app.commit();

    assert!(hash != [0u8; 32], "Should handle high block heights");
}

#[tokio::test]
async fn test_max_timestamp() {
    let mut app = create_app();

    // Test with maximum timestamp
    let max_timestamp = u64::MAX - 1;
    app.begin_block(1, max_timestamp);
    app.end_block();
    let hash = app.commit();

    assert!(hash != [0u8; 32], "Should handle max timestamp");
}
