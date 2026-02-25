# Implementation Plan: HyperCore Production Readiness

## Current State
- 654+ tests passing, 0 failures
- MVP complete: matching engine, risk, liquidation, funding, EVM, persistence, gateway, CometBFT consensus
- Performance optimization modules (B-E) built but NOT wired into live code
- Only 4 TODO comments remain in codebase

---

## Phase 1: Integrate Performance Optimizations (P1.1)

### 1A: Wire EngineHandle into Node Binary (HIGH IMPACT)

**Goal**: Replace `Arc<RwLock<Engine>>` with lock-free channel-based `EngineHandle`

**Current state**: `crates/node/src/main.rs` line 417 wraps engine in `Arc::new(RwLock::new(full_engine))`. The ABCI app (`crates/chain/src/cometbft/app.rs`) acquires write locks at lines 477, 672, 823, 922, 1124, 1158.

**Files to modify**:

1. **`crates/engine/src/engine_handle.rs`** — Add blocking variants for ABCI compatibility:
   ```rust
   // The ABCI finalize_block is synchronous (tendermint-abci calls it on spawn_blocking thread)
   // So we need blocking_send/blocking_recv variants
   impl EngineHandle {
       pub fn place_order_blocking(&self, ...) -> Result<...> {
           let (reply_tx, reply_rx) = oneshot::channel();
           self.tx.blocking_send(EngineCommand::PlaceOrder { ..., reply: reply_tx })?;
           reply_rx.blocking_recv()?
       }
       // Same for cancel_order_blocking, cancel_all_blocking, update_leverage_blocking,
       // process_funding_blocking, process_liquidations_blocking
   }
   ```
   Also add a `GetState` command for persistence extraction:
   ```rust
   EngineCommand::GetState { reply: oneshot::Sender<EngineStateSnapshot> }
   ```

2. **`crates/node/src/main.rs`** — Replace engine creation (around line 351-418):
   ```rust
   // OLD: let perp_engine = Arc::new(RwLock::new(full_engine));
   // NEW:
   let (engine_handle, matching_core) = EngineHandle::new(full_engine, 10_000);
   std::thread::Builder::new()
       .name("matching-core".into())
       .spawn(move || {
           matching_core.with_core_pin(0).run()
       })
       .expect("Failed to spawn matching core thread");
   ```
   Update all references from `Arc<RwLock<Engine>>` to `EngineHandle`.

3. **`crates/chain/src/app.rs`** — Change `perp_engine` field type:
   - Field declaration: change from `Arc<RwLock<Engine>>` to `EngineHandle`
   - `execute_orders_sync()` (line 337): Replace `acquire_write_with_retry(perp_engine)` → `self.state.perp_engine.place_order_blocking(...)`
   - `execute_cancel()` (line 672): Replace write lock → `cancel_order_blocking()`
   - `execute_cancel_all()` (line 823): Replace → `cancel_all_blocking()`
   - `execute_update_leverage()` (line 922): Replace → `update_leverage_blocking()`
   - `process_funding()` (line 1124): Replace → `process_funding_blocking()`
   - `process_liquidations()` (line 1158): Replace → `process_liquidations_blocking()`

4. **`crates/chain/src/cometbft/app.rs`** — State extraction for persistence:
   - `extract_state()` (line 730): Currently reads engine via `try_read()`. Replace with `GetState` command via `engine_handle.get_state_blocking()`.
   - `peek_next_order_id()` (line 439): Add cached value or query command.

5. **`crates/gateway/src/handlers.rs`** — Read-only queries:
   - Gateway info handlers (orderbook, positions, etc.) currently use `read()` locks.
   - Option A: Keep a separate `Arc<RwLock<EngineState>>` for read queries (updated after each block commit).
   - Option B: Route read queries through EngineHandle with a `GetOrderbook` command.
   - **Recommendation**: Option A (separate read snapshot) for minimal latency on info queries.

6. **`crates/chain/src/state.rs`** — AppState struct:
   - Change `perp_engine` field type from `Arc<RwLock<Engine>>` to `EngineHandle`.

**Key design decision**: The ABCI `finalize_block` runs on a `spawn_blocking` thread, so `blocking_send`/`blocking_recv` is safe and appropriate. The matching core thread processes commands sequentially, eliminating all lock contention.

**Testing**: All existing 654 tests should still pass. The matching behavior is identical — only the synchronization mechanism changes.

