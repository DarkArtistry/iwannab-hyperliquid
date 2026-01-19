# Orders Throughput Upgrade Plan

A comprehensive plan to achieve Hyperliquid-level performance (100k+ orders/second).

## Executive Summary

| Metric | Current State | Target State | Gap |
|--------|--------------|--------------|-----|
| **Orders/Second** | ~1,000 | 100,000+ | 100x |
| **Block Time** | 500ms | <10ms | 50x |
| **Latency (p99)** | ~50ms | <1ms | 50x |
| **Concurrent Users** | ~100 | 10,000+ | 100x |

**Strategy**: Complete current implementation → Full E2E testing → Incremental upgrades → Full Hyperliquid-style rewrite

---

## Table of Contents

1. [Current Architecture Analysis](#1-current-architecture-analysis)
2. [Hyperliquid's Architecture (Reference)](#2-hyperliquids-architecture-reference)
3. [Bottleneck Identification](#3-bottleneck-identification)
4. [Upgrade Phases](#4-upgrade-phases)
5. [Detailed Implementation Plans](#5-detailed-implementation-plans)
6. [Benchmarking Strategy](#6-benchmarking-strategy)
7. [Risk Assessment](#7-risk-assessment)

---

## 1. Current Architecture Analysis

### 1.1 System Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CURRENT HYPERCORE ARCHITECTURE                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   Client                                                                     │
│     │                                                                        │
│     │ HTTP/JSON (500+ bytes per order)                                      │
│     ▼                                                                        │
│   ┌─────────────────┐                                                        │
│   │  Gateway API    │  ← JSON parsing, EIP-712 verification                 │
│   │  (Axum/Tower)   │  ← Rate limiting (new)                                │
│   └────────┬────────┘                                                        │
│            │                                                                 │
│            │ Submit to mempool                                               │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │    Mempool      │  ← Transactions queue                                 │
│   │  (Vec + Mutex)  │  ← Lock contention point                              │
│   └────────┬────────┘                                                        │
│            │                                                                 │
│            │ BlockProducer pulls transactions                                │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │  BlockProducer  │  ← 500ms block time                                   │
│   │  (Tokio task)   │  ← Sequential transaction execution                   │
│   └────────┬────────┘                                                        │
│            │                                                                 │
│            │ Execute via HyperCoreApp                                        │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │  HyperCoreApp   │  ← Transaction validation                             │
│   │  (app.rs)       │  ← Nonce checking                                     │
│   └────────┬────────┘                                                        │
│            │                                                                 │
│            │ Arc<RwLock<EngineState>>                                        │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │  EngineState    │  ← Order matching                                     │
│   │  (RwLock)       │  ← Position updates                                   │
│   └────────┬────────┘  ← Lock contention point                              │
│            │                                                                 │
│            │ State commitment                                                │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │   RocksDB       │  ← Persistence (optional)                             │
│   │  (Sync writes)  │  ← I/O bottleneck when enabled                        │
│   └─────────────────┘                                                        │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Current Performance Characteristics

| Component | Implementation | Latency | Throughput Limit |
|-----------|---------------|---------|------------------|
| **HTTP Layer** | Axum + Tower | ~1ms | ~50k req/sec |
| **JSON Parsing** | serde_json | ~10μs/order | ~100k/sec |
| **EIP-712 Verification** | k256 ECDSA | ~50μs/sig | ~20k/sec |
| **Mempool Lock** | `Mutex<Vec>` | ~1μs | Contention at scale |
| **Engine Lock** | `RwLock` | ~1μs | Contention at scale |
| **Order Matching** | BTreeMap | ~1μs/order | ~1M/sec (unlocked) |
| **Block Production** | 500ms interval | 500ms | 2 blocks/sec |
| **State Commit** | Keccak256 hash | ~100μs | ~10k/sec |
| **RocksDB Write** | Sync write batch | ~1ms | ~1k/sec |

**Bottleneck Analysis**:
1. **Block time (500ms)** - Primary bottleneck, limits to ~2k orders/sec max
2. **Lock contention** - RwLock on EngineState serializes all operations
3. **Per-order signature verification** - 50μs × 100k = 5 seconds
4. **JSON overhead** - Parsing and serialization costs

### 1.3 Code Locations

| Component | File | Key Functions |
|-----------|------|---------------|
| Gateway HTTP | `crates/gateway/src/handlers.rs` | `handle_exchange()` |
| Rate Limiting | `crates/gateway/src/rate_limit.rs` | `RateLimitService` |
| Mempool | `crates/chain/src/mempool.rs` | `Mempool::add_tx()` |
| Block Producer | `crates/chain/src/block_producer.rs` | `BlockProducer::produce_block()` |
| App Execution | `crates/chain/src/app.rs` | `HyperCoreApp::execute_tx()` |
| Order Matching | `crates/engine/src/matching.rs` | `MatchingEngine::match_order()` |
| Orderbook | `crates/engine/src/orderbook.rs` | `OrderBook::add_order()` |
| State Lock | `crates/engine/src/lib.rs` | `Arc<RwLock<EngineState>>` |

---

## 2. Hyperliquid's Architecture (Reference)

### 2.1 Public Information Sources

- [Hyperliquid Docs](https://hyperliquid.gitbook.io/hyperliquid-docs/)
- [HyperBFT Wiki](https://hyperliquid-co.gitbook.io/wiki/architecture/hyperbft)
- [Inside Hyperliquid's Technical Architecture](https://www.blockhead.co/2025/06/05/inside-hyperliquids-technical-architecture/)

### 2.2 Hyperliquid Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      HYPERLIQUID ARCHITECTURE (INFERRED)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   Clients (10,000+)                                                          │
│     │                                                                        │
│     │ Binary WebSocket Protocol (persistent connections)                     │
│     │ ~83 bytes per order (vs 500+ JSON)                                    │
│     ▼                                                                        │
│   ┌─────────────────┐                                                        │
│   │  Network Layer  │  ← Zero-copy parsing                                  │
│   │  (Custom TCP)   │  ← Connection multiplexing                            │
│   └────────┬────────┘  ← Batch accumulation                                 │
│            │                                                                 │
│            │ Batches of 1000+ orders                                         │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │  Batch Verifier │  ← SIMD signature verification                        │
│   │  (Parallel)     │  ← ~1μs per signature (batched)                       │
│   └────────┬────────┘                                                        │
│            │                                                                 │
│            │ Verified batches                                                │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │   HyperBFT      │  ← Custom consensus                                   │
│   │   Consensus     │  ← Leader-based fast path                             │
│   └────────┬────────┘  ← <10ms block time                                   │
│            │                                                                 │
│            │ Ordered transactions                                            │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │  Matching Core  │  ← SINGLE THREADED (no locks!)                        │
│   │  (Lockless)     │  ← In-memory everything                               │
│   └────────┬────────┘  ← ~100ns per order match                             │
│            │                                                                 │
│            │ Async (non-blocking)                                            │
│            ▼                                                                 │
│   ┌─────────────────┐                                                        │
│   │  State Snapshots│  ← Background persistence                             │
│   │  (Async)        │  ← Not on critical path                               │
│   └─────────────────┘                                                        │
│                                                                              │
│   Key Insight: The matching engine is NEVER blocked by I/O or locks         │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.3 Key Differences

| Aspect | HyperCore (Current) | Hyperliquid | Impact |
|--------|---------------------|-------------|--------|
| **Consensus** | CometBFT (generic) | HyperBFT (custom) | 50x latency |
| **Block Time** | 500ms | <10ms | 50x throughput |
| **Protocol** | HTTP/JSON | Binary WebSocket | 6x bandwidth |
| **Signature Verify** | Sequential | Batched SIMD | 50x throughput |
| **Engine Locking** | `RwLock` | Lockless (single-thread) | 10x throughput |
| **Persistence** | Sync (optional) | Async (always) | No I/O blocking |
| **Order Format** | JSON (~500 bytes) | Binary (~83 bytes) | 6x efficiency |

### 2.4 HyperBFT Consensus Details

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           HYPERBFT CONSENSUS                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  FAST PATH (Happy Case - Leader is Honest):                                 │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                                                                       │   │
│  │   Leader                 Validators                                   │   │
│  │     │                        │                                        │   │
│  │     │──── Propose Block ────►│  (1 round trip)                       │   │
│  │     │◄─── Vote ──────────────│                                        │   │
│  │     │                        │                                        │   │
│  │     │──── Commit ───────────►│  Block finalized!                     │   │
│  │     │                        │                                        │   │
│  │   Total: ~2 network hops = <10ms with good network                   │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
│  OPTIMISTIC EXECUTION:                                                       │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                                                                       │   │
│  │   While consensus is happening:                                       │   │
│  │   - Execute transactions speculatively                                │   │
│  │   - If consensus succeeds: commit results (instant)                  │   │
│  │   - If consensus fails: rollback (rare)                              │   │
│  │                                                                       │   │
│  │   Result: Execution is "free" - hidden behind consensus latency      │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
│  BLOCK PIPELINING:                                                           │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                                                                       │   │
│  │   Time ──────────────────────────────────────────────────►           │   │
│  │                                                                       │   │
│  │   Block N:   [Propose]──[Vote]──[Commit]                             │   │
│  │   Block N+1:          [Propose]──[Vote]──[Commit]                    │   │
│  │   Block N+2:                   [Propose]──[Vote]──[Commit]           │   │
│  │                                                                       │   │
│  │   Result: Continuous block production, no gaps                        │   │
│  │                                                                       │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Bottleneck Identification

### 3.1 Current Bottlenecks (Ranked by Impact)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    BOTTLENECK PRIORITY MATRIX                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   Impact ▲                                                                   │
│          │                                                                   │
│    HIGH  │  ┌─────────────┐  ┌─────────────┐                                │
│          │  │ #1 BLOCK    │  │ #2 ENGINE   │                                │
│          │  │ TIME        │  │ LOCKING     │                                │
│          │  │ (500ms)     │  │ (RwLock)    │                                │
│          │  └─────────────┘  └─────────────┘                                │
│          │                                                                   │
│   MEDIUM │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│          │  │ #3 SIG      │  │ #4 JSON     │  │ #5 MEMPOOL  │              │
│          │  │ VERIFY      │  │ PARSING     │  │ LOCK        │              │
│          │  │ (50μs each) │  │ (10μs each) │  │ (contention)│              │
│          │  └─────────────┘  └─────────────┘  └─────────────┘              │
│          │                                                                   │
│    LOW   │  ┌─────────────┐  ┌─────────────┐                                │
│          │  │ #6 NETWORK  │  │ #7 ROCKSDB  │                                │
│          │  │ (HTTP)      │  │ (when on)   │                                │
│          │  └─────────────┘  └─────────────┘                                │
│          │                                                                   │
│          └──────────────────────────────────────────────────────────────►   │
│                              Effort to Fix                                   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Detailed Bottleneck Analysis

#### #1: Block Time (500ms) - CRITICAL

**Current Code** (`crates/chain/src/block_producer.rs`):
```rust
pub struct BlockProducerConfig {
    pub block_time_ms: u64,  // Default: 500ms
    // ...
}

async fn produce_block(&mut self) {
    tokio::time::sleep(Duration::from_millis(self.config.block_time_ms)).await;
    // Only then produce a block
}
```

**Problem**:
- Maximum 2 blocks/second
- Even with 1000 orders/block = 2000 orders/second max
- Latency floor of 250ms average (half block time)

**Solution Path**:
1. **Quick**: Reduce to 50ms (20 blocks/sec) - 10x improvement
2. **Medium**: Reduce to 10ms (100 blocks/sec) - 50x improvement
3. **Full**: Custom consensus with <1ms blocks

---

#### #2: Engine Locking (RwLock) - CRITICAL

**Current Code** (`crates/engine/src/lib.rs`, `crates/node/src/main.rs`):
```rust
// Shared state with lock
let engine = Arc::new(RwLock::new(EngineState::new()));

// Every operation requires lock
async fn place_order(&self, order: OrderRequest) -> Result<()> {
    let mut engine = self.engine.write().await;  // ← LOCK
    engine.place_order(order)?;
}  // ← UNLOCK
```

**Problem**:
- Write lock serializes ALL order operations
- Read operations blocked during writes
- Lock overhead adds latency
- Contention increases with concurrency

**Solution Path**:
1. **Quick**: Use `parking_lot::RwLock` (faster than tokio)
2. **Medium**: Sharded locks (per-market locking)
3. **Full**: Single-threaded lockless design (Hyperliquid-style)

---

#### #3: Signature Verification (50μs each) - HIGH

**Current Code** (`crates/gateway/src/handlers.rs`):
```rust
// Each request verified individually
fn verify_signature(action: &Action, sig: &Signature) -> Result<AccountAddress> {
    let hash = compute_typed_data_hash(action);
    sig.recover(&hash)  // ← ~50μs ECDSA recovery
}
```

**Problem**:
- 50μs × 100,000 orders = 5 seconds of CPU time
- Sequential verification
- No batching or parallelization

**Solution Path**:
1. **Quick**: Parallel verification with rayon
2. **Medium**: Batch verification (aggregate signatures)
3. **Full**: SIMD-optimized batch ECDSA (like Hyperliquid)

---

#### #4: JSON Parsing (10μs each) - MEDIUM

**Current Code** (`crates/gateway/src/handlers.rs`):
```rust
let request: ExchangeRequest = serde_json::from_slice(&body)?;
```

**Problem**:
- JSON is verbose (~500 bytes per order)
- Parsing overhead per request
- Allocation for each parse

**Solution Path**:
1. **Quick**: Use `simd_json` for faster parsing
2. **Medium**: Binary protocol (MessagePack/Protobuf)
3. **Full**: Custom zero-copy binary format

---

#### #5: Mempool Lock - MEDIUM

**Current Code** (`crates/chain/src/mempool.rs`):
```rust
pub struct Mempool {
    pending: Mutex<Vec<Transaction>>,
}

pub fn add_tx(&self, tx: Transaction) {
    let mut pending = self.pending.lock().unwrap();
    pending.push(tx);
}
```

**Problem**:
- Single lock for all transactions
- Contention under load
- Lock held during push

**Solution Path**:
1. **Quick**: Use `crossbeam` lock-free queue
2. **Medium**: Multiple mempool shards
3. **Full**: Dedicated network thread with channel

---

## 4. Upgrade Phases

### Phase Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         UPGRADE ROADMAP                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  CURRENT ──────────────────────────────────────────────────────► HYPERLIQUID│
│  ~1k/sec                                                           100k+/sec│
│                                                                              │
│  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐       │
│  │ PHASE A │──►│ PHASE B │──►│ PHASE C │──►│ PHASE D │──►│ PHASE E │       │
│  │ Quick   │   │ Engine  │   │ Network │   │Consensus│   │ Full    │       │
│  │ Wins    │   │ Optimize│   │ Protocol│   │ Replace │   │ Stack   │       │
│  └─────────┘   └─────────┘   └─────────┘   └─────────┘   └─────────┘       │
│                                                                              │
│  ~5k/sec       ~15k/sec      ~30k/sec      ~60k/sec      100k+/sec         │
│                                                                              │
│  Effort:  1 week      2 weeks     3 weeks     6 weeks     8+ weeks         │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Phase A: Quick Wins (No Architecture Changes)

**Target**: 5,000 orders/second
**Effort**: ~1 week
**Risk**: Low

| Task | File | Change | Impact |
|------|------|--------|--------|
| A1. Reduce block time to 50ms | `block_producer.rs` | Change default | 10x blocks/sec |
| A2. Add batch order endpoint | `handlers.rs` | New endpoint | Amortize overhead |
| A3. Increase rate limits | `rate_limit.rs` | Config change | More requests |
| A4. Use parking_lot locks | `Cargo.toml`, `*.rs` | Replace locks | 2x lock speed |
| A5. Enable keep-alive | `server.rs` | Config change | Reduce connections |
| A6. Pre-allocate buffers | `orderbook.rs` | Pool allocations | Reduce GC |

### Phase B: Engine Optimization

**Target**: 15,000 orders/second
**Effort**: ~2 weeks
**Risk**: Medium

| Task | File | Change | Impact |
|------|------|--------|--------|
| B1. Single-threaded matching | `engine/` | Remove locks | 5x throughput |
| B2. Batch signature verify | `handlers.rs` | Parallel rayon | 10x verify speed |
| B3. Order batch processing | `app.rs` | Process in batch | Amortize overhead |
| B4. Sharded orderbooks | `orderbook.rs` | Per-market shards | Reduce contention |
| B5. Inline hot paths | `matching.rs` | `#[inline(always)]` | 10% speedup |

### Phase C: Network Protocol

**Target**: 30,000 orders/second
**Effort**: ~3 weeks
**Risk**: Medium (breaking change)

| Task | File | Change | Impact |
|------|------|--------|--------|
| C1. Binary order format | `primitives/` | New types | 6x smaller |
| C2. WebSocket binary mode | `websocket.rs` | Binary messages | Persistent conn |
| C3. Zero-copy parsing | `handlers.rs` | `bytes` crate | No allocations |
| C4. Request multiplexing | `server.rs` | Pipeline requests | Higher throughput |
| C5. UDP market data | `gateway/` | New UDP server | Broadcast efficiency |

### Phase D: Consensus Replacement

**Target**: 60,000 orders/second
**Effort**: ~6 weeks
**Risk**: High (core change)

| Task | File | Change | Impact |
|------|------|--------|--------|
| D1. Custom BFT consensus | `chain/consensus/` | New module | <10ms blocks |
| D2. Optimistic execution | `app.rs` | Speculative exec | Hide latency |
| D3. Block pipelining | `block_producer.rs` | Parallel blocks | Continuous |
| D4. Leader fast path | `consensus/` | 1-RTT commits | Faster finality |
| D5. Remove CometBFT | `Cargo.toml` | Delete dependency | Simpler stack |

### Phase E: Full Stack Optimization

**Target**: 100,000+ orders/second
**Effort**: ~8+ weeks
**Risk**: High (full rewrite)

| Task | File | Change | Impact |
|------|------|--------|--------|
| E1. SIMD batch operations | `matching.rs` | Use `packed_simd` | 4-8x speedup |
| E2. Memory-mapped state | `state.rs` | `mmap` state | Instant snapshots |
| E3. Async persistence | `persistence/` | Background writes | No I/O blocking |
| E4. CPU pinning | `main.rs` | `core_affinity` | No context switch |
| E5. NUMA optimization | `main.rs` | Memory locality | Cache efficiency |
| E6. Custom allocator | `Cargo.toml` | `jemalloc`/`mimalloc` | 20% memory perf |

---

## 5. Detailed Implementation Plans

### 5.1 Phase A: Quick Wins

#### A1. Reduce Block Time

**Current** (`crates/chain/src/block_producer.rs:15`):
```rust
pub struct BlockProducerConfig {
    pub block_time_ms: u64,  // 500ms
}
```

**Change to**:
```rust
pub struct BlockProducerConfig {
    pub block_time_ms: u64,  // 50ms (10x faster)
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            block_time_ms: 50,  // Changed from 500
            max_txs_per_block: 10_000,  // Increase batch size
            ..
        }
    }
}
```

**Impact**: 10x more blocks/second, 10x lower latency

---

#### A2. Add Batch Order Endpoint

**New endpoint** (`crates/gateway/src/handlers.rs`):
```rust
/// Batch order request - up to 100 orders in one request
#[derive(Deserialize)]
pub struct BatchOrderRequest {
    pub orders: Vec<OrderWire>,
    pub signatures: Vec<Signature>,
    pub nonce: u64,  // Single nonce for batch
}

pub async fn handle_batch_orders(
    State(state): State<AppState>,
    Json(batch): Json<BatchOrderRequest>,
) -> impl IntoResponse {
    // Validate batch size
    if batch.orders.len() > 100 {
        return Err(ApiError::BatchTooLarge);
    }

    // Verify all signatures in parallel
    let verifications: Vec<_> = batch.orders.par_iter()
        .zip(batch.signatures.par_iter())
        .map(|(order, sig)| verify_signature(order, sig))
        .collect();

    // Process all valid orders
    let results = process_order_batch(&state, batch.orders, verifications).await;

    Ok(Json(BatchOrderResponse { results }))
}
```

**Impact**: Amortizes network + parsing overhead across 100 orders

---

#### A3. Batch Signature Verification (Phase B, but easy to start)

**New module** (`crates/gateway/src/batch_verify.rs`):
```rust
use rayon::prelude::*;

/// Verify multiple signatures in parallel
pub fn batch_verify_signatures(
    messages: &[impl AsRef<[u8]>],
    signatures: &[Signature],
) -> Vec<Result<AccountAddress, VerifyError>> {
    messages.par_iter()
        .zip(signatures.par_iter())
        .map(|(msg, sig)| {
            let hash = keccak256(msg.as_ref());
            sig.recover(&hash)
        })
        .collect()
}
```

**Impact**: Uses all CPU cores, 8x speedup on 8-core machine

---

### 5.2 Phase B: Single-Threaded Matching Engine

The key insight from Hyperliquid: **a single-threaded matching engine is faster than a multi-threaded one with locks**.

#### B1. Lockless Engine Design

**New architecture**:
```rust
/// Message-passing architecture for lockless matching
pub struct MatchingCore {
    /// Channel to receive order batches
    order_rx: mpsc::Receiver<OrderBatch>,
    /// Channel to send results
    result_tx: mpsc::Sender<MatchResult>,
    /// Engine state (owned, no locks!)
    engine: EngineState,
}

impl MatchingCore {
    /// Run the matching loop - SINGLE THREAD, NO LOCKS
    pub fn run(mut self) {
        // Pin to CPU core for best cache performance
        core_affinity::set_for_current(CoreId { id: 0 });

        loop {
            // Receive batch of orders
            let batch = self.order_rx.recv().unwrap();

            // Process all orders (no locks needed!)
            let results: Vec<_> = batch.orders.into_iter()
                .map(|order| self.engine.match_order(order))
                .collect();

            // Send results back
            self.result_tx.send(MatchResult { results }).unwrap();
        }
    }
}

/// Network threads send orders to matching core via channel
pub struct GatewayToEngine {
    order_tx: mpsc::Sender<OrderBatch>,
    result_rx: mpsc::Receiver<MatchResult>,
}
```

**Why this is faster**:
1. No lock acquisition/release overhead
2. No cache invalidation from lock contention
3. Predictable memory access patterns
4. CPU branch predictor works better

---

### 5.3 Phase C: Binary Protocol

#### C1. Binary Order Format

**Current JSON** (~500 bytes):
```json
{
  "action": {
    "type": "order",
    "orders": [{
      "a": 0,
      "b": true,
      "p": "65000.5",
      "s": "0.1",
      "r": false,
      "t": {"limit": {"tif": "Gtc"}}
    }],
    "grouping": "na"
  },
  "nonce": 1234567890,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

**Binary format** (~83 bytes):
```rust
#[repr(C, packed)]
pub struct BinaryOrder {
    pub market_id: u8,           // 1 byte
    pub side: u8,                // 1 byte (0=buy, 1=sell)
    pub order_type: u8,          // 1 byte
    pub tif: u8,                 // 1 byte
    pub price: u64,              // 8 bytes (fixed-point, 8 decimals)
    pub size: u64,               // 8 bytes (fixed-point, 8 decimals)
    pub reduce_only: u8,         // 1 byte
    pub client_order_id: u64,    // 8 bytes (optional)
    pub nonce: u64,              // 8 bytes
    pub signature: [u8; 65],     // 65 bytes (r: 32, s: 32, v: 1)
}
// Total: 100 bytes (but can be 83 without cloid)

impl BinaryOrder {
    /// Zero-copy parse from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<&Self> {
        if bytes.len() < std::mem::size_of::<Self>() {
            return None;
        }
        // Safety: BinaryOrder is repr(C, packed)
        Some(unsafe { &*(bytes.as_ptr() as *const Self) })
    }
}
```

**Impact**: 6x less bandwidth, zero-copy parsing

---

### 5.4 Phase D: Custom Consensus

#### D1. HyperBFT-Style Consensus

**New module** (`crates/chain/src/hyperbft/`):

```rust
/// HyperBFT-style consensus with optimistic execution
pub struct HyperBftConsensus {
    /// Current leader
    leader: ValidatorId,
    /// Validator set
    validators: Vec<Validator>,
    /// Current view/round
    view: u64,
    /// Pending block being built
    pending_block: Option<Block>,
    /// Speculatively executed state
    speculative_state: Option<EngineState>,
}

impl HyperBftConsensus {
    /// Fast path: Leader proposes, validators vote, commit
    /// Total: 2 network hops for finality
    pub async fn fast_path_commit(&mut self, txs: Vec<Transaction>) -> Result<Block> {
        // 1. Build block
        let block = self.build_block(txs);

        // 2. Execute speculatively (in parallel with consensus)
        let exec_handle = tokio::spawn({
            let state = self.engine.clone();
            let block = block.clone();
            async move {
                execute_block_speculative(&state, &block).await
            }
        });

        // 3. Propose to validators
        let votes = self.propose_and_collect_votes(&block).await?;

        // 4. Check for supermajority (2f+1)
        if votes.len() >= self.supermajority_threshold() {
            // 5. Wait for execution to complete
            let executed_state = exec_handle.await?;

            // 6. Commit state
            self.commit_block(block, executed_state);

            return Ok(block);
        }

        // Fallback to view change if fast path fails
        self.view_change().await
    }
}
```

**Key optimizations**:
1. **Optimistic execution**: Execute while voting happens
2. **Leader fast path**: Single round trip in happy case
3. **Pipelining**: Prepare next block while committing current

---

### 5.5 Phase E: SIMD and Hardware Optimization

#### E1. SIMD Batch Operations

```rust
use std::simd::*;

/// SIMD-optimized batch price comparison
pub fn batch_price_check(
    order_prices: &[u64],
    best_prices: &[u64],
    sides: &[u8],  // 0=buy, 1=sell
) -> Vec<bool> {
    let mut results = vec![false; order_prices.len()];

    // Process 8 orders at a time with AVX-512
    for (i, chunk) in order_prices.chunks(8).enumerate() {
        let orders = u64x8::from_slice(chunk);
        let bests = u64x8::from_slice(&best_prices[i*8..]);
        let side = u8x8::from_slice(&sides[i*8..]);

        // Buy orders: price >= best_ask
        // Sell orders: price <= best_bid
        let buy_matches = orders.simd_ge(bests);
        let sell_matches = orders.simd_le(bests);

        // Select based on side
        let is_buy = side.simd_eq(u8x8::splat(0));
        let matches = is_buy.select(buy_matches, sell_matches);

        // Store results
        for (j, &matched) in matches.to_array().iter().enumerate() {
            results[i*8 + j] = matched;
        }
    }

    results
}
```

---

## 6. Benchmarking Strategy

### 6.1 Benchmark Suite

Create `benches/throughput.rs`:

```rust
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_order_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("order_matching");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("1000_orders_sequential", |b| {
        let mut engine = EngineState::new();
        let orders = generate_orders(1000);

        b.iter(|| {
            for order in &orders {
                engine.place_order(order.clone()).unwrap();
            }
        });
    });

    group.bench_function("1000_orders_batched", |b| {
        let mut engine = EngineState::new();
        let orders = generate_orders(1000);

        b.iter(|| {
            engine.place_orders_batch(&orders).unwrap();
        });
    });

    group.finish();
}

fn bench_signature_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("signature_verification");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("1000_sigs_sequential", |b| {
        let sigs = generate_signatures(1000);
        b.iter(|| {
            for sig in &sigs {
                verify_signature(sig).unwrap();
            }
        });
    });

    group.bench_function("1000_sigs_parallel", |b| {
        let sigs = generate_signatures(1000);
        b.iter(|| {
            batch_verify_signatures(&sigs);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_order_matching, bench_signature_verification);
criterion_main!(benches);
```

### 6.2 Performance Targets

| Phase | Metric | Target | How to Measure |
|-------|--------|--------|----------------|
| A | Orders/sec | 5,000 | `wrk` load test |
| A | P99 latency | 100ms | `wrk --latency` |
| B | Orders/sec | 15,000 | `wrk` load test |
| B | P99 latency | 50ms | `wrk --latency` |
| C | Orders/sec | 30,000 | Custom binary client |
| C | P99 latency | 20ms | Custom client |
| D | Orders/sec | 60,000 | Custom binary client |
| D | P99 latency | 5ms | Custom client |
| E | Orders/sec | 100,000+ | Custom binary client |
| E | P99 latency | <1ms | Custom client |

### 6.3 Load Testing Commands

```bash
# Phase A: HTTP/JSON testing
wrk -t12 -c400 -d30s -s scripts/bench/order_load.lua http://localhost:3000/exchange

# Phase B: With batch endpoint
wrk -t12 -c400 -d30s -s scripts/bench/batch_order_load.lua http://localhost:3000/batch

# Phase C+: Binary protocol (custom tool needed)
cargo run --release --bin bench_client -- --target localhost:3000 --orders 100000 --concurrency 100
```

---

## 7. Risk Assessment

### 7.1 Risk Matrix

| Phase | Risk Level | Main Risks | Mitigation |
|-------|------------|------------|------------|
| A | **Low** | Minimal changes | Thorough testing |
| B | **Medium** | Lock removal complexity | Incremental refactor |
| C | **Medium** | Breaking API changes | Version protocol |
| D | **High** | Consensus bugs | Formal verification |
| E | **High** | Unsafe code | Extensive fuzzing |

### 7.2 Rollback Strategy

Each phase should be:
1. **Feature-flagged**: Can disable new code paths
2. **Backwards compatible**: Old clients still work (except Phase C)
3. **Independently deployable**: Don't require all phases

### 7.3 Testing Requirements

| Phase | Unit Tests | Integration Tests | Load Tests | Chaos Tests |
|-------|------------|-------------------|------------|-------------|
| A | Required | Required | Required | Optional |
| B | Required | Required | Required | Required |
| C | Required | Required | Required | Required |
| D | Required | Required | Required | Required |
| E | Required | Required | Required | Required |

---

## 8. Timeline and Milestones

### Current State (Pre-Upgrade)

- [x] Core trading engine working
- [x] 246 Rust tests passing
- [x] 135 E2E tests passing
- [x] Rate limiting implemented
- [ ] Full E2E testing complete
- [ ] Production deployment tested

### Upgrade Schedule

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           UPGRADE TIMELINE                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  NOW                                                                         │
│   │                                                                          │
│   ▼                                                                          │
│  ┌─────────────────────────────────┐                                        │
│  │ Complete Current Implementation │  ← You are here                        │
│  │ - Finish any pending features   │                                        │
│  │ - Full E2E test suite           │                                        │
│  │ - Production deployment test    │                                        │
│  └─────────────────────────────────┘                                        │
│   │                                                                          │
│   ▼                                                                          │
│  ┌─────────────────────────────────┐                                        │
│  │ Phase A: Quick Wins             │  ~1 week                               │
│  │ - Reduce block time             │                                        │
│  │ - Batch endpoint                │                                        │
│  │ - Benchmark baseline            │                                        │
│  └─────────────────────────────────┘                                        │
│   │                                                                          │
│   ▼                                                                          │
│  ┌─────────────────────────────────┐                                        │
│  │ Phase B: Engine Optimization    │  ~2 weeks                              │
│  │ - Lockless matching             │                                        │
│  │ - Batch signature verify        │                                        │
│  └─────────────────────────────────┘                                        │
│   │                                                                          │
│   ▼                                                                          │
│  ┌─────────────────────────────────┐                                        │
│  │ Phase C: Binary Protocol        │  ~3 weeks                              │
│  │ - Binary order format           │                                        │
│  │ - WebSocket upgrade             │                                        │
│  └─────────────────────────────────┘                                        │
│   │                                                                          │
│   ▼                                                                          │
│  ┌─────────────────────────────────┐                                        │
│  │ Phase D: Custom Consensus       │  ~6 weeks                              │
│  │ - HyperBFT implementation       │                                        │
│  │ - Optimistic execution          │                                        │
│  └─────────────────────────────────┘                                        │
│   │                                                                          │
│   ▼                                                                          │
│  ┌─────────────────────────────────┐                                        │
│  │ Phase E: Full Optimization      │  ~8+ weeks                             │
│  │ - SIMD operations               │                                        │
│  │ - Hardware optimization         │                                        │
│  └─────────────────────────────────┘                                        │
│   │                                                                          │
│   ▼                                                                          │
│  100k+ ORDERS/SECOND                                                         │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 9. References

### Hyperliquid Resources
- [Hyperliquid Documentation](https://hyperliquid.gitbook.io/hyperliquid-docs/)
- [HyperBFT Architecture](https://hyperliquid-co.gitbook.io/wiki/architecture/hyperbft)
- [Hyperliquid API](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api)

### Technical References
- [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/) - Lock-free exchange architecture
- [Aeron](https://github.com/real-logic/aeron) - High-performance messaging
- [SBE](https://github.com/real-logic/simple-binary-encoding) - Binary encoding for finance

### Rust Performance
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Criterion.rs](https://bheisler.github.io/criterion.rs/book/) - Benchmarking
- [Rayon](https://github.com/rayon-rs/rayon) - Parallel iterators

---

## 10. Conclusion

Achieving Hyperliquid-level performance (100k+ orders/second) requires:

1. **Complete current implementation first** - Ensure stability
2. **Incremental upgrades** - Each phase delivers measurable improvement
3. **Fundamental architecture changes** - Custom consensus and lockless matching
4. **Continuous benchmarking** - Measure improvement at each step

The path from 1k to 100k orders/second is achievable but requires significant engineering effort, especially for Phases D and E. The quick wins in Phase A can provide immediate 5-10x improvement with minimal risk.

**Recommended approach**:
1. Finish current implementation
2. Full E2E testing
3. Deploy and validate current system
4. Implement Phase A (quick wins)
5. Measure and validate
6. Proceed with Phase B-E based on requirements

---

*Document created: January 2026*
*Last updated: January 2026*
*Author: HyperCore Development Team*
