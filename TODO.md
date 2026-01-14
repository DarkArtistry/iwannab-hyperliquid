# HyperCore Development TODO

Prioritized list of outstanding work items organized by criticality and phase.

## Legend
- 🔴 **P0 - Critical** - Blocks core functionality
- 🟠 **P1 - High** - Required for MVP
- 🟡 **P2 - Medium** - Important for production
- 🟢 **P3 - Low** - Nice to have

---

## Phase 1: EVM Integration ✅ COMPLETED (100%)

### Phase 1a: EVM JSON-RPC Server ✅ COMPLETED

**File created:** `crates/evm/src/rpc.rs`

```
[x] Create JSON-RPC server using jsonrpsee
[x] Implement all eth_* methods (chainId, blockNumber, gasPrice, etc.)
[x] Implement web3_clientVersion, net_version
[x] Upgrade revm to 19.0 with API compatibility fixes
[x] Update bytecode to solc 0.8.29 (PUSH0 opcode support)
[x] All 20 EVM unit tests passing
```

**File modified:** `crates/evm/src/executor.rs`

```
[x] Replace stub execute_tx() with real revm execution
[x] Create HyperEvmDb that wraps EvmState with Database trait
[x] Handle precompile calls directly before EVM execution
[x] Implement proper gas accounting via revm
[x] Handle transaction receipts and logs
[x] Implement state commitment via DatabaseCommit
```

**File modified:** `crates/node/src/main.rs`

```
[x] Create EvmRpcServer instance
[x] Start RPC server on port 8545 (configurable)
[x] Add --evm-rpc-addr CLI flag
[x] Wire up executor to RPC handlers
```

### Phase 1b: Token Standard Support ✅ COMPLETED

**File modified:** `scripts/e2e/runner.ts`

```
[x] Add ERC20 token deployment tests
[x] Add ERC20 metadata and transfer tests
[x] Add ERC721 NFT deployment tests
[x] Add ERC721 mint tests
[x] Add ERC1155 multi-token deployment tests
[x] Add ERC1155 mint tests
[x] All 8 token tests pass - verified with Docker rebuild!
```

### Phase 1c: HIP-1 Style Token Integration ✅ COMPLETED