---

### 1B: Wire Binary Protocol into WebSocket Server

**Goal**: Replace JSON WebSocket messages with TLV-encoded binary for ~70% bandwidth savings

**Current state**: `crates/gateway/src/binary_protocol.rs` has full encode/decode for 11 message types. `crates/gateway/src/websocket.rs` has `binary_mode` flag but uses `bincode::serialize(&WsMessage)` instead of the proper TLV protocol.

**Files to modify**:

1. **`crates/gateway/src/websocket.rs`** — Outbound path (lines 288-307, sender task):
   ```rust
   // OLD: bincode::serialize(&ws_msg)
   // NEW: Match on WsMessage variant and call binary_protocol encoder
   ClientMsg::Binary(ws_msg) => {
       let bytes = match &ws_msg {
           WsMessage::L2Book { coin, data } => {
               let binary_book = convert_l2_to_binary(coin, data);
               binary_protocol::encode_l2_book(&binary_book)
           }
           WsMessage::Trade { coin, data } => {
               let binary_trade = convert_trade_to_binary(coin, data);
               binary_protocol::encode_trade(&binary_trade)
           }
           WsMessage::Fill { data } => {
               let binary_fill = convert_fill_to_binary(data);
               binary_protocol::encode_fill(&binary_fill)
           }
           // ... other variants
           _ => bincode::serialize(&ws_msg).unwrap_or_default(), // fallback
       };
       Message::Binary(bytes)
   }
   ```

2. **`crates/gateway/src/websocket.rs`** — Inbound path (lines 314-329):
   ```rust
   // OLD: simple byte[0] check
   // NEW: Use TLV decoder
   Message::Binary(data) => {
       match binary_protocol::decode_message(&data) {
           Ok((BinaryMsgType::PlaceOrder, payload)) => {
               let tx = Transaction::from_binary(payload)?;
               handle_binary_order(tx, &app_state).await;
           }
           Ok((BinaryMsgType::CancelOrder, payload)) => { ... }
           Ok((BinaryMsgType::BatchOrder, payload)) => { ... }
           _ => { /* unknown message type */ }
       }
   }
   ```

3. **`crates/gateway/src/websocket.rs`** — Add conversion functions:
   ```rust
   fn convert_l2_to_binary(coin: &str, data: &L2BookData) -> BinaryL2Book {
       // Parse string prices/sizes to u64 raw values (8 decimal places)
       // Map coin name to market_id u16
   }
   fn convert_trade_to_binary(coin: &str, data: &TradeData) -> BinaryTrade { ... }
   fn convert_fill_to_binary(data: &FillData) -> BinaryFill { ... }
   fn convert_position_to_binary(data: &PositionData) -> BinaryPosition { ... }
   ```

**Testing**: Add integration tests that connect a binary WS client, subscribe, place orders, and verify binary-encoded responses decode correctly.

---

### 1C: Complete VecPool Integration (LOW EFFORT)

**Goal**: Return fills to pool after processing to enable reuse

**Current state**: `MatchingEngine` allocates fills from pool but callers never return them.

**Files to modify**:

1. **`crates/engine/src/lib.rs`** — After `place_order()` returns fills, the caller (app.rs) processes them for events. Add a `return_fills` call.

2. **`crates/chain/src/app.rs`** — After processing fills for events (around lines 500-610):
   ```rust
   // After all fill events are generated:
   self.state.perp_engine.matching.return_fills(fills);
   ```

**Note**: With EngineHandle (1A), fills are returned within the matching core thread after the command reply is sent — even simpler.

---

### 1D: Skip huge_alloc and mmap_state (LOW PRIORITY)

**Reasoning**:
- `huge_alloc.rs`: Does NOT actually use `mmap` with `MAP_HUGETLB` — it's `vec![0u8; aligned_size]` which goes through the global allocator. No real benefit until rewritten with `libc::mmap`. Also a no-op on macOS.
- `mmap_state.rs`: Duplicates existing RocksDB persistence (which is fully integrated with 24 column families, snapshots, state sync). Not worth the integration effort.

**Recommendation**: Defer both. Focus on EngineHandle (1A) and binary protocol (1B) for real performance gains.

---

## Phase 2: Mark Price / Internal Oracle (P1.2)

**Goal**: Replace naive last-trade-price mark with a proper TWAP/EWMA-based mark price

