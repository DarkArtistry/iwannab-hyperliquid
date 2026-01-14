//! Benchmarks for the matching engine

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use hypercore_engine::{EngineState, MatchingEngine, OrderBook};
use hypercore_primitives::{Decimal, Market, MarketConfig, Order, OrderRequest, OrderSide, OrderType, TimeInForce};
use alloy_primitives::Address;

fn create_test_market() -> Market {
    Market::new(
        MarketConfig::btc_perp(),
        Decimal::price("65000"),
        0,
    )
}

fn create_test_order(side: OrderSide, price: &str, size: &str, id: u64) -> Order {
    Order {
        id,
        owner: Address::repeat_byte(1),
        market_id: 0,
        side,
        price: Decimal::price(price),
        size: Decimal::size(size),
        filled_size: Decimal::ZERO,
        timestamp: 0,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::GTC,
        reduce_only: false,
        client_order_id: None,
    }
}

fn benchmark_orderbook_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_insert");

    for count in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            b.iter(|| {
                let mut book = OrderBook::new();
                for i in 0..count {
                    let price = format!("{}", 65000 + (i % 1000));
                    let order = create_test_order(
                        if i % 2 == 0 { OrderSide::Buy } else { OrderSide::Sell },
                        &price,
                        "0.1",
                        i as u64,
                    );
                    book.insert(black_box(order));
                }
            });
        });
    }

    group.finish();
}

fn benchmark_orderbook_cancel(c: &mut Criterion) {
    c.bench_function("orderbook_cancel", |b| {
        let mut book = OrderBook::new();

        // Pre-populate with 1000 orders
        for i in 0..1000 {
            let price = format!("{}", 65000 + (i % 100));
            let order = create_test_order(
                if i % 2 == 0 { OrderSide::Buy } else { OrderSide::Sell },
                &price,
                "0.1",
                i,
            );
            book.insert(order);
        }

        b.iter(|| {
            // Cancel and re-add to maintain state
            let removed = book.remove(black_box(500));
            if let Some(order) = removed {
                book.insert(order);
            }
        });
    });
}

fn benchmark_l2_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("l2_snapshot");

    for depth in [10, 50, 100].iter() {
        // Create orderbook with many price levels
        let mut book = OrderBook::new();
        for i in 0..500 {
            let bid_price = format!("{}", 64500 + i);
            let ask_price = format!("{}", 65500 + i);

            book.insert(create_test_order(OrderSide::Buy, &bid_price, "1.0", i * 2));
            book.insert(create_test_order(OrderSide::Sell, &ask_price, "1.0", i * 2 + 1));
        }

        group.bench_with_input(BenchmarkId::from_parameter(depth), depth, |b, &depth| {
            b.iter(|| {
                black_box(book.get_l2_snapshot(depth));
            });
        });
    }

    group.finish();
}

fn benchmark_matching_limit_order(c: &mut Criterion) {
    c.bench_function("matching_limit_order", |b| {
        let engine = MatchingEngine::new();
        let market = create_test_market();

        // Create orderbook with liquidity
        let mut book = OrderBook::new();
        for i in 0..100 {
            let bid_price = format!("{}", 64900 + i);
            let ask_price = format!("{}", 65100 + i);

            book.insert(create_test_order(OrderSide::Buy, &bid_price, "1.0", i * 2));
            book.insert(create_test_order(OrderSide::Sell, &ask_price, "1.0", i * 2 + 1));
        }

        let incoming_order = create_test_order(OrderSide::Buy, "65150", "0.5", 10000);

        b.iter(|| {
            let mut book_clone = book.clone();
            black_box(engine.process_order(black_box(&incoming_order), &mut book_clone, &market));
        });
    });
}

fn benchmark_matching_market_order(c: &mut Criterion) {
    c.bench_function("matching_market_order", |b| {
        let engine = MatchingEngine::new();
        let market = create_test_market();

        // Create orderbook with liquidity
        let mut book = OrderBook::new();
        for i in 0..100 {
            let ask_price = format!("{}", 65000 + i * 10);
            book.insert(create_test_order(OrderSide::Sell, &ask_price, "1.0", i));
        }

        let mut market_order = create_test_order(OrderSide::Buy, "0", "5.0", 10000);
        market_order.order_type = OrderType::Market;

        b.iter(|| {
            let mut book_clone = book.clone();
            black_box(engine.process_order(black_box(&market_order), &mut book_clone, &market));
        });
    });
}

fn benchmark_decimal_arithmetic(c: &mut Criterion) {
    let mut group = c.benchmark_group("decimal");

    let a = Decimal::price("65432.12345678");
    let b = Decimal::price("12345.87654321");

    group.bench_function("add", |bench| {
        bench.iter(|| black_box(a + b));
    });

    group.bench_function("sub", |bench| {
        bench.iter(|| black_box(a - b));
    });

    group.bench_function("mul", |bench| {
        bench.iter(|| black_box(a * b));
    });

    group.bench_function("div", |bench| {
        bench.iter(|| black_box(a / b));
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_orderbook_insert,
    benchmark_orderbook_cancel,
    benchmark_l2_snapshot,
    benchmark_matching_limit_order,
    benchmark_matching_market_order,
    benchmark_decimal_arithmetic,
);

criterion_main!(benches);
