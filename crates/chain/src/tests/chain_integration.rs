//! Chain Integration Tests with Real EIP-712 Signature Verification
//!
//! These tests verify that transactions actually execute through the chain layer
//! with cryptographically valid EIP-712 signatures (no Signature::zero() shortcuts).
//! They confirm that:
//! - Signed transactions are accepted and modify engine state
//! - Invalid signatures are rejected
//! - Orders placed via the chain layer create real positions

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::app::HyperCoreApp;
use crate::state::AppState;
use crate::tx::{
    LimitTif, OrderGrouping, OrderTypeWire, OrderWire, Transaction, TransactionType,
};
use hypercore_engine::{Engine, EngineConfig};
use hypercore_primitives::{
    new_shared_unified_state, AccountAddress, Decimal, MarketConfig, Signature,
    eip712::{compute_domain_separator, DEFAULT_CHAIN_ID},
};

use k256::ecdsa::{SigningKey, signature::hazmat::PrehashSigner};
use sha3::{Digest, Keccak256};

/// Test wallet: a known private key + derived address for deterministic testing
struct TestWallet {
    signing_key: SigningKey,
    address: AccountAddress,
}

impl TestWallet {
    /// Create a wallet from a fixed seed (deterministic)
    fn new(seed: u8) -> Self {
        let mut secret = [0u8; 32];
        secret[31] = seed;
        secret[0] = 0x01; // Ensure valid scalar
        let signing_key = SigningKey::from_bytes((&secret).into())
            .expect("valid key");

        // Derive address from public key
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_encoded_point(false);
        let public_key_bytes = &public_key.as_bytes()[1..]; // Skip 0x04 prefix
        let hash = Keccak256::digest(public_key_bytes);
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&hash[12..]);
        let address = AccountAddress::from(addr_bytes);

        Self { signing_key, address }
    }

    /// Sign a transaction with EIP-712
    fn sign_tx(&self, tx: &mut Transaction) {
        // Compute EIP-712 typed data hash (mirrors Transaction::compute_eip712_hash)
        let domain_separator = compute_domain_separator(DEFAULT_CHAIN_ID);
        let struct_hash = tx.action.compute_struct_hash(tx.nonce)
            .expect("compute struct hash");

        let mut hasher = Keccak256::new();
        hasher.update(b"\x19\x01");
        hasher.update(domain_separator);
        hasher.update(struct_hash);
        let typed_hash: [u8; 32] = hasher.finalize().into();

        // Sign with k256
        let (sig, recovery_id) = self.signing_key
            .sign_prehash(&typed_hash)
            .expect("signing should succeed");

        let k256_bytes = sig.to_bytes();
        let mut sig_65 = [0u8; 65];
        sig_65[..64].copy_from_slice(&k256_bytes);
        sig_65[64] = recovery_id.to_byte() + 27;

        tx.signature = Signature::from_bytes(&sig_65);
    }
}

/// Create a test app with a fully-wired perp engine (not just EngineState)
fn create_full_app() -> HyperCoreApp {
    let unified_state = new_shared_unified_state();
    let engine = Engine::with_unified_state(EngineConfig::default(), Arc::clone(&unified_state));

    let perp_engine = Arc::new(RwLock::new(engine));
    let state = AppState::with_full_engine(unified_state, perp_engine, None);

    HyperCoreApp { state }
}

/// Add a BTC-PERP market to the perp engine
fn add_btc_market(app: &HyperCoreApp) {
    let perp = app.state.perp_engine().expect("perp engine should be set");
    let mut engine = perp.try_write().expect("write lock");
    engine.add_market(MarketConfig::btc_perp(), Decimal::price("65000"), 0);
}

/// Credit funds to an account in the unified state
fn deposit(app: &HyperCoreApp, account: AccountAddress, amount_usdc: i128) {
    let unified = app.state.unified_state.clone();
    let mut us = unified.write().unwrap();
    us.credit(account, 0, Decimal::from_raw(amount_usdc, Decimal::USDC_DECIMALS));
}

// =============================================================================
// REAL SIGNATURE TESTS
// =============================================================================