**Current state**: Mark price is set to last fill price at `crates/engine/src/lib.rs` line 597. The `FundingEngine` has an `ewma_decay` field (0.9) that is never used. `MarketState` has `index_price` that is never updated from external sources.

### 2A: Internal Mark Price (EWMA + Impact Mid)

**Files to modify**:

1. **`crates/engine/src/state.rs`** — Add `update_mark_price()` to `EngineState`:
   ```rust
   pub fn update_mark_price(&mut self, market_id: MarketId, trade_price: Decimal) {
       if let Some(market) = self.markets.get_mut(&market_id) {
           let decay = Decimal::from_str("0.9"); // EWMA decay factor
           // EWMA: new_mark = decay * old_mark + (1 - decay) * trade_price
           let new_mark = market.state.mark_price * decay
               + trade_price * (Decimal::one() - decay);
           market.state.mark_price = new_mark;
       }
   }
   ```

2. **`crates/engine/src/lib.rs`** — Replace naive mark price update (line 597):
   ```rust
   // OLD: market.state.mark_price = Decimal::from_raw(fill.price as i128, Decimal::PRICE_DECIMALS);
   // NEW:
   self.state.update_mark_price(market_id, Decimal::from_raw(fill.price as i128, Decimal::PRICE_DECIMALS));
   ```

3. **`crates/engine/src/state.rs`** — Add impact price calculation:
   ```rust
   pub fn compute_impact_mid(&self, market_id: MarketId, impact_notional: Decimal) -> Option<Decimal> {
       // Walk orderbook bids up to impact_notional to get impact_bid
       // Walk orderbook asks up to impact_notional to get impact_ask
       // impact_mid = (impact_bid + impact_ask) / 2
       // mark_price = median(impact_mid, oracle_price, last_ewma)
   }
   ```

4. **`crates/engine/src/funding.rs`** — Update funding rate to use proper index:
   - For now, `index_price` = EWMA of trade prices (self-referential but functional)
   - Later, can be replaced with external oracle feed

### 2B: External Oracle Integration (DEFERRED)

**Deferred until needed**. The internal EWMA mark price is sufficient for a closed trading system. External oracles (Pyth/Chainlink) can be added later by:
- Adding a new `TransactionType::UpdateOraclePrice` processed by validators
- Validators fetch external prices and submit as special transactions
- Consensus ensures all validators agree on the oracle price
- `MarketState.index_price` updated from oracle, used in funding calculation

---

## Phase 3: Advanced Order Types (P2.2)

### 3A: TP/SL (TakeProfit / StopLoss) Orders

**Files to modify**:

1. **`crates/primitives/src/order.rs`** — Extend OrderRequest and Order:
   ```rust
   // Add to OrderRequest:
   pub trigger_price: Option<Decimal>,
   pub trigger_direction: Option<TriggerDirection>,

   // New enum:
   pub enum TriggerDirection {
       Above,  // Trigger when mark >= trigger_price
       Below,  // Trigger when mark <= trigger_price
   }

   // Add to Order:
   pub trigger_price: Option<Decimal>,
   pub trigger_direction: Option<TriggerDirection>,
   pub is_triggered: bool,
   ```

2. **`crates/engine/src/state.rs`** — Add trigger order storage:
   ```rust
   // In EngineState:
   /// Trigger orders: market_id -> direction -> BTreeMap<trigger_price, Vec<Order>>
   pub trigger_orders_above: HashMap<MarketId, BTreeMap<Decimal, Vec<Order>>>,
   pub trigger_orders_below: HashMap<MarketId, BTreeMap<Decimal, Vec<Order>>>,
   pub trigger_order_index: HashMap<OrderId, (MarketId, TriggerDirection, Decimal)>,
   ```
   Add methods: `add_trigger_order()`, `remove_trigger_order()`, `get_triggered_orders()`, `get_user_trigger_orders()`

3. **`crates/engine/src/lib.rs`** — Modify `place_order()`:
   ```rust
   // After validation, before matching:
   if let Some(trigger_price) = order_request.trigger_price {
       // Don't match yet — store as trigger order
       let order = Order::new_trigger(id, account, order_request, timestamp);
       self.state.add_trigger_order(order);
       return Ok(PlaceOrderResult { order_id, status: OrderStatus::Open, fills: vec![] });
   }
   // ... existing matching logic ...
   ```

