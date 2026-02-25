//! Block production pipeline benchmarks
//!
//! Measures the full block production pipeline:
//! - begin_block / execute_tx / end_block / commit
//! - Transaction serialization and deserialization
//! - Multi-transaction blocks

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use hypercore_chain::app::HyperCoreApp;
use hypercore_chain::tx::{Transaction, TransactionType};
use hypercore_primitives::Signature;

/// Create a CancelAll transaction with a zero signature.
///
/// CancelAll is the simplest transaction type: it has no parameters,
/// minimal EIP-712 encoding overhead, and exercises the full execute_tx
/// path including nonce validation and event generation.
fn make_cancel_all_tx(nonce: u64) -> Transaction {
    Transaction {
        action: TransactionType::CancelAll,
        nonce,
        signature: Signature::zero(),
        hash: None,
    }
}

/// Benchmark: single-transaction block pipeline.
///
/// Measures the cost of the full begin_block -> execute_tx -> end_block -> commit
/// cycle with a single CancelAll transaction per block.
fn bench_single_tx_block(c: &mut Criterion) {
    c.bench_function("block_single_tx", |b| {
        b.iter_with_setup(
            || {
                let app = HyperCoreApp::new();
                app
            },
            |mut app| {
                let tx = make_cancel_all_tx(0);
                let height = 1u64;
                let timestamp = 1000u64;

                app.begin_block(height, timestamp);
                // CancelAll with zero signature will fail sender recovery,
                // but we still measure the execution path overhead
                let _ = black_box(app.execute_tx(&tx, timestamp));
                let _ = app.end_block();
                let _ = black_box(app.commit());
            },
        );
    });
}

/// Benchmark: multi-transaction blocks.
///
/// Measures how block production scales with the number of transactions
/// per block. This is critical for throughput optimization.
fn bench_multi_tx_block(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_multi_tx");

    for &tx_count in &[10u64, 50, 100] {
        group.throughput(Throughput::Elements(tx_count));
        group.bench_with_input(
            BenchmarkId::from_parameter(tx_count),
            &tx_count,
            |b, &n| {
                b.iter_with_setup(
                    || {
                        let app = HyperCoreApp::new();
                        let txs: Vec<Transaction> = (0..n)
                            .map(|i| make_cancel_all_tx(i))
                            .collect();
                        (app, txs)
                    },
                    |(mut app, txs)| {
                        let height = 1u64;
                        let timestamp = 1000u64;

                        app.begin_block(height, timestamp);
                        for tx in &txs {
                            let _ = black_box(app.execute_tx(tx, timestamp));
                        }
                        let _ = app.end_block();
                        let _ = black_box(app.commit());
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: sequential block production.
///
/// Measures the cost of producing N consecutive blocks, each with
/// a single transaction. This captures any per-block overhead that
/// accumulates over time (merkle tree growth, state cleanup, etc.).
fn bench_sequential_blocks(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_sequential");

    for &block_count in &[10u64, 50] {
        group.throughput(Throughput::Elements(block_count));
        group.bench_with_input(
            BenchmarkId::from_parameter(block_count),
            &block_count,
            |b, &n| {
                b.iter_with_setup(
                    HyperCoreApp::new,
                    |mut app| {
                        for height in 1..=n {
                            let timestamp = height * 1000;
                            let tx = make_cancel_all_tx(height - 1);

                            app.begin_block(height, timestamp);
                            let _ = app.execute_tx(&tx, timestamp);
                            let _ = app.end_block();
                            let _ = black_box(app.commit());
                        }
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: transaction serialization (JSON).
///
/// Measures the cost of serializing a Transaction to JSON bytes,
/// which is the format used when broadcasting to CometBFT.
fn bench_tx_serialization(c: &mut Criterion) {
    let tx = make_cancel_all_tx(12345);

    c.bench_function("tx_serialize_json", |b| {
        b.iter(|| {
            let _ = black_box(serde_json::to_vec(&tx));
        });
    });
}

/// Benchmark: transaction deserialization (JSON).
///
/// Measures the cost of deserializing a Transaction from JSON bytes,
/// which happens for every transaction in FinalizeBlock.
fn bench_tx_deserialization(c: &mut Criterion) {
    let tx = make_cancel_all_tx(12345);
    let bytes = serde_json::to_vec(&tx).unwrap();

    c.bench_function("tx_deserialize_json", |b| {
        b.iter(|| {
            let _ = black_box(serde_json::from_slice::<Transaction>(&bytes));
        });
    });
}

/// Benchmark: begin_block + end_block overhead without any transactions.
///
/// Isolates the per-block overhead: merkle tree computation, funding
/// rate processing, liquidation scanning, and state commitment.
fn bench_empty_block_overhead(c: &mut Criterion) {
    c.bench_function("block_empty_overhead", |b| {
        b.iter_with_setup(
            HyperCoreApp::new,
            |mut app| {
                let height = 1u64;
                let timestamp = 1000u64;

                app.begin_block(height, timestamp);
                let _ = app.end_block();
                let _ = black_box(app.commit());
            },
        );
    });
}

criterion_group!(
    benches,
    bench_single_tx_block,
    bench_multi_tx_block,
    bench_sequential_blocks,
    bench_tx_serialization,
    bench_tx_deserialization,
    bench_empty_block_overhead,
);

criterion_main!(benches);
