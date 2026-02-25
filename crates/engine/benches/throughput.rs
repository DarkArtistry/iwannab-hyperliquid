//! Engine throughput benchmarks
//!
//! Measures orders/second for the full Engine pipeline including:
//! - Order placement (matching + risk checks + position updates)
//! - Order cancellation
//! - Cancel-all operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use hypercore_engine::{Engine, EngineConfig};
use hypercore_primitives::{Decimal, MarketConfig, OrderRequest, OrderSide, OrderType};
use alloy_primitives::Address;

/// Create a fresh engine with BTC-PERP market and funded accounts.
fn setup_engine(num_accounts: usize) -> Engine {
    let mut engine = Engine::new(EngineConfig::default());
    engine.add_market(MarketConfig::btc_perp(), Decimal::price("65000"), 0);

    // Deposit $100,000 for each account
    for i in 0..num_accounts {
        let account = Address::repeat_byte((i + 1) as u8);
        engine.state.deposit(account, 100_000_000000); // $100k USDC
    }

    engine
}

/// Create a limit order request at the given price and size.
fn make_order(side: OrderSide, price: &str, size: &str) -> OrderRequest {
    OrderRequest {
        market_id: 0,
        side,
        price: Decimal::price(price),
        size: Decimal::size(size),
        order_type: OrderType::default(), // Limit GTC
        reduce_only: false,
        client_order_id: None,
        trigger_price: None,
        trigger_direction: None,
    }
}

/// Benchmark: place_order throughput with alternating buy/sell at crossing prices.
///
/// This measures end-to-end order processing including matching, risk checks,
/// position updates, and fill application. Orders alternate between two accounts
/// at crossing prices so every other order produces a fill.
fn bench_place_order_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_place_order");

    for &batch_size in &[100u64, 1000, 5000] {
        group.throughput(Throughput::Elements(batch_size));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &n| {
                b.iter_with_setup(
                    || setup_engine(2),
                    |mut engine| {
                        let buyer = Address::repeat_byte(1);
                        let seller = Address::repeat_byte(2);

                        for i in 0..n {
                            if i % 2 == 0 {
                                // Seller posts an ask
                                let req = make_order(OrderSide::Sell, "65000.0", "0.001");
                                let _ = black_box(engine.place_order(seller, req, 1000 + i));
                            } else {
                                // Buyer crosses the ask
                                let req = make_order(OrderSide::Buy, "65000.0", "0.001");
                                let _ = black_box(engine.place_order(buyer, req, 1000 + i));
                            }
                        }
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: place_order with non-crossing prices (resting orders only, no fills).
///
/// This isolates the orderbook insertion path without matching overhead.
fn bench_place_order_resting(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_place_order_resting");

    for &batch_size in &[100u64, 1000, 5000] {
        group.throughput(Throughput::Elements(batch_size));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &n| {
                b.iter_with_setup(
                    || setup_engine(4),
                    |mut engine| {
                        for i in 0..n {
                            let account = Address::repeat_byte(((i % 4) + 1) as u8);
                            // Spread bids across price levels so nothing crosses
                            let price_offset = i % 500;
                            let price = format!("{}.0", 64000 + price_offset);
                            let req = make_order(OrderSide::Buy, &price, "0.001");
                            let _ = black_box(engine.place_order(account, req, 1000 + i));
                        }
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: cancel_order throughput.
///
/// Pre-populates the book with resting orders, then measures how fast
/// individual orders can be canceled.
fn bench_cancel_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_cancel_order");

    for &num_orders in &[100u64, 1000] {
        group.throughput(Throughput::Elements(num_orders));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_orders),
            &num_orders,
            |b, &n| {
                b.iter_with_setup(
                    || {
                        let mut engine = setup_engine(1);
                        let account = Address::repeat_byte(1);
                        let mut order_ids = Vec::with_capacity(n as usize);

                        for i in 0..n {
                            let price = format!("{}.0", 64000 + (i % 500));
                            let req = make_order(OrderSide::Buy, &price, "0.001");
                            if let Ok((order, _)) = engine.place_order(account, req, 1000 + i) {
                                order_ids.push(order.id);
                            }
                        }

                        (engine, order_ids)
                    },
                    |(mut engine, order_ids)| {
                        let account = Address::repeat_byte(1);
                        for oid in order_ids {
                            let _ = black_box(engine.cancel_order(account, 0, oid));
                        }
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: cancel_all_orders throughput.
///
/// Pre-populates the book with N orders per account, then measures
/// how fast cancel_all_orders completes.
fn bench_cancel_all_orders(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_cancel_all");

    for &num_orders in &[10u64, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_orders),
            &num_orders,
            |b, &n| {
                b.iter_with_setup(
                    || {
                        let mut engine = setup_engine(1);
                        let account = Address::repeat_byte(1);

                        for i in 0..n {
                            let price = format!("{}.0", 64000 + (i % 500));
                            let req = make_order(OrderSide::Buy, &price, "0.001");
                            let _ = engine.place_order(account, req, 1000 + i);
                        }

                        engine
                    },
                    |mut engine| {
                        let account = Address::repeat_byte(1);
                        let _ = black_box(engine.cancel_all_orders(account, 0));
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: mixed workload simulating realistic trading activity.
///
/// 70% new orders (mix of crossing and resting), 20% cancels, 10% cancel-all.
fn bench_mixed_workload(c: &mut Criterion) {
    c.bench_function("engine_mixed_workload_1000ops", |b| {
        b.iter_with_setup(
            || {
                let engine = setup_engine(4);
                engine
            },
            |mut engine| {
                let accounts = [
                    Address::repeat_byte(1),
                    Address::repeat_byte(2),
                    Address::repeat_byte(3),
                    Address::repeat_byte(4),
                ];
                let mut order_ids: Vec<(usize, u64)> = Vec::new(); // (account_idx, order_id)

                for i in 0u64..1000 {
                    let acct_idx = (i as usize) % 4;
                    let account = accounts[acct_idx];
                    let op = i % 10;

                    if op < 7 {
                        // 70%: place order
                        let side = if i % 2 == 0 { OrderSide::Buy } else { OrderSide::Sell };
                        let price_offset = (i % 200) as i64 - 100;
                        let price = format!("{}.0", (65000i64 + price_offset).max(60000));
                        let req = make_order(side, &price, "0.001");
                        if let Ok((order, _)) = engine.place_order(account, req, 1000 + i) {
                            order_ids.push((acct_idx, order.id));
                        }
                    } else if op < 9 {
                        // 20%: cancel a random order
                        if let Some((aidx, oid)) = order_ids.pop() {
                            let _ = black_box(engine.cancel_order(accounts[aidx], 0, oid));
                        }
                    } else {
                        // 10%: cancel all for this account
                        let _ = black_box(engine.cancel_all_orders(account, 0));
                        order_ids.retain(|(idx, _)| *idx != acct_idx);
                    }
                }
            },
        );
    });
}

criterion_group!(
    benches,
    bench_place_order_throughput,
    bench_place_order_resting,
    bench_cancel_order,
    bench_cancel_all_orders,
    bench_mixed_workload,
);

criterion_main!(benches);