4. **`crates/engine/src/lib.rs`** — Add trigger checking after mark price update:
   ```rust
   fn check_triggers(&mut self, market_id: MarketId, new_mark_price: Decimal, timestamp: Timestamp) {
       // Check Above triggers: scan from lowest to new_mark_price
       let triggered_above = self.state.get_triggered_orders(
           market_id, TriggerDirection::Above, new_mark_price
       );
       // Check Below triggers: scan from highest down to new_mark_price
       let triggered_below = self.state.get_triggered_orders(
           market_id, TriggerDirection::Below, new_mark_price
       );
       // Process in deterministic order (by trigger_price, then order_id)
       for order in triggered_above.chain(triggered_below) {
           let mut activated = order.clone();
           activated.is_triggered = true;
           activated.trigger_price = None; // Clear trigger, now it's a regular order
           self.place_order_internal(activated.owner, activated.to_request(), timestamp);
       }
   }
   ```

5. **`crates/chain/src/tx.rs`** — Add trigger fields to `OrderWire` and EIP-712 encoding.

6. **`crates/chain/src/state.rs`** — Include trigger orders in `compute_app_hash()` for consensus.

7. **`crates/gateway/src/handlers.rs`** — Update API to accept/return trigger orders.

### 3B: Conditional Order Cancellation

When a position is fully closed, cancel all TP/SL orders for that account+market. Add to `apply_fill()`:
```rust
if position.is_empty() {
    self.state.cancel_trigger_orders_for_position(account, market_id);
}
```

---

## Phase 4: Cross-Margin (P2.3)

**Current state**: `RiskEngine::is_liquidatable()` checks ONE position at a time (isolated margin). But `calculate_equity()` already sums unrealized PnL across ALL positions.

### 4A: Account-Level Liquidation Check

**Files to modify**:

1. **`crates/engine/src/risk.rs`** — Add cross-margin liquidation:
   ```rust
   pub fn is_account_liquidatable(
       &self,
       account: &AccountState,
       all_positions: &[(Position, &Market)],
   ) -> bool {
       let equity = self.calculate_equity(account, all_positions);
       let maintenance = self.calculate_maintenance_margin(all_positions);
       equity <= maintenance
   }
   ```

2. **`crates/engine/src/state.rs`** — Add margin mode:
   ```rust
   pub margin_mode: HashMap<AccountAddress, MarginMode>,

   pub enum MarginMode {
       Isolated,  // Current behavior (default)
       Cross,     // Account-level margin
   }
   ```

3. **`crates/engine/src/lib.rs`** — Update `find_underwater_accounts()`:
   ```rust
   fn find_underwater_accounts(&self) -> Vec<(AccountAddress, MarketId)> {
       let mut result = Vec::new();
       for (account_addr, account_state) in self.state.accounts.iter() {
           let mode = self.state.margin_mode.get(account_addr).unwrap_or(&MarginMode::Isolated);
           match mode {
               MarginMode::Isolated => {
                   // Current per-position check (existing code)
               }
               MarginMode::Cross => {
                   // Collect ALL positions for this account
                   let all_positions: Vec<_> = self.state.get_all_positions(*account_addr)
                       .filter_map(|(mid, pos)| {
                           self.state.get_market(mid).map(|m| (pos, m))
                       }).collect();
                   if self.risk.is_account_liquidatable(account_state, &all_positions) {
                       // Find worst position to liquidate
                       let worst = find_worst_position(&all_positions);
                       result.push((*account_addr, worst.market_id));
                   }
               }
           }
       }
       result
   }
   ```

4. **`crates/chain/src/tx.rs`** — Add `SetMarginMode` transaction type.

5. **`crates/chain/src/state.rs`** — Include margin mode in AppHash.

---

## Phase 5: Application Prometheus Metrics (P2.4)

**Goal**: Implement the `:9100/metrics` endpoint planned in `docs/MONITORING.md`

### 5A: Add Metrics Dependencies

**File**: `Cargo.toml` (workspace)
```toml
[workspace.dependencies]
prometheus = { version = "0.13", features = ["process"] }
```

### 5B: Create Metrics Registry