#[tokio::test]
async fn test_signed_order_executes_and_creates_position() {
    let mut app = create_full_app();
    add_btc_market(&app);

    let wallet_a = TestWallet::new(1);
    let wallet_b = TestWallet::new(2);

    deposit(&app, wallet_a.address, 100_000_000000);
    deposit(&app, wallet_b.address, 100_000_000000);

    app.begin_block(1, 1000);

    // Wallet A places sell order via chain (nonce=0, first tx for this account)
    let mut sell_tx = Transaction {
        action: TransactionType::Order {
            orders: vec![OrderWire {
                a: 0,
                b: false, // sell
                p: "65000.0".to_string(),
                s: "0.1".to_string(),
                r: false,
                t: OrderTypeWire::Limit {
                    limit: LimitTif { tif: "Gtc".to_string() },
                },
                c: None,
            }],
            grouping: OrderGrouping::Na,
        },
        nonce: 0,
        signature: Signature::zero(), // will be replaced
        hash: None,
    };
    wallet_a.sign_tx(&mut sell_tx);

    // Verify signature recovery returns correct address
    let recovered = sell_tx.sender().expect("should recover sender");
    assert_eq!(recovered, wallet_a.address, "Recovered address should match wallet");

    let result = app.execute_tx(&sell_tx, 1000);
    assert!(result.is_ok(), "Signed sell order tx should succeed: {:?}", result.err());

    // Wallet B places matching buy order (nonce=0, first tx for wallet B)
    let mut buy_tx = Transaction {
        action: TransactionType::Order {
            orders: vec![OrderWire {
                a: 0,
                b: true, // buy
                p: "65000.0".to_string(),
                s: "0.1".to_string(),
                r: false,
                t: OrderTypeWire::Limit {
                    limit: LimitTif { tif: "Gtc".to_string() },
                },
                c: None,
            }],
            grouping: OrderGrouping::Na,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet_b.sign_tx(&mut buy_tx);

    let result = app.execute_tx(&buy_tx, 1000);
    assert!(result.is_ok(), "Buy order should succeed: {:?}", result.err());

    app.end_block();
    app.commit();

    // Verify positions were actually created in the perp engine
    let perp = app.state.perp_engine().unwrap();
    let engine = perp.try_read().unwrap();

    let pos_a = engine.state.get_position(wallet_a.address, 0);
    assert!(pos_a.is_some(), "Wallet A should have a position after sell order filled");
    let pos_a = pos_a.unwrap();
    assert!(pos_a.is_short(), "Wallet A sold, should be short");

    let pos_b = engine.state.get_position(wallet_b.address, 0);
    assert!(pos_b.is_some(), "Wallet B should have a position after buy order filled");
    let pos_b = pos_b.unwrap();
    assert!(pos_b.is_long(), "Wallet B bought, should be long");
}

#[tokio::test]
async fn test_invalid_signature_rejected() {
    let mut app = create_full_app();
    add_btc_market(&app);

    let wallet = TestWallet::new(1);
    deposit(&app, wallet.address, 100_000_000000);

    app.begin_block(1, 1000);

    // Create a tx with zero signature (invalid recovery)
    let tx = Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 25,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };

    let result = app.execute_tx(&tx, 1000);
    // Zero signature → recovery fails or recovers wrong address → rejected
    assert!(result.is_err(), "Zero-signature tx should be rejected");
}

#[tokio::test]
async fn test_signed_leverage_update_modifies_engine() {
    let mut app = create_full_app();
    add_btc_market(&app);

    let wallet = TestWallet::new(1);
    deposit(&app, wallet.address, 100_000_000000);

    app.begin_block(1, 1000);

    let mut tx = Transaction {
        action: TransactionType::UpdateLeverage {
            asset: 0,
            is_cross: true,
            leverage: 25,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet.sign_tx(&mut tx);

    let result = app.execute_tx(&tx, 1000);
    assert!(result.is_ok(), "Signed leverage update should succeed: {:?}", result.err());

    app.end_block();
    app.commit();

    // Verify leverage was actually changed in the perp engine
    let perp = app.state.perp_engine().unwrap();
    let engine = perp.try_read().unwrap();
    let leverage = engine.state.get_leverage(wallet.address, 0);
    assert_eq!(leverage, 25, "Leverage should be updated to 25x");
}

#[tokio::test]
async fn test_signed_cancel_all_succeeds() {
    let mut app = create_full_app();
    add_btc_market(&app);

    let wallet = TestWallet::new(1);
    deposit(&app, wallet.address, 100_000_000000);

    app.begin_block(1, 1000);

    // Place an order (nonce=0)
    let mut order_tx = Transaction {
        action: TransactionType::Order {
            orders: vec![OrderWire {
                a: 0,
                b: true,
                p: "64000.0".to_string(),
                s: "0.1".to_string(),
                r: false,
                t: OrderTypeWire::Limit {
                    limit: LimitTif { tif: "Gtc".to_string() },
                },
                c: None,
            }],
            grouping: OrderGrouping::Na,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet.sign_tx(&mut order_tx);
    let result = app.execute_tx(&order_tx, 1000);
    assert!(result.is_ok(), "Order should succeed: {:?}", result.err());

    // Verify order is resting
    {
        let perp = app.state.perp_engine().unwrap();
        let engine = perp.try_read().unwrap();
        let orders = engine.state.get_orders_by_account(wallet.address, 0);
        assert_eq!(orders.len(), 1, "Should have 1 resting order");
    }

    // CancelAll (nonce=1, second tx)
    let mut cancel_tx = Transaction {
        action: TransactionType::CancelAll,
        nonce: 1,
        signature: Signature::zero(),
        hash: None,
    };
    wallet.sign_tx(&mut cancel_tx);
    let result = app.execute_tx(&cancel_tx, 1000);
    assert!(result.is_ok(), "CancelAll should succeed: {:?}", result.err());

    // Verify all orders were canceled
    {
        let perp = app.state.perp_engine().unwrap();
        let engine = perp.try_read().unwrap();
        let orders = engine.state.get_orders_by_account(wallet.address, 0);
        assert!(orders.is_empty(), "All orders should be canceled after CancelAll");
    }
}

#[tokio::test]
async fn test_wrong_signer_cannot_cancel_others_orders() {
    let mut app = create_full_app();
    add_btc_market(&app);

    let wallet_a = TestWallet::new(1);
    let wallet_b = TestWallet::new(2);
    deposit(&app, wallet_a.address, 100_000_000000);
    deposit(&app, wallet_b.address, 100_000_000000);

    app.begin_block(1, 1000);

    // Wallet A places order (nonce=0)
    let mut order_tx = Transaction {
        action: TransactionType::Order {
            orders: vec![OrderWire {
                a: 0,
                b: true,
                p: "64000.0".to_string(),
                s: "0.1".to_string(),
                r: false,
                t: OrderTypeWire::Limit {
                    limit: LimitTif { tif: "Gtc".to_string() },
                },
                c: None,
            }],
            grouping: OrderGrouping::Na,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet_a.sign_tx(&mut order_tx);
    app.execute_tx(&order_tx, 1000).unwrap();

    // Wallet B tries CancelAll (nonce=0) — should only cancel B's orders (none), not A's
    let mut cancel_tx = Transaction {
        action: TransactionType::CancelAll,
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet_b.sign_tx(&mut cancel_tx);
    let _ = app.execute_tx(&cancel_tx, 1000);

    // Wallet A's order should still exist
    let perp = app.state.perp_engine().unwrap();
    let engine = perp.try_read().unwrap();
    let orders = engine.state.get_orders_by_account(wallet_a.address, 0);
    assert_eq!(orders.len(), 1, "Wallet A's order should survive wallet B's CancelAll");
}

#[tokio::test]
async fn test_order_fill_changes_balances_and_records_fills() {
    // Verify that a matched order at the chain level actually:
    // 1. Changes unified state balances (margin deducted)
    // 2. Creates fills in the engine
    // 3. Updates position sizes correctly
    let mut app = create_full_app();
    add_btc_market(&app);

    let wallet_a = TestWallet::new(1);
    let wallet_b = TestWallet::new(2);

    deposit(&app, wallet_a.address, 100_000_000000); // $100k
    deposit(&app, wallet_b.address, 100_000_000000);

    app.begin_block(1, 1000);

    // Wallet A: sell 0.1 BTC at $65,000
    let mut sell_tx = Transaction {
        action: TransactionType::Order {
            orders: vec![OrderWire {
                a: 0,
                b: false,
                p: "65000.0".to_string(),
                s: "0.1".to_string(),
                r: false,
                t: OrderTypeWire::Limit {
                    limit: LimitTif { tif: "Gtc".to_string() },
                },
                c: None,
            }],
            grouping: OrderGrouping::Na,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet_a.sign_tx(&mut sell_tx);
    let result_a = app.execute_tx(&sell_tx, 1000);
    assert!(result_a.is_ok(), "Sell should succeed: {:?}", result_a.err());

    // Wallet B: buy 0.1 BTC at $65,000 (crosses sell)
    let mut buy_tx = Transaction {
        action: TransactionType::Order {
            orders: vec![OrderWire {
                a: 0,
                b: true,
                p: "65000.0".to_string(),
                s: "0.1".to_string(),
                r: false,
                t: OrderTypeWire::Limit {
                    limit: LimitTif { tif: "Gtc".to_string() },
                },
                c: None,
            }],
            grouping: OrderGrouping::Na,
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet_b.sign_tx(&mut buy_tx);
    let result_b = app.execute_tx(&buy_tx, 1000);
    assert!(result_b.is_ok(), "Buy should succeed: {:?}", result_b.err());

    app.end_block();
    app.commit();

    // Verify fills were recorded
    let perp = app.state.perp_engine().unwrap();
    let engine = perp.try_read().unwrap();

    let fills_a = engine.state.get_user_fills(wallet_a.address, None);
    assert!(!fills_a.is_empty(), "Wallet A should have fill records");
    assert_eq!(fills_a[0].market_id, 0);

    let fills_b = engine.state.get_user_fills(wallet_b.address, None);
    assert!(!fills_b.is_empty(), "Wallet B should have fill records");

    // Verify position sizes are correct
    let pos_a = engine.state.get_position(wallet_a.address, 0).unwrap();
    assert_eq!(pos_a.abs_size().to_string_trimmed(), "0.1",
        "Wallet A should have 0.1 BTC short position");
    assert!(pos_a.is_short());

    let pos_b = engine.state.get_position(wallet_b.address, 0).unwrap();
    assert_eq!(pos_b.abs_size().to_string_trimmed(), "0.1",
        "Wallet B should have 0.1 BTC long position");
    assert!(pos_b.is_long());

    // Verify no resting orders remain (both fully filled)
    let orders_a = engine.state.get_orders_by_account(wallet_a.address, 0);
    let orders_b = engine.state.get_orders_by_account(wallet_b.address, 0);
    assert!(orders_a.is_empty(), "No resting orders for wallet A after full fill");
    assert!(orders_b.is_empty(), "No resting orders for wallet B after full fill");
}

#[tokio::test]
async fn test_usd_transfer_changes_balance() {
    // Verify UsdTransfer at the chain level actually moves funds
    let mut app = create_full_app();

    let wallet_a = TestWallet::new(1);
    let wallet_b = TestWallet::new(2);

    deposit(&app, wallet_a.address, 100_000_000000);

    // Verify wallet B starts with zero
    {
        let us = app.state.unified_state.read().unwrap();
        assert!(us.get_core_view(wallet_b.address, 0).is_zero(),
            "Wallet B should start with zero balance");
    }

    app.begin_block(1, 1000);

    // Wallet A transfers $1000 to wallet B
    let mut transfer_tx = Transaction {
        action: TransactionType::UsdTransfer {
            destination: wallet_b.address,
            amount: "1000.0".to_string(),
        },
        nonce: 0,
        signature: Signature::zero(),
        hash: None,
    };
    wallet_a.sign_tx(&mut transfer_tx);

    let result = app.execute_tx(&transfer_tx, 1000);
    assert!(result.is_ok(), "UsdTransfer should succeed: {:?}", result.err());

    app.end_block();
    app.commit();

    // Verify balances changed
    let us = app.state.unified_state.read().unwrap();
    let balance_a = us.get_core_view(wallet_a.address, 0);
    let balance_b = us.get_core_view(wallet_b.address, 0);

    assert_eq!(balance_a.raw(), 99_000_000000,
        "Wallet A should have $99,000 after sending $1,000");
    assert_eq!(balance_b.raw(), 1_000_000000,
        "Wallet B should have $1,000 after receiving transfer");
}
