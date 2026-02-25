//! Cross-Layer EVM ↔ Trading State Integration Tests
//!
//! These tests verify that EVM state and perpetual trading state
//! interact correctly within the same block and across the AppHash.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use hypercore_engine::{EngineState, SpotEngine};
use hypercore_evm::EvmExecutor;
use hypercore_primitives::{
    new_shared_unified_state, AccountAddress, Decimal,
};

/// Create a test app with EVM executor wired in
fn create_app_with_evm() -> (HyperCoreApp, Arc<RwLock<EvmExecutor>>) {
    let unified_state = new_shared_unified_state();
    let engine = Arc::new(RwLock::new(EngineState::new()));
    let spot_engine = Arc::new(RwLock::new(
        SpotEngine::with_unified_state(Arc::clone(&unified_state)),
    ));

    let evm = Arc::new(RwLock::new(EvmExecutor::with_unified_state(
        Arc::clone(&engine),
        Arc::clone(&unified_state),
        1337,
    )));

    let mut app = HyperCoreApp::with_shared_state(
        Arc::clone(&unified_state),
        Arc::clone(&engine),
        Some(spot_engine),
    );
    app.state.set_evm_executor(Arc::clone(&evm));

    (app, evm)
}

/// Create a test app without EVM executor
fn create_app_without_evm() -> HyperCoreApp {
    let unified_state = new_shared_unified_state();
    let engine = Arc::new(RwLock::new(EngineState::new()));
    let spot_engine = Arc::new(RwLock::new(
        SpotEngine::with_unified_state(Arc::clone(&unified_state)),
    ));

    HyperCoreApp::with_shared_state(unified_state, engine, Some(spot_engine))
}

/// Run a full block cycle: begin_block → end_block → commit
fn run_block(app: &mut HyperCoreApp, height: u64, timestamp: u64) -> [u8; 32] {
    app.begin_block(height, timestamp);
    app.end_block();
    app.commit()
}

// =============================================================================
// EVM STATE IN APPHASH TESTS
// =============================================================================

#[tokio::test]
async fn test_evm_state_included_in_app_hash() {
    // Two apps: one with EVM, one without. After same operations, AppHash should differ.
    let (mut app_with_evm, _evm) = create_app_with_evm();
    let mut app_without_evm = create_app_without_evm();

    let user = AccountAddress::from([0x42u8; 20]);

    // Credit same balance to both
    {
        let mut unified = app_with_evm.state.unified_state.write().unwrap();
        unified.credit(user, 0, Decimal::from_raw(100_000_000, 6));
    }
    {
        let mut unified = app_without_evm.state.unified_state.write().unwrap();
        unified.credit(user, 0, Decimal::from_raw(100_000_000, 6));
    }

    // Run a block on both (begin_block → end_block → commit)
    let hash_with = run_block(&mut app_with_evm, 1, 1000);
    let hash_without = run_block(&mut app_without_evm, 1, 1000);

    // Even with no EVM transactions, the EVM executor presence changes the hash
    // because the EVM state root (empty state) is included
    assert_ne!(
        hash_with, hash_without,
        "AppHash should differ when EVM executor is present vs absent"
    );
}

#[tokio::test]
async fn test_evm_state_changes_affect_app_hash() {
    let (mut app, evm) = create_app_with_evm();

    let user = AccountAddress::from([0x42u8; 20]);
    {
        let mut unified = app.state.unified_state.write().unwrap();
        unified.credit(user, 0, Decimal::from_raw(100_000_000, 6));
    }

    // Block 1: Empty EVM
    let hash1 = run_block(&mut app, 1, 1000);

    // Modify EVM state directly
    {
        let mut executor = evm.write().await;
        let addr_bytes = [0x01u8; 20];
        let addr = hypercore_evm::EvmState::address_from_bytes(&addr_bytes);
        executor.state_mut().set_nonce(addr, 5);
    }

    // Block 2: EVM has changed
    let hash2 = run_block(&mut app, 2, 2000);

    assert_ne!(hash1, hash2, "AppHash should change when EVM state changes");
}

// =============================================================================
// CROSS-LAYER VIEW TRANSFER TESTS
// =============================================================================

#[tokio::test]
async fn test_view_transfer_reflected_in_both_layers() {
    // Verify that a view transfer (core→evm) via UnifiedState updates both views
    let (mut app, _evm) = create_app_with_evm();

    let user = AccountAddress::from([0x42u8; 20]);

    // Credit user in core view
    {
        let mut unified = app.state.unified_state.write().unwrap();
        unified.credit(user, 0, Decimal::from_raw(100_000_000, 6)); // 100 USDC
    }

    // Transfer 50 USDC from core to EVM view
    {
        let mut unified = app.state.unified_state.write().unwrap();
        let amount = Decimal::from_raw(50_000_000, 6);
        let result = unified.transfer_to_evm_view(user, 0, amount);
        assert!(result.is_ok(), "Transfer to EVM should succeed");
    }

    // Run a block
    let hash = run_block(&mut app, 1, 1000);
    assert_ne!(hash, [0u8; 32], "AppHash should be non-zero");

    // Verify both views reflect the transfer
    {
        let unified = app.state.unified_state.read().unwrap();
        let balance = unified.get_balance(user, 0);
        assert!(balance.is_some(), "User should have balance");
        let bal = balance.unwrap();
        assert_eq!(bal.core_view, Decimal::from_raw(50_000_000, 6), "Core view should be 50");
        assert_eq!(bal.evm_view, Decimal::from_raw(50_000_000, 6), "EVM view should be 50");
        assert_eq!(bal.total, Decimal::from_raw(100_000_000, 6), "Total should still be 100");
    }
}

// =============================================================================
// PERP TX AND EVM STATE IN SAME BLOCK
// =============================================================================

#[tokio::test]
async fn test_evm_and_perp_operations_same_block() {
    // Run both EVM state changes and perp-layer state changes in the same block,
    // verify both are reflected in the final state and AppHash
    let (mut app, evm) = create_app_with_evm();

    let user = AccountAddress::from([0x42u8; 20]);

    // Credit user
    {
        let mut unified = app.state.unified_state.write().unwrap();
        unified.credit(user, 0, Decimal::from_raw(100_000_000, 6));
    }

    // Modify EVM state (simulate a contract deployment setting a nonce)
    {
        let mut executor = evm.write().await;
        let addr = hypercore_evm::EvmState::address_from_bytes(&[0x42u8; 20]);
        executor.state_mut().set_nonce(addr, 1);
    }

    // Transfer some funds to EVM view (simulates perp-layer activity + EVM activity in one block)
    {
        let mut unified = app.state.unified_state.write().unwrap();
        let amount = Decimal::from_raw(25_000_000, 6);
        unified.transfer_to_evm_view(user, 0, amount).unwrap();
    }

    // Run a block
    let hash = run_block(&mut app, 1, 1000);

    // Hash should be non-zero (state is not empty)
    assert_ne!(hash, [0u8; 32]);

    // Both EVM and trading state should be reflected
    {
        let unified = app.state.unified_state.read().unwrap();
        let balance = unified.get_balance(user, 0).unwrap();
        assert_eq!(balance.evm_view, Decimal::from_raw(25_000_000, 6));
        assert_eq!(balance.core_view, Decimal::from_raw(75_000_000, 6));
    }
    {
        let executor = evm.read().await;
        let addr = hypercore_evm::EvmState::address_from_bytes(&[0x42u8; 20]);
        assert_eq!(executor.state().get_nonce(&addr), 1);
    }
}