**New file**: `crates/engine/src/metrics.rs`
```rust
use prometheus::{Registry, IntCounter, IntGauge, Histogram, HistogramOpts};
use once_cell::sync::Lazy;

pub static METRICS: Lazy<HyperCoreMetrics> = Lazy::new(HyperCoreMetrics::new);

pub struct HyperCoreMetrics {
    pub registry: Registry,
    pub block_height: IntGauge,
    pub block_production_duration_ms: Histogram,
    pub tx_count_total: IntCounter,
    pub tx_errors_total: IntCounter,
    pub mempool_size: IntGauge,
    pub match_latency_us: Histogram,
    pub positions_total: IntGauge,
    pub open_orders_total: IntGauge,
    pub fills_total: IntCounter,
    pub liquidations_total: IntCounter,
    pub persistence_write_duration_ms: Histogram,
    pub snapshot_height: IntGauge,
    pub websocket_connections: IntGauge,
    pub evm_tx_total: IntCounter,
    pub gateway_requests_total: IntCounter,
    pub gateway_request_duration_ms: Histogram,
}
```

### 5C: Instrument Code

Add metric recording at each instrumentation point:

| Metric | File | Location |
|--------|------|----------|
| `block_height` | `crates/chain/src/cometbft/app.rs` | After `FinalizeBlock` commit |
| `block_production_duration_ms` | `crates/chain/src/block_producer.rs` | Wrap block production loop |
| `tx_count_total` | `crates/chain/src/cometbft/app.rs` | In `FinalizeBlock`, per tx |
| `mempool_size` | `crates/chain/src/mempool.rs` | After `add_tx()` and drain |
| `match_latency_us` | `crates/engine/src/matching.rs` | Wrap `process_order()` |
| `fills_total` | `crates/engine/src/matching.rs` | After each fill |
| `liquidations_total` | `crates/engine/src/lib.rs` | After each liquidation |
| `persistence_write_duration_ms` | `crates/persistence/src/persister.rs` | Wrap `persist_state()` |
| `websocket_connections` | `crates/gateway/src/websocket.rs` | On connect/disconnect |
| `gateway_requests_total` | `crates/gateway/src/server.rs` | Tower middleware layer |

### 5D: Expose Metrics Endpoint

**File**: `crates/node/src/main.rs` — Add metrics HTTP server:
```rust
// Start metrics server on port 9100
let metrics_app = Router::new()
    .route("/metrics", get(|| async {
        let encoder = TextEncoder::new();
        let metric_families = METRICS.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        (StatusCode::OK, String::from_utf8(buffer).unwrap())
    }));
tokio::spawn(async move {
    axum::serve(TcpListener::bind("0.0.0.0:9100").await.unwrap(), metrics_app).await
});
```

---

## Phase 6: Insurance Fund & Socialized Loss (P2.5)

### 6A: Wire Liquidation Fees into Insurance Fund

**Current state**: `LiquidationEngine::calculate_insurance_contribution()` exists but is never called during actual liquidation processing.

**File**: `crates/engine/src/lib.rs` — In `apply_liquidation()` (line 692):
```rust
fn apply_liquidation(&mut self, account: AccountAddress, market_id: MarketId, size: Decimal, price: Decimal) {
    // 1. Calculate liquidation and bankruptcy prices
    let liq_price = price;
    let position = self.state.get_position(account, market_id);
    let bankruptcy_price = self.liquidation.calculate_bankruptcy_price(&position, &account_state);

    // 2. Calculate residual
    let residual = (liq_price - bankruptcy_price).abs() * size;

    // 3. Credit/debit insurance fund
    if residual > Decimal::zero() {
        self.state.add_to_insurance_fund(residual.raw());
    } else {
        let deficit = residual.abs();
        if !self.state.use_insurance_fund(deficit.raw()) {
            // Insurance fund depleted — trigger ADL
            self.trigger_adl(market_id, deficit);
        }
    }

    // 4. Apply the fill to close position
    position.apply_fill(size, price, is_buy);
}
```

### 6B: ADL Execution

```rust
fn trigger_adl(&mut self, market_id: MarketId, deficit: Decimal) {
    // Find most profitable counter-party using ADL scoring
    let counter_parties = self.find_adl_candidates(market_id);
    for (counter_addr, score) in counter_parties {
        let counter_pos = self.state.get_position(counter_addr, market_id);
        let adl_size = min(counter_pos.size.abs(), remaining_deficit_in_size);
        // Force-close their position at bankruptcy price
        self.apply_adl(counter_addr, market_id, adl_size, bankruptcy_price);
        // Track the ADL event
    }
}
```