**References:**
- [HIP-1: Native token standard](https://hyperliquid.gitbook.io/hyperliquid-docs/hyperliquid-improvement-proposals-hips/hip-1-native-token-standard)
- [HyperEVM Architecture](https://hyperliquid.gitbook.io/hyperliquid-docs/hyperevm)

**All Items Completed:**
```
[x] Add spot token primitives (crates/primitives/src/spot.rs)
    - SpotToken, SpotBalance, SpotMarketConfig
    - TokenIndex, system address derivation
[x] Implement SpotEngine (crates/engine/src/spot_engine.rs)
    - Token deployment with genesis allocations
    - Spot orderbook with price-time priority
    - Balance-based trading (no margin/leverage)
    - All 3 spot engine unit tests passing
[x] Create SpotToken.sol contract (contracts/src/SpotToken.sol)
    - HIP-1 style ERC20 with bridge support
    - SpotTokenFactory for deployment
[x] Add spot API endpoints to gateway
    - SpotMeta, SpotL2Book, SpotAllMids, SpotBalances, SpotOpenOrders
    - SpotOrder, SpotCancel, SpotCancelAll exchange actions
[x] Add spot precompiles (0x0806-0x0808)
    - SpotBalance: Get token balance for address
    - SpotMarket: Get spot market info
    - SpotOrderBook: Get L2 orderbook snapshot
[x] Wire up SpotEngine to node main.rs
    - GatewayServer.with_spot_engine() constructor
    - Default TEST-USDC spot market initialized
    - Test accounts credited with USDC and TEST balances
[x] Comprehensive E2E tests for spot trading (12 tests)
    - SpotMeta, SpotL2Book, SpotAllMids queries
    - SpotBalances with balance verification
    - SpotOpenOrders with/without market filter
    - SpotTokenInfo queries
    - Spot order placement and verification
    - Order cancellation and verification
```

---

## Phase 2: Unified State & Consensus Integration

### Phase 2A: Unified State Refactor ✅ COMPLETED (100%)

**Reference:** [Inside Hyperliquid's Technical Architecture](https://www.blockhead.co/2025/06/05/inside-hyperliquids-technical-architecture/)

#### ✅ P0: Create UnifiedState Layer - COMPLETED

**File created:** `crates/primitives/src/unified_state.rs`

```
[x] Create UnifiedBalance struct with views:
    - total: Decimal (source of truth)
    - core_view: Decimal (available for trading)
    - evm_view: Decimal (available for EVM)
    - Invariant: total == core_view + evm_view
    - is_valid() method to verify invariant

[x] Create UnifiedState struct:
    - balances: HashMap<(AccountAddress, TokenIndex), UnifiedBalance>
    - Single state, two views

[x] Create SharedUnifiedState type: Arc<RwLock<UnifiedState>>
```

#### ✅ P0: Refactor SpotEngineState to use UnifiedState - COMPLETED

**File modified:** `crates/engine/src/spot_engine.rs`

```
[x] Replace SpotEngineState.balances with SharedUnifiedState reference (line 50)
[x] Implement get_core_view() for reading core balance (line 187)
[x] Implement reserve_balance() for order reservation
[x] Implement release_balance() for releasing reservations
[x] Update place_order() to use unified balance + reserved tracking
[x] All 8 spot engine unit tests passing
```

#### ✅ P0: Refactor EvmState to use UnifiedState - COMPLETED

**File modified:** `crates/evm/src/state.rs`

```
[x] Replace EvmState balance storage with SharedUnifiedState reference (line 53)
[x] Implement get_evm_view() for reading EVM balance (line 134)
[x] Implement debit_evm() for gas/transfers
[x] Implement credit_evm() for receives
[x] EVM nonce/storage remains in EvmState (not unified)
[x] All EVM unit tests passing
```

#### ✅ P0: Implement View Transfer (Not Bridging!) - COMPLETED

**File modified:** `crates/primitives/src/unified_state.rs`

```
[x] Implement UnifiedState.transfer_to_evm_view() (line 203):
    - Verify core_view >= amount
    - core_view -= amount
    - evm_view += amount
    - total unchanged!

[x] Implement UnifiedState.transfer_to_core_view() (line 234):
    - Verify evm_view >= amount
    - evm_view -= amount
    - core_view += amount
    - total unchanged!

[x] Both methods verify invariant with debug_assert!
```

#### ✅ P0: Shared Process Architecture - COMPLETED

**File modified:** `crates/node/src/main.rs`

```
[x] Create single SharedUnifiedState instance (line 121)
[x] SpotEngine uses Arc::clone(&unified_state) (line 129)
[x] EvmExecutor uses Arc::clone(&unified_state) (line 149)
[x] Gateway and EVM RPC run in same process, share state
[x] Docker compose updated to expose port 3000 from node
```

#### ✅ P1: Update E2E Tests for Unified State - COMPLETED

**File modified:** `scripts/e2e/runner.ts`

```
[x] Test: EVM balance reflects unified state evm_view
[x] Test: Reserved balance prevents over-transfer
[x] Test: View transfers work correctly
[x] 104+ E2E tests passing
```

---

### Phase 2B: Connect ABCI to CometBFT 🔴 NEXT PRIORITY

The ABCI server exists but just sleeps forever instead of connecting to CometBFT.
Now that Phase 2A (Unified State) is complete, this is the next critical step.

**Prerequisites (all from Phase 2A - DONE):**
- ✅ Single `UnifiedState` to commit
- ✅ Gateway and EVM RPC share same process
- ✅ View transfer methods ready

#### 🔴 P0: Connect ABCI to CometBFT

**File to modify:** `crates/node/src/main.rs`

```
[ ] Add tendermint-abci crate dependency
[ ] Replace sleep with actual ABCI server
[ ] Configure connection to CometBFT node
[ ] Handle ABCI callbacks properly
```

**Current stub location:**
```rust
// main.rs:160-163
// In production, integrate with tendermint-abci crate
// For now, just keep the task alive
tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;
```

#### 🟠 P1: Wire Transaction Processing to Engine

**File to modify:** `crates/chain/src/app.rs`

```
[ ] Fix borrow checker issues in execute_orders()
[ ] Implement Order action processing
[ ] Implement Cancel action processing
[ ] Implement CancelByCloid processing
[ ] Implement CancelAll processing
[ ] Implement UsdTransfer processing
[ ] Implement Withdraw processing
[ ] Implement EvmAction processing
[ ] Implement ViewTransfer action (Core ↔ EVM view transfers)
    - Note: UnifiedState.transfer_to_evm_view() already implemented!
    - Note: UnifiedState.transfer_to_core_view() already implemented!
[ ] Connect to UnifiedState for balance operations
    - Note: SharedUnifiedState type ready from Phase 2A
```

**Current stub location:**
```rust
// app.rs - Multiple TODO markers
// TODO: Implement order processing
// TODO: Implement cancellation
```

#### 🟠 P1: Implement State Commitment

**File to modify:** `crates/chain/src/state.rs`

```
[ ] Implement proper state root computation (Merkle tree)
[ ] Include UnifiedState in commitment (single state root!)
    - Note: UnifiedState with all views ready from Phase 2A
[ ] Store block hashes correctly
[ ] Implement app_hash computation
```

---

## Phase 3: Gateway Completion

### 🟠 P1: Implement Signature Verification

**File to modify:** `crates/gateway/src/handlers.rs`

```
[ ] Implement proper EIP-712 signature verification
[ ] Remove test stub that extracts address from r value
[ ] Add proper error handling for invalid signatures
[ ] Verify nonce to prevent replay attacks
```

**Current security bypass:**
```rust
// handlers.rs - verify_signature()
// STUB: Currently just extracts "address" from r value for testing!
```

### 🟡 P2: Complete Query Handlers

**File to modify:** `crates/gateway/src/handlers.rs`

```
[ ] Implement UserFills query
[ ] Implement FundingHistory query
[ ] Implement UserFundingHistory query
[ ] Implement RecentTrades query
[ ] Implement CandleSnapshot query
```

### 🟡 P2: WebSocket Broadcasting

**File to modify:** `crates/gateway/src/websocket.rs`

```
[ ] Implement trade broadcast on fills
[ ] Implement orderbook update broadcasts
[ ] Implement user-specific update broadcasts
[ ] Fix unsubscribe cleanup
```

---

## Phase 4: Persistence

### 🟠 P1: Add State Persistence

```
[ ] Choose storage backend (RocksDB recommended)
[ ] Add persistence crate or module
[ ] Implement write-ahead log for crash recovery
[ ] Persist engine state on commit
[ ] Persist EVM state on commit
[ ] Load state on node startup
```

### 🟡 P2: Implement Export/Import

**File to modify:** `crates/node/src/main.rs`

```
[ ] Implement export command (state snapshot to file)
[ ] Implement import command (restore from snapshot)
```

### 🟡 P2: Enable State Sync

**File to modify:** `crates/chain/src/abci.rs`

```
[ ] Implement ListSnapshots
[ ] Implement OfferSnapshot
[ ] Implement LoadSnapshotChunk
[ ] Implement ApplySnapshotChunk
```

---

## Phase 5: Indexer Completion

### 🟡 P2: Connect Indexer to Node

**File to modify:** `crates/node/src/main.rs`

```
[ ] Start indexer service when enabled
[ ] Pass engine events to indexer
[ ] Handle indexer errors gracefully
```

### 🟡 P2: Fix Indexer Engine Calls

**File to modify:** `crates/indexer/src/ingest.rs`

```
[ ] Add missing engine methods for block info
[ ] Fix event processing
[ ] Test full indexing pipeline
```

---

## Phase 6: Security Hardening

### 🟡 P2: Input Validation

```
[ ] Validate all user inputs
[ ] Add size limits
[ ] Sanitize addresses
[ ] Validate prices and sizes against market config
```

### 🟡 P2: Rate Limiting

```
[ ] Add rate limiting to gateway
[ ] Implement per-IP limits
[ ] Implement per-account limits
```

### 🟢 P3: Security Audit

```
[ ] Internal security review
[ ] External security audit
[ ] Bug bounty program
```

---

## Phase 7: Testing & Documentation

### 🟡 P2: Integration Tests

```
[ ] Add Rust integration tests for ABCI flow
[ ] Add EVM execution tests
[ ] Add consensus simulation tests
[ ] Improve E2E test coverage
```

### 🟢 P3: Documentation

```
[ ] API documentation with examples
[ ] Deployment guide
[ ] Operator manual
[ ] SDK tutorials
```

---

## Missing Engine Methods

These methods are referenced but not implemented in `crates/engine/src/state.rs`:

```rust
// Block info
[ ] current_height() -> u64
[ ] get_block_hash(height) -> Option<[u8; 32]>
[ ] get_block_timestamp(height) -> Option<u64>
[ ] get_block_tx_count(height) -> Option<usize>
[ ] get_block_events(height) -> Vec<Event>

// Queries
[ ] get_open_orders(user, market) -> Vec<Order>
[ ] get_user_fills(user, start, end) -> Vec<Fill>
[ ] get_funding_history(market) -> Vec<FundingEvent>
[ ] get_user_funding_history(user) -> Vec<FundingPayment>
[ ] get_recent_trades(market, limit) -> Vec<Trade>
[ ] get_candles(market, interval, start, end) -> Vec<Candle>

// Market info
[ ] get_market_id_by_name(name) -> Option<MarketId>
[ ] get_market_name(id) -> Option<String>
[ ] get_all_markets() -> Vec<&Market>
[ ] get_all_mid_prices() -> HashMap<String, Decimal>

// Funding
[ ] last_funding_time(market) -> u64
[ ] set_last_funding_time(market, time)
[ ] apply_funding_payment(user, market, amount)
[ ] should_apply_funding(market) -> bool
[ ] apply_funding(market)

// Liquidation
[ ] apply_liquidation(liquidation_result)

// State
[ ] compute_state_root() -> [u8; 32]
```

---

## Missing Primitives Methods

These methods are referenced but not implemented in `crates/primitives/`:

```rust
// Decimal
[ ] scale_to(decimals: u8) -> Self
[ ] to_be_bytes() -> [u8; 32]
[ ] to_be_bytes_signed() -> [u8; 32]
[ ] is_negative() -> bool

// Position
[ ] unrealized_pnl field
[ ] margin_used field
[ ] liquidation_price field
[ ] leverage field
[ ] is_long() method

// Order
[ ] is_resting_order() method

// MarketState struct (may be missing entirely)
```

---

## Quick Wins (Can be done independently)

1. ~~**Fix Docker healthcheck** - Currently checks 8545 which doesn't exist~~ ✅ EVM RPC server now exists

2. **Add missing Decimal methods** - Required by precompiles
   - Implement scale_to, to_be_bytes, etc.

3. **Add Position fields** - Required by precompiles
   - Add unrealized_pnl, margin_used, etc.

4. ~~**Update CLI help** - Document actual behavior~~ ✅ EVM is now functional

---

## Estimated Effort

| Phase | Focus | Dependencies | Status |
|-------|-------|--------------|--------|
| Phase 1a (EVM RPC) | JSON-RPC server | None | ✅ **COMPLETED** |
| Phase 1b (Token Standards) | ERC20/721/1155 | Phase 1a | ✅ **COMPLETED** |
| Phase 1c (HIP-1 Tokens) | Spot trading | Phase 1b | ✅ **COMPLETED** |
| **Phase 2A (Unified State)** | **Master balance sheet** | **Phase 1** | ✅ **COMPLETED** |
| **Phase 2B (CometBFT)** | **Consensus connection** | **Phase 2A** | **🔴 NEXT** |
| Phase 3 (Gateway) | Signatures, view transfer API | Phase 2B | ⚠️ Partial |
| Phase 4 (Persistence) | RocksDB, WAL | Phase 2B | Pending |
| Phase 5 (Indexer) | Historical data | Phase 4 | Pending |
| Phase 6 (Security) | Hardening | Phase 1-5 | Pending |
| Phase 7 (Testing) | Comprehensive testing | Phase 1-5 | Ongoing |

**Phase 1 + 2A Complete!** EVM integration with unified state model is fully implemented.

### Phase 2A Completion Summary

The unified state model matching Hyperliquid's architecture is now implemented:

| Component | Implementation |
|-----------|----------------|
| `UnifiedBalance` struct | `crates/primitives/src/unified_state.rs:37` |
| `UnifiedState` struct | `crates/primitives/src/unified_state.rs:85` |
| `SharedUnifiedState` type | `Arc<RwLock<UnifiedState>>` |
| SpotEngine integration | `crates/engine/src/spot_engine.rs:50` |
| EvmState integration | `crates/evm/src/state.rs:53` |
| Shared process | `crates/node/src/main.rs:121-149` |
| View transfers | `transfer_to_evm_view()`, `transfer_to_core_view()` |

### Why Phase 2B (CometBFT) is Next

With unified state complete, we can now properly integrate consensus:

- Single `UnifiedState` to commit (not two separate states)
- Correct architecture for multi-node deployment
- State root computation will include all balance views
- ABCI server ready for real consensus connection