### 6C: Update Open Interest Tracking

**File**: `crates/engine/src/lib.rs` — In `apply_fill()`:
```rust
// After updating positions, update market open interest
if position_increased {
    market.state.open_interest_long += fill_size; // or open_interest_short
} else {
    market.state.open_interest_long -= fill_size; // proportional
}
```

---

## Phase 7: HyperBFT Production-Ready (P2.1) — DEFERRED

**Reasoning**: CometBFT works for 1k-5k TPS. HyperBFT requires significant engineering for production safety (real signatures, P2P transport, view change, etc.). Defer until CometBFT throughput is actually the bottleneck.

**When needed, the critical changes are**:
1. Replace `DefaultHasher` with SHA-256 in `compute_proposal_hash()` (`runner.rs:35-56`)
2. Add Ed25519 signatures using existing `AttestationKeyPair` (`runner.rs:238,327`)
3. Separate proposal from execution/commit (`runner.rs:315`)
4. Add `ViewChange` message type and 2f+1 timeout certificate collection
5. Replace mpsc channels with libp2p transport (extend existing attestation gossip)
6. Wire into `main.rs` as `ConsensusMode::HyperBft`

---

## Phase 8: External Bridge (P1.3) — DEFERRED

**Reasoning**: Largest scope item. Requires L1 smart contract, bridge oracle, deposit/withdrawal processing, and significant consensus changes. Not needed for testnet/devnet.

**When needed**:
1. Deploy bridge contract on L1 (Ethereum/Arbitrum) for USDC escrow
2. Add `TransactionType::DepositAttestation` — validators attest to L1 deposit events
3. Add quorum logic — credit `UnifiedState` when 2/3+ validators attest
4. Add withdrawal processing via CoreWriter (already has `Withdraw` action type ID=4)
5. Add `Withdrawals` column family to persistence
6. Withdrawal finality: epoch-based Merkle commitment to L1

---

## Recommended Build Order

| # | Phase | Effort | Impact | Can Parallelize |
|---|-------|--------|--------|-----------------|
| 1 | **1A: EngineHandle integration** | High | Very High | No (foundational) |
| 2 | **2A: EWMA mark price** | Low | High | Yes (independent) |
| 3 | **1B: Binary WS protocol** | Medium | Medium | Yes (independent) |
| 4 | **3A: TP/SL orders** | Medium | High | After 2A (needs mark price) |
| 5 | **5: Prometheus metrics** | Low | Medium | Yes (independent) |
| 6 | **4A: Cross-margin** | Medium | High | After 1A |
| 7 | **6: Insurance fund/ADL** | Medium | Medium | After 4A |
| 8 | **1C: VecPool return** | Low | Low | Yes |

**Parallel track 1** (core engine): 1A → 4A → 6
**Parallel track 2** (features): 2A → 3A
**Parallel track 3** (infrastructure): 1B, 5, 1C (all independent)

---

## Files Reference Quick-Look

### Key files that will be modified across all phases:

| File | Phases | Why |
|------|--------|-----|
| `crates/engine/src/lib.rs` | 1A, 2A, 3A, 4A, 6 | Core engine, order placement, liquidation |
| `crates/engine/src/engine_handle.rs` | 1A | Add blocking variants, GetState command |
| `crates/engine/src/state.rs` | 2A, 3A, 4A | Mark price, trigger orders, margin mode |
| `crates/engine/src/risk.rs` | 4A | Cross-margin liquidation check |
| `crates/engine/src/matching.rs` | 5 | Metrics instrumentation |
| `crates/primitives/src/order.rs` | 3A | Trigger price fields |
| `crates/chain/src/app.rs` | 1A, 3A | EngineHandle integration, trigger order tx |
| `crates/chain/src/cometbft/app.rs` | 1A, 5 | Engine access via handle, metrics |
| `crates/chain/src/state.rs` | 3A, 4A | AppHash for new state |
| `crates/chain/src/tx.rs` | 3A, 4A | New transaction types |
| `crates/gateway/src/websocket.rs` | 1B | Binary protocol integration |
| `crates/gateway/src/handlers.rs` | 1A, 3A | Engine access, trigger order API |
| `crates/node/src/main.rs` | 1A, 5 | Engine creation, metrics server |
| `Cargo.toml` | 5 | prometheus dependency |
